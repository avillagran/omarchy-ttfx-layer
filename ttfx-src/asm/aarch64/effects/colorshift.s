// effects/colorshift.s - "Display a gradient that shifts colors across the
// terminal" (src/effects/colorshift.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Colorshift). The effect draws
// nothing from the RNG.
//
// Per character: ch_user0 is loop_tracker_map's count, ch_user1 holds the
// "gradient" scene (low half) and "final_gradient" (high half).

.equ HAVE_colorshift, 1

.equ COLORSHIFT.stops,              0       // *const u64 gradient_stops
.equ COLORSHIFT.stop_count,         8
.equ COLORSHIFT.steps,              16      // *const i64 gradient_steps
.equ COLORSHIFT.step_count,         24
.equ COLORSHIFT.frames,             32      // gradient_frames (fits 32 bits)
.equ COLORSHIFT.no_travel,          40
.equ COLORSHIFT.travel_direction,   48      // GradientDirection
.equ COLORSHIFT.reverse,            56      // reverse_travel_direction
.equ COLORSHIFT.no_loop,            64
.equ COLORSHIFT.cycles,             72
.equ COLORSHIFT.skip_final,         80      // skip_final_gradient
.equ COLORSHIFT.final_stops,        88      // *const u64
.equ COLORSHIFT.final_stop_count,   96
.equ COLORSHIFT.final_steps,        104     // *const i64
.equ COLORSHIFT.final_step_count,   112
.equ COLORSHIFT.final_direction,    120
.equ COLORSHIFT_size,               128

// scene names
.equ CS_GRADIENT,           NAME_LITERAL + 0
.equ CS_FINAL_GRADIENT,     NAME_LITERAL + 1

.equ CS_MEMO_BITS,          8           // cs_memo entries: 1 << CS_MEMO_BITS

    .text

// colorshift_build: ColorShift::build.
colorshift_build:
    stp     x19, x20, [sp, #-80]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56] = frame counter, [sp, #64] = spectrum index
    bl      cs_final_color_map
    bl      cs_gradient
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    mov     x19, #0
colorshift_build.char:
    cmp     x19, x22
    b.hs    colorshift_build.built
    ldr     w20, [x21, x19, lsl #2]
    mov     w0, w20
    bl      set_visible
    bl      cs_rotation                 // x9 = k, the rotated spectrum's start
    str     x9, [sp, #64]
    // "gradient": the rotated spectrum, gradient_frames per color
    mov     w0, w20
    MOV64   w1, CS_GRADIENT
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w23, w9
    LDX     x9, ch_sym
    ldr     x0, [x9, x20, lsl #3]
    bl      cs_symbol_handles
    mov     x24, x9
    LDX     x9, cs_len
    str     x9, [sp, #56]
colorshift_build.frame:
    ldr     x9, [sp, #64]
    ldr     w1, [x24, x9, lsl #2]
    cbnz    w1, colorshift_build.cached
    LDX     x3, cs_spectrum
    ldr     x0, [x3, x9, lsl #3]
    mov     x1, #NONE
    LDX     x2, ch_sym
    ldr     x2, [x2, x20, lsl #3]
    mov     w3, #0
    bl      visual_make
    ldr     x3, [sp, #64]
    str     w9, [x24, x3, lsl #2]
    mov     w1, w9
colorshift_build.cached:
    ldr     x9, [sp, #64]
    add     x9, x9, #1
    LDX     x10, cs_len
    cmp     x9, x10
    b.lo    colorshift_build.wrapped
    mov     x9, #0
colorshift_build.wrapped:
    str     x9, [sp, #64]
    mov     w0, w23
    LDX     x2, effect_config
    ldr     w2, [x2, #COLORSHIFT.frames]
    bl      scene_add_frame_visual
    ldr     x9, [sp, #56]
    subs    x9, x9, #1
    str     x9, [sp, #56]
    b.ne    colorshift_build.frame
    // the last color shown: spectrum[k - 1], wrapping
    ldr     x9, [sp, #64]
    cbnz    x9, colorshift_build.prev
    LDX     x9, cs_len
colorshift_build.prev:
    LDX     x3, cs_spectrum
    add     x3, x3, x9, lsl #3
    ldur    x9, [x3, #-8]
    STX     x9, cs_pair
    mov     w0, w20
    MOV64   w1, CS_FINAL_GRADIENT
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9
    LDX     x3, ch_user1
    add     x3, x3, x20, lsl #3
    str     w23, [x3]
    str     w24, [x3, #4]
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    colorshift_build.dynamic
    // last color -> the final gradient's color at the input coordinate
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x20, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, cs_final_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x20, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, cs_final_map
    ldr     x9, [x3, x9, lsl #3]
    ADRG    x4, cs_fg_spectrum
    bl      cs_pair_gradient
    mov     w23, w9
    str     xzr, [sp, #56]
colorshift_build.final_frame:
    ldr     x9, [sp, #56]
    cmp     w9, w23
    b.hs    colorshift_build.activate
    add     x10, x9, #1
    str     x10, [sp, #56]
    ADRG    x3, cs_fg_spectrum
    ldr     x3, [x3, x9, lsl #3]
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    mov     w0, w24
    LDX     x2, effect_config
    ldr     w2, [x2, #COLORSHIFT.frames]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       colorshift_build.final_frame
colorshift_build.dynamic:
    // last color -> the input colors, those the character has
    mov     w23, #0                     // fg count
    LDX     x9, ch_fg
    ldr     x9, [x9, x20, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    colorshift_build.bg
    ADRG    x4, cs_fg_spectrum
    bl      cs_pair_gradient
    mov     w23, w9
colorshift_build.bg:
    str     xzr, [sp, #56]              // bg count
    LDX     x9, ch_bg
    ldr     x9, [x9, x20, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    colorshift_build.dynamic_frames
    ADRG    x4, cs_bg_spectrum
    bl      cs_pair_gradient
    mov     w9, w9
    str     x9, [sp, #56]
colorshift_build.dynamic_frames:
    LDX     x3, effect_config
    ldr     w3, [x3, #COLORSHIFT.frames]
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    ldr     x9, [sp, #56]
    orr     w9, w9, w23
    cbnz    w9, colorshift_build.apply
    mov     w0, w24
    mov     w2, w3
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       colorshift_build.activate
colorshift_build.apply:
    STX     x1, cs_symbol
    ldr     x7, [sp, #56]               // bg count
    ADRG    x6, cs_bg_spectrum
    cmp     x7, #0
    csel    x6, xzr, x6, eq
    mov     x4, #0
    mov     w5, #0
    cbz     w23, colorshift_build.no_fg
    ADRG    x4, cs_fg_spectrum
    mov     w5, w23
colorshift_build.no_fg:
    mov     w0, w24
    ADRG    x1, cs_symbol
    mov     w2, #1
    bl      scene_apply_gradient
colorshift_build.activate:
    mov     w0, w20
    LDX     x9, ch_user1
    add     x9, x9, x20, lsl #3
    ldr     w1, [x9]
    bl      scene_activate
    mov     w0, w20
    bl      active_insert
    mov     w0, w20
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   x3, CS_GRADIENT
    mov     w4, #ACT_CALLBACK
    ADRG    x5, cs_loop_tracker
    mov     x6, #0
    bl      event_register
    add     x19, x19, #1
    b       colorshift_build.char
colorshift_build.built:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #80
    ret

// cs_symbol_handles(x0=symbol) -> x9 = the symbol's visual handles, one
// u32 per spectrum index (0 = not made yet): a direct-mapped memo of
// (symbol, spectrum color) -> visual_make handle. A colliding symbol takes
// the entry and its array over, cleared (visuals are interned, so remaking
// a handle gives the same one). Clobbers C.
cs_symbol_handles:
    MOV64   x9, 0x9e3779b97f4a7c15
    mul     x9, x9, x0
    lsr     x9, x9, #(64 - CS_MEMO_BITS)
    lsl     w9, w9, #4
    ADRG    x3, cs_memo
    add     x3, x3, x9
    ldr     x9, [x3, #8]
    cbz     x9, cs_symbol_handles.miss
    ldr     x2, [x3]
    cmp     x2, x0
    b.ne    cs_symbol_handles.miss
    ret
cs_symbol_handles.miss:
    str     x0, [x3]
    cbz     x9, cs_symbol_handles.fresh
    mov     x2, x9                      // a collision: clear the array
    mov     x0, x9
    LDX     x3, cs_len
    mov     w9, #0
    REP_STOSD
    mov     x9, x2
    ret
cs_symbol_handles.fresh:
    stp     x3, x0, [sp, #-32]!
    str     x30, [sp, #16]
    LDX     x0, cs_len
    lsl     x0, x0, #2
    bl      alloc
    ldr     x30, [sp, #16]
    ldp     x3, x0, [sp], #32
    str     x9, [x3, #8]
    ret

// cs_pair_gradient(x9=end color, x4=out) -> w9 = length:
// Gradient::with_steps(&[cs_pair, end], 8, false). Clobbers C.
cs_pair_gradient:
    STX     x9, cs_pair + 8
    ADRG    x0, cs_pair
    mov     w1, #2
    ADRG    x2, cs_eight
    mov     w3, #1
    b       gradient_new

// cs_rotation(w20=slot) -> x9 = k: the gradient's colors start at
// spectrum[k] (Python's spectrum[shift:] + spectrum[:shift]). 0 without
// travel. Clobbers C.
cs_rotation:
    PUSH1   x30
    LDX     x4, effect_config
    mov     x9, #0
    ldr     x3, [x4, #COLORSHIFT.no_travel]
    cbnz    x3, cs_rotation.done
    LDX     x9, ch_irow
    ldrsw   x5, [x9, x20, lsl #2]       // row
    LDX     x9, ch_icol
    ldrsw   x10, [x9, x20, lsl #2]      // column
    ldr     x9, [x4, #COLORSHIFT.travel_direction]
    cmp     w9, #1
    b.eq    cs_rotation.horizontal
    cmp     w9, #2
    b.eq    cs_rotation.radial
    cmp     w9, #3
    b.eq    cs_rotation.diagonal
    // vertical: row / canvas.top
    scvtf   d0, x5
    LDX     x9, canvas_top
    scvtf   d1, x9
    fdiv    d0, d0, d1
    b       cs_rotation.shift
cs_rotation.horizontal:
    scvtf   d0, x10
    LDX     x9, canvas_right
    scvtf   d1, x9
    fdiv    d0, d0, d1
    b       cs_rotation.shift
cs_rotation.diagonal:
    add     x9, x5, x10
    scvtf   d0, x9
    LDX     x9, canvas_right
    LDX     x3, canvas_top
    add     x9, x9, x3
    scvtf   d1, x9
    fdiv    d0, d0, d1
    b       cs_rotation.shift
cs_rotation.radial:
    LDX     x0, text_bottom
    LDX     x1, text_top
    LDX     x2, text_left
    LDX     x3, text_right
    lsl     x4, x5, #32
    mov     w10, w10
    orr     x4, x4, x10
    bl      find_normalized_distance_from_center
    cbz     w9, cs_rotation.outside
    LDX     x4, effect_config
cs_rotation.shift:
    // shift = (len as f64 * index) as i64, negated when reversed
    LDX     x9, cs_len
    scvtf   d1, x9
    fmul    d0, d0, d1
    F64_TO_I64
    LDX     x4, effect_config
    neg     x3, x9
    ldr     x2, [x4, #COLORSHIFT.reverse]
    cmp     x2, #0
    csel    x9, x3, x9, ne
    LDX     x3, cs_len
    tbnz    x9, #63, cs_rotation.negative
    cmp     x9, x3
    csel    x9, x3, x9, hi
    b       cs_rotation.wrap
cs_rotation.negative:
    adds    x9, x9, x3
    csel    x9, xzr, x9, mi
cs_rotation.wrap:
    // k == len rotates by nothing
    cmp     x9, x3
    csel    x9, xzr, x9, eq
cs_rotation.done:
    POP1    x30
    ret
cs_rotation.outside:
    // FAIL msg_not_in_rectangle, with the length (utils/geometry.s) from a
    // literal so this file also assembles on its own
    ADRG    x0, msg_not_in_rectangle
    ldr     x1, =msg_not_in_rectangle_len
    b       engine_fail
    .ltorg

// cs_gradient: Gradient::new(gradient stops, steps, false, !no_loop). A
// looping gradient of two or more stops gets the first stop appended.
cs_gradient:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x19, effect_config
    ldr     x21, [x19, #COLORSHIFT.stops]
    ldr     x22, [x19, #COLORSHIFT.stop_count]
    ldr     x9, [x19, #COLORSHIFT.no_loop]
    cbnz    x9, cs_gradient.sized
    cmp     x22, #1
    b.ls    cs_gradient.sized
    lsl     x0, x22, #3
    add     x0, x0, #8
    bl      alloc
    mov     x3, #0
cs_gradient.copy:
    ldr     x2, [x21, x3, lsl #3]
    str     x2, [x9, x3, lsl #3]
    add     x3, x3, #1
    cmp     x3, x22
    b.lo    cs_gradient.copy
    ldr     x2, [x21]
    str     x2, [x9, x3, lsl #3]
    mov     x21, x9
    add     x22, x22, #1
cs_gradient.sized:
    ldr     x0, [x19, #COLORSHIFT.steps]
    ldr     x3, [x19, #COLORSHIFT.step_count]
    mov     x1, x22
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, cs_spectrum
    mov     x0, x21
    mov     w1, w22
    ldr     x2, [x19, #COLORSHIFT.steps]
    ldr     x3, [x19, #COLORSHIFT.step_count]
    LDX     x4, cs_spectrum
    bl      gradient_new
    mov     w9, w9
    STX     x9, cs_len
    POP2    x22, x30
    POP2    x19, x21
    ret

// cs_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
cs_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #COLORSHIFT.final_steps]
    ldr     x3, [x19, #COLORSHIFT.final_step_count]
    ldr     x1, [x19, #COLORSHIFT.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, cs_final_spectrum
    ldr     x0, [x19, #COLORSHIFT.final_stops]
    ldr     x1, [x19, #COLORSHIFT.final_stop_count]
    ldr     x2, [x19, #COLORSHIFT.final_steps]
    ldr     x3, [x19, #COLORSHIFT.final_step_count]
    LDX     x4, cs_final_spectrum
    bl      gradient_new
    LDX     x0, cs_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, cs_final_map_width
    ldr     x6, [x19, #COLORSHIFT.final_direction]
    bl      gradient_map
    STX     x9, cs_final_map
    POP2    x19, x30
    ret

// cs_loop_tracker(w0=slot, x1=payload): ColorShift::dispatch_callback, on
// the "gradient" scene's completion.
cs_loop_tracker:
    mov     w0, w0
    LDX     x9, ch_user0
    ldr     x3, [x9, x0, lsl #3]
    add     x3, x3, #1
    str     x3, [x9, x0, lsl #3]
    LDX     x2, effect_config
    LDX     x9, ch_user1
    add     x9, x9, x0, lsl #3
    ldr     x4, [x2, #COLORSHIFT.cycles]
    cbz     x4, cs_loop_tracker.again
    cmp     x3, x4
    b.lt    cs_loop_tracker.again
    ldr     x2, [x2, #COLORSHIFT.skip_final]
    cbnz    x2, cs_loop_tracker.done
    ldr     w1, [x9, #4]                // final_gradient
    b       scene_activate
cs_loop_tracker.again:
    ldr     w1, [x9]                    // gradient
    b       scene_activate
cs_loop_tracker.done:
    ret

// colorshift_next_frame -> w9 = 1 while characters are active, else 0.
colorshift_next_frame:
    PUSH1   x30
    bl      active_empty
    cbnz    w9, colorshift_next_frame.finished
    bl      update
    mov     w9, #1
    POP1    x30
    ret
colorshift_next_frame.finished:
    mov     w9, #0
    POP1    x30
    ret

    .section .rodata
    .balign 8
cs_eight:       .quad 8

    TSTATE
    .balign 8
cs_spectrum:        .skip 8
cs_len:             .skip 8
cs_final_spectrum:  .skip 8
cs_final_map:       .skip 8
cs_final_map_width: .skip 8
cs_symbol:          .skip 8
cs_pair:            .skip 8 * 2
cs_fg_spectrum:     .skip 8 * 16
cs_bg_spectrum:     .skip 8 * 16
cs_memo:            .skip 8 * (2 << CS_MEMO_BITS)  // (symbol, *u32 handles)

    .text
