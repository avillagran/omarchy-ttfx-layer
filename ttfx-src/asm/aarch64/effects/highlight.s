// effects/highlight.s - "Run a specular highlight across the text"
// (src/effects/highlight.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Highlight). The effect draws no
// random numbers: its characters come in row/column order and its groups in
// grouping order.

.equ HAVE_highlight, 1

.equ HIGHLIGHT.brightness,          0       // f64
.equ HIGHLIGHT.direction,           8       // GROUP_*
.equ HIGHLIGHT.width,               16
.equ HIGHLIGHT.final_stops,         24      // *const u64
.equ HIGHLIGHT.final_stop_count,    32
.equ HIGHLIGHT.final_steps,         40      // *const i64
.equ HIGHLIGHT.final_step_count,    48
.equ HIGHLIGHT.final_direction,     56
.equ HIGHLIGHT_size,                64

.equ HL_SCENE,                      NAME_LITERAL + 0    // "highlight"
.equ HL_IN_OUT_CIRC,                21

// ch_user0 holds each character's highlight scene index.

    .text

// highlight_build: Highlight::build.
// Locals: [sp + 56] the character's scene.
highlight_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    // groups in highlight direction, eased as a sequence of group indices
    mov     w0, #FILTER_INPUT
    LDX     x9, effect_config
    ldr     w1, [x9, #HIGHLIGHT.direction]
    bl      get_characters_grouped
    STX     x9, hl_groups
    mov     x19, x2
    lsl     x0, x2, #3
    add     x0, x0, #8
    bl      alloc
    mov     x3, #0
highlight_build.index:
    cmp     x3, x19
    b.hs    highlight_build.easer
    str     x3, [x9, x3, lsl #3]
    add     x3, x3, #1
    b       highlight_build.index
highlight_build.easer:
    ADRG    x0, hl_easer
    mov     x1, x9
    mov     x2, x19
    mov     w3, #HL_IN_OUT_CIRC
    mov     w4, #100
    bl      sequence_easer_new
    // the final gradient mapped over the text rectangle
    bl      hl_final_color_map
    // the highlight gradient: [base, bright, bright, base] in [3, width, 3]
    LDX     x9, effect_config
    ldr     x9, [x9, #HIGHLIGHT.width]
    ADRG    x0, hl_steps
    mov     x10, #3
    str     x10, [x0]
    str     x9, [x0, #8]
    str     x10, [x0, #16]
    mov     w3, #3
    mov     w1, #4
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, hl_spectrum
    mov     x9, #NONE
    STX     x9, hl_last_base
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    mov     x19, #0
highlight_build.char:
    cmp     x19, x22
    b.hs    highlight_build.built
    ldr     w20, [x21, x19, lsl #2]
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    highlight_build.gradient
    // dynamic: the input colors
    LDX     x9, ch_fg
    ldr     x23, [x9, x20, lsl #3]
    LDX     x9, ch_bg
    ldr     x24, [x9, x20, lsl #3]
    b       highlight_build.colors
highlight_build.gradient:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x20, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, hl_final_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x20, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, hl_final_map
    ldr     x23, [x3, x9, lsl #3]       // base color
    mov     x24, #NONE                  // input bg color
highlight_build.colors:
    cmn     x23, #1                     // NONE
    b.eq    highlight_build.appearance
    LDX     x9, hl_last_base
    cmp     x23, x9
    b.eq    highlight_build.appearance
    bl      hl_highlight_gradient
highlight_build.appearance:
    mov     w0, w20
    mov     x1, #0
    mov     x2, x23
    mov     x3, x24
    bl      set_appearance
    mov     w0, w20
    MOV64   w1, HL_SCENE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    LDX     x3, ch_user0
    lsl     x10, x20, #3
    str     w9, [x3, x10]
    str     w9, [sp, #56]
    cmn     x23, #1                     // NONE
    b.ne    highlight_build.frames
    // no base color: a single frame in the base colors
    mov     w0, w9
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    mov     w2, #2
    mov     x3, x23
    mov     x4, x24
    mov     w5, #0
    bl      scene_add_frame
    b       highlight_build.visible
highlight_build.frames:
    // one frame per spectrum color, the visuals shared by symbol, base
    // color (the spectrum's) and bg
    LDX     x0, ch_sym
    ldr     x0, [x0, x20, lsl #3]
    LDX     x1, hl_spectrum
    LDX     x2, hl_spectrum_len
    mov     x3, x24
    mov     x4, x23
    bl      visual_run
    ldr     w0, [sp, #56]
    mov     x1, x9
    mov     w3, #2
    bl      visual_frames
highlight_build.visible:
    mov     w0, w20
    mov     w1, #1
    bl      set_visibility
    add     x19, x19, #1
    b       highlight_build.char
highlight_build.built:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// hl_highlight_gradient(x23=base color): Gradient::new([base, highlight,
// highlight, base], [3, width, 3]) into hl_spectrum, highlight being
// adjust_color_brightness(base, brightness). Remembers base in hl_last_base.
hl_highlight_gradient:
    PUSH1   x30
    STX     x23, hl_last_base
    mov     x0, x23
    LDX     x9, effect_config
    ldr     d0, [x9, #HIGHLIGHT.brightness]
    bl      adjust_color_brightness
    ADRG    x0, hl_stops
    str     x23, [x0]
    str     x9, [x0, #8]
    str     x9, [x0, #16]
    str     x23, [x0, #24]
    mov     w1, #4
    ADRG    x2, hl_steps
    mov     w3, #3
    LDX     x4, hl_spectrum
    bl      gradient_new
    mov     w9, w9
    STX     x9, hl_spectrum_len
    POP1    x30
    ret

// hl_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
hl_final_color_map:
    PUSH2   x19, x21
    PUSH1   x30
    LDX     x19, effect_config
    ldr     x0, [x19, #HIGHLIGHT.final_steps]
    ldr     x3, [x19, #HIGHLIGHT.final_step_count]
    ldr     x1, [x19, #HIGHLIGHT.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    mov     x21, x9
    ldr     x0, [x19, #HIGHLIGHT.final_stops]
    ldr     x1, [x19, #HIGHLIGHT.final_stop_count]
    ldr     x2, [x19, #HIGHLIGHT.final_steps]
    ldr     x3, [x19, #HIGHLIGHT.final_step_count]
    mov     x4, x21
    bl      gradient_new
    mov     x0, x21
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, hl_final_map_width
    ldr     x6, [x19, #HIGHLIGHT.final_direction]
    bl      gradient_map
    STX     x9, hl_final_map
    POP1    x30
    POP2    x19, x21
    ret

// highlight_next_frame -> w9 = 1 for a frame, 0 when done.
highlight_next_frame:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    bl      active_empty
    cbz     w9, highlight_next_frame.step
    ADRG    x0, hl_easer
    bl      sequence_easer_is_complete
    cbnz    w9, highlight_next_frame.finished
highlight_next_frame.step:
    ADRG    x0, hl_easer
    bl      sequence_easer_step
    ldr     x21, [x9, #SequenceStep.added]
    ldr     x22, [x9, #SequenceStep.added_count]
highlight_next_frame.group:
    cbz     x22, highlight_next_frame.update
    ldr     x9, [x21]                   // group index
    lsl     x9, x9, #4
    LDX     x10, hl_groups
    add     x9, x9, x10
    ldr     x23, [x9]                   // slots
    ldr     x24, [x9, #8]               // count
    mov     x19, #0
highlight_next_frame.member:
    cmp     x19, x24
    b.hs    highlight_next_frame.next_group
    ldr     w20, [x23, x19, lsl #2]
    mov     w0, w20
    LDX     x9, ch_user0
    lsl     x10, x20, #3
    ldr     w1, [x9, x10]
    bl      scene_activate
    mov     w0, w20
    bl      active_insert
    add     x19, x19, #1
    b       highlight_next_frame.member
highlight_next_frame.next_group:
    add     x21, x21, #8
    sub     x22, x22, #1
    b       highlight_next_frame.group
highlight_next_frame.update:
    bl      update
    mov     w9, #1
    b       highlight_next_frame.done
highlight_next_frame.finished:
    mov     w9, #0
highlight_next_frame.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

    TSTATE
    .balign 8
hl_easer:           .skip SequenceEaser_size
hl_groups:          .skip 8
hl_final_map:       .skip 8
hl_final_map_width: .skip 8
hl_spectrum:        .skip 8
hl_spectrum_len:    .skip 8
hl_last_base:       .skip 8
hl_steps:           .skip 8*3
hl_stops:           .skip 8*4
