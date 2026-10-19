// effects/bouncyballs.s - "Characters are bouncy balls falling from the top
// of the canvas" (src/effects/bouncyballs.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Bouncyballs):
//
// Every character gets, in Rust's order: two RNG choices (ball color, ball
// symbol), a one-frame ball scene, a final scene fading from the ball color
// to its final color, a drop row from rng.uniform, and a one-waypoint path
// back to its input coordinate whose completion activates the final scene.
//
// group_by_row (a BTreeMap over input rows, in input order within a row) is
// the input order read backwards by row runs: get_characters with
// TopToBottomLeftToRight is the input order, whose rows never increase.

.equ HAVE_bouncyballs, 1

.equ BOUNCYBALLS.ball_colors,       0       // *const u64
.equ BOUNCYBALLS.ball_color_count,  8
.equ BOUNCYBALLS.ball_symbols,      16      // *const u64 (packed symbols)
.equ BOUNCYBALLS.ball_symbol_count, 24
.equ BOUNCYBALLS.ball_delay,        32
.equ BOUNCYBALLS.movement_speed,    40      // f64
.equ BOUNCYBALLS.movement_easing,   48
.equ BOUNCYBALLS.final_stops,       56      // *const u64
.equ BOUNCYBALLS.final_stop_count,  64
.equ BOUNCYBALLS.final_steps,       72      // *const i64
.equ BOUNCYBALLS.final_step_count,  80
.equ BOUNCYBALLS.final_direction,   88
.equ BOUNCYBALLS_size,              96

.equ BB_DYNAMIC,        1               // cfg_existing_colors: dynamic

// build loop locals
.equ BB_L_BALL,         0               // ball scene
.equ BB_L_FINAL,        8               // final scene
.equ BB_L_COLOR,        16              // ball color
.equ BB_L_FG,           24              // input fg (dynamic)
.equ BB_L_BG,           32              // input bg (dynamic)
.equ BB_L_FG_LEN,       40
.equ BB_L_BG_LEN,       48
.equ BB_L_LEN,          56              // final spectrum length
.equ BB_L_RET,          64              // bb_final_dynamic's return address
.equ BB_L_SIZE,         80
.equ BB_FRAME,          (BB_L_SIZE + 64)   // + x19-x24, x30

    .text

// bouncyballs_build: BouncyBalls::build.
bouncyballs_build:
    sub     sp, sp, #BB_FRAME
    stp     x19, x20, [sp, #BB_L_SIZE]
    stp     x21, x22, [sp, #(BB_L_SIZE + 16)]
    stp     x23, x24, [sp, #(BB_L_SIZE + 32)]
    str     x30, [sp, #(BB_L_SIZE + 48)]
    bl      bb_final_color_map
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    STX     x9, bb_order
    STX     x2, bb_group_end
    lsl     x0, x2, #2
    add     x0, x0, #4
    bl      alloc
    STX     x9, bb_pending
    STX     xzr, bb_pending_count
    LDX     x24, effect_config
    mov     x19, #0
bouncyballs_build.char:
    cmp     x19, x22
    b.hs    bouncyballs_build.check_order
    ldr     w23, [x21, x19, lsl #2]
    // final color: final_gradient_mapping[input_coord]
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x23, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, bb_final_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x23, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, bb_final_map
    ldr     x9, [x3, x9, lsl #3]
    ADRG    x3, bb_pair
    str     x9, [x3, #8]
    // color = choice(ball_colors), symbol = choice(ball_symbols)
    ldr     x0, [x24, #BOUNCYBALLS.ball_color_count]
    bl      rng_below
    ldr     x3, [x24, #BOUNCYBALLS.ball_colors]
    ldr     x9, [x3, x9, lsl #3]
    str     x9, [sp, #BB_L_COLOR]
    ldr     x0, [x24, #BOUNCYBALLS.ball_symbol_count]
    bl      rng_below
    ldr     x3, [x24, #BOUNCYBALLS.ball_symbols]
    ldr     x20, [x3, x9, lsl #3]
    // ball scene: the symbol in the ball color for one frame
    mov     w0, w23
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     w9, [sp, #BB_L_BALL]
    mov     w0, w9
    mov     x1, x20
    mov     w2, #1
    ldr     x3, [sp, #BB_L_COLOR]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    // final scene
    mov     w0, w23
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     w9, [sp, #BB_L_FINAL]
    LDX     x9, cfg_existing_colors
    cmp     x9, #BB_DYNAMIC
    b.eq    bouncyballs_build.dynamic
    // with_steps([color, final color], 10), the input symbol for 6 frames each
    ldr     x9, [sp, #BB_L_COLOR]
    ADRG    x0, bb_pair
    str     x9, [x0]
    mov     w1, #2
    ADRG    x2, bb_ten_steps
    mov     w3, #1
    ADRG    x4, bb_fg_spectrum
    bl      gradient_new
    str     w9, [sp, #BB_L_LEN]
    mov     x20, #0
bouncyballs_build.final_frame:
    ldr     w9, [sp, #BB_L_LEN]
    cmp     x20, x9
    b.hs    bouncyballs_build.drop
    ldr     w0, [sp, #BB_L_FINAL]
    LDX     x1, ch_sym
    ldr     x1, [x1, x23, lsl #3]
    mov     w2, #6
    ADRG    x9, bb_fg_spectrum
    ldr     x3, [x9, x20, lsl #3]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    add     w20, w20, #1
    b       bouncyballs_build.final_frame
bouncyballs_build.drop:
    // drop row: int(canvas.top * uniform(1.0, 1.5))
    fmov    d0, #1.0
    fmov    d1, #1.5
    bl      rng_uniform
    LDX     x9, canvas_top
    scvtf   d1, x9
    fmul    d0, d0, d1
    F64_TO_I64
    lsl     x1, x9, #32
    LDX     x9, ch_icol
    ldr     w9, [x9, x23, lsl #2]
    orr     x1, x1, x9
    mov     w0, w23
    bl      set_coordinate
    // the path to the input coordinate
    mov     w0, w23
    ldr     d0, [x24, #BOUNCYBALLS.movement_speed]
    ldr     w1, [x24, #BOUNCYBALLS.movement_easing]
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w20, w9
    mov     w0, w23
    bl      char_input_coord
    mov     x1, x9
    mov     w0, w20
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w23
    mov     w1, w20
    bl      path_activate
    mov     w0, w23
    ldr     w1, [sp, #BB_L_BALL]
    bl      scene_activate
    // PathComplete(path) -> ActivateScene(final)
    PATH_PTR x3, x20
    ldr     w3, [x3, #PA_NAME]
    ldr     w9, [sp, #BB_L_FINAL]
    SCENE_PTR x5, x9
    MOV64   x12, SC_NAME
    ldr     w5, [x5, x12]
    mov     x6, #0
    mov     w0, w23
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w4, #ACT_ACTIVATE_SCENE
    bl      event_register
    add     x19, x19, #1
    b       bouncyballs_build.char
bouncyballs_build.dynamic:
    bl      bb_final_dynamic
    b       bouncyballs_build.drop
bouncyballs_build.check_order:
    // input rows never increase, so row runs read backwards are the groups
    mov     x19, #1
bouncyballs_build.order:
    cmp     x19, x22
    b.hs    bouncyballs_build.built
    LDX     x3, ch_irow
    sub     x9, x19, #1
    ldr     w9, [x21, x9, lsl #2]
    ldr     w2, [x21, x19, lsl #2]
    ldr     w9, [x3, x9, lsl #2]
    ldr     w2, [x3, x2, lsl #2]
    cmp     w9, w2
    b.lt    bouncyballs_build.unordered
    add     x19, x19, #1
    b       bouncyballs_build.order
bouncyballs_build.built:
    STX     xzr, bb_delay
    ldr     x30, [sp, #(BB_L_SIZE + 48)]
    ldp     x23, x24, [sp, #(BB_L_SIZE + 32)]
    ldp     x21, x22, [sp, #(BB_L_SIZE + 16)]
    ldp     x19, x20, [sp, #BB_L_SIZE]
    add     sp, sp, #BB_FRAME
    ret
bouncyballs_build.unordered:
    ADRG    x0, msg_bb_order
    mov     w1, #msg_bb_order_len
    b       fatal

// bb_final_dynamic(w23=slot; the build's locals at [sp]): the final
// scene under --existing-color-handling dynamic - gradients from the ball
// color to the input colors, or the input symbol with no colors. Keeps its
// return address in the build's BB_L_RET slot.
bb_final_dynamic:
    str     x30, [sp, #BB_L_RET]
    LDX     x9, ch_fg
    ldr     x9, [x9, x23, lsl #3]
    str     x9, [sp, #BB_L_FG]
    LDX     x3, ch_bg
    ldr     x3, [x3, x23, lsl #3]
    str     x3, [sp, #BB_L_BG]
    and     x9, x9, x3
    cmn     x9, #1                      // NONE
    b.ne    bb_final_dynamic.gradients
    ldr     w0, [sp, #BB_L_FINAL]
    LDX     x1, ch_sym
    ldr     x1, [x1, x23, lsl #3]
    mov     w2, #6
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    ldr     x30, [sp, #BB_L_RET]
    b       scene_add_frame
bb_final_dynamic.gradients:
    str     xzr, [sp, #BB_L_FG_LEN]
    str     xzr, [sp, #BB_L_BG_LEN]
    ldr     x9, [sp, #BB_L_FG]
    cmn     x9, #1                      // NONE
    b.eq    bb_final_dynamic.bg
    ldr     x9, [sp, #BB_L_COLOR]
    ADRG    x0, bb_pair
    str     x9, [x0]
    ldr     x9, [sp, #BB_L_FG]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, bb_ten_steps
    mov     w3, #1
    ADRG    x4, bb_fg_spectrum
    bl      gradient_new
    mov     w9, w9
    str     x9, [sp, #BB_L_FG_LEN]
bb_final_dynamic.bg:
    ldr     x9, [sp, #BB_L_BG]
    cmn     x9, #1                      // NONE
    b.eq    bb_final_dynamic.apply
    ldr     x9, [sp, #BB_L_COLOR]
    ADRG    x0, bb_pair
    str     x9, [x0]
    ldr     x9, [sp, #BB_L_BG]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, bb_ten_steps
    mov     w3, #1
    ADRG    x4, bb_bg_spectrum
    bl      gradient_new
    mov     w9, w9
    str     x9, [sp, #BB_L_BG_LEN]
bb_final_dynamic.apply:
    LDX     x9, ch_sym
    ldr     x9, [x9, x23, lsl #3]
    STX     x9, bb_symbol
    ldr     w0, [sp, #BB_L_FINAL]
    ADRG    x1, bb_symbol
    mov     x2, #1
    mov     w3, #6
    mov     x4, #0
    mov     x5, #0
    ldr     x9, [sp, #BB_L_FG]
    cmn     x9, #1                      // NONE
    b.eq    bb_final_dynamic.no_fg
    ADRG    x4, bb_fg_spectrum
    ldr     x5, [sp, #BB_L_FG_LEN]
bb_final_dynamic.no_fg:
    mov     x6, #0
    mov     x7, #0
    ldr     x9, [sp, #BB_L_BG]
    cmn     x9, #1                      // NONE
    b.eq    bb_final_dynamic.no_bg
    ADRG    x6, bb_bg_spectrum
    ldr     x7, [sp, #BB_L_BG_LEN]
bb_final_dynamic.no_bg:
    bl      scene_apply_gradient
    ldr     x30, [sp, #BB_L_RET]
    ret

// bb_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
bb_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #BOUNCYBALLS.final_steps]
    ldr     x3, [x19, #BOUNCYBALLS.final_step_count]
    ldr     x1, [x19, #BOUNCYBALLS.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, bb_final_spectrum
    ldr     x0, [x19, #BOUNCYBALLS.final_stops]
    ldr     x1, [x19, #BOUNCYBALLS.final_stop_count]
    ldr     x2, [x19, #BOUNCYBALLS.final_steps]
    ldr     x3, [x19, #BOUNCYBALLS.final_step_count]
    LDX     x4, bb_final_spectrum
    bl      gradient_new
    LDX     x0, bb_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, bb_final_map_width
    ldr     x6, [x19, #BOUNCYBALLS.final_direction]
    bl      gradient_map
    STX     x9, bb_final_map
    POP2    x19, x30
    ret

// bouncyballs_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
bouncyballs_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x9, bb_pending_count
    cbnz    x9, bouncyballs_next_frame.drop
    LDX     x9, bb_group_end
    cbnz    x9, bouncyballs_next_frame.next_group
    bl      active_empty
    cbnz    w9, bouncyballs_next_frame.finished
    b       bouncyballs_next_frame.update
bouncyballs_next_frame.next_group:
    // pending = group_by_row.remove(min row): the last row run of the order
    LDX     x2, bb_order
    LDX     x3, ch_irow
    LDX     x19, bb_group_end
    sub     x9, x19, #1
    ldr     w9, [x2, x9, lsl #2]
    ldr     w4, [x3, x9, lsl #2]        // the group's row
    sub     x21, x19, #1
bouncyballs_next_frame.run:
    cbz     x21, bouncyballs_next_frame.take
    sub     x9, x21, #1
    ldr     w9, [x2, x9, lsl #2]
    ldr     w9, [x3, x9, lsl #2]
    cmp     w9, w4
    b.ne    bouncyballs_next_frame.take
    sub     x21, x21, #1
    b       bouncyballs_next_frame.run
bouncyballs_next_frame.take:
    STX     x21, bb_group_end
    add     x1, x2, x21, lsl #2
    LDX     x0, bb_pending
    sub     x3, x19, x21
    STX     x3, bb_pending_count
    REP_MOVSD
bouncyballs_next_frame.drop:
    LDX     x9, bb_delay
    cbz     x9, bouncyballs_next_frame.release
    sub     x9, x9, #1
    STX     x9, bb_delay
    b       bouncyballs_next_frame.update
bouncyballs_next_frame.release:
    mov     x0, #2
    mov     x1, #6
    bl      rng_randint
    mov     x22, x9
bouncyballs_next_frame.ball:
    cbz     x22, bouncyballs_next_frame.released
    sub     x22, x22, #1
    LDX     x1, bb_pending_count
    cbz     x1, bouncyballs_next_frame.released
    mov     x0, #0
    sub     x1, x1, #1
    bl      rng_randint
    // pending.remove(index): shift the tail down one slot (the destination
    // is below the source, so the forward copy is safe)
    LDX     x0, bb_pending
    add     x0, x0, x9, lsl #2
    ldr     w21, [x0]
    add     x1, x0, #4
    LDX     x3, bb_pending_count
    sub     x3, x3, #1
    STX     x3, bb_pending_count
    sub     x3, x3, x9
    REP_MOVSD
    mov     w0, w21
    bl      set_visible
    mov     w0, w21
    bl      active_insert
    b       bouncyballs_next_frame.ball
bouncyballs_next_frame.released:
    LDX     x9, effect_config
    ldr     x9, [x9, #BOUNCYBALLS.ball_delay]
    STX     x9, bb_delay
bouncyballs_next_frame.update:
    bl      update
    mov     w9, #1
    POP2    x22, x30
    POP2    x19, x21
    ret
bouncyballs_next_frame.finished:
    mov     w9, #0
    POP2    x22, x30
    POP2    x19, x21
    ret

    .section .rodata
    .balign 8
// dropped: bb_one bb_one_half (fmov immediates)
bb_ten_steps:       .quad 10
STRING msg_bb_order, "ttfx: asm engine: bouncyballs input rows out of order\n"

    TSTATE
    .balign 8
bb_order:           .skip 8         // u32 slots, input order
bb_group_end:       .skip 8         // groups left: bb_order[..end]
bb_pending:         .skip 8         // u32 slots
bb_pending_count:   .skip 8
bb_delay:           .skip 8
bb_final_spectrum:  .skip 8
bb_final_map:       .skip 8
bb_final_map_width: .skip 8
bb_symbol:          .skip 8
bb_pair:            .skip 8 * 2
bb_fg_spectrum:     .skip 8 * 16
bb_bg_spectrum:     .skip 8 * 16

    .text
