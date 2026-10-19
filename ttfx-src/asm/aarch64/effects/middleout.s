// effects/middleout.s - "Text expands in a single row or column in the
// middle of the canvas then out" (src/effects/middleout.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Middleout).
//
// The effect draws no random numbers. Every character starts at the canvas
// center in the starting color, rides an auto-named path to the center line,
// and once all have settled, the "full" path home and the "full" scene fading
// to its final color, in ascending slot order (the canonical set order).

.equ HAVE_middleout, 1

.equ MIDDLEOUT.starting_color,      0
.equ MIDDLEOUT.expand_direction,    8       // 0 vertical, 1 horizontal
.equ MIDDLEOUT.center_speed,        16      // f64
.equ MIDDLEOUT.full_speed,          24      // f64
.equ MIDDLEOUT.center_easing,       32
.equ MIDDLEOUT.full_easing,         40
.equ MIDDLEOUT.final_stops,         48      // *const u64
.equ MIDDLEOUT.final_stop_count,    56
.equ MIDDLEOUT.final_steps,         64      // *const i64
.equ MIDDLEOUT.final_step_count,    72
.equ MIDDLEOUT.final_direction,     80
.equ MIDDLEOUT_size,                88

// names: path "full", waypoint "full", scene "full"
.equ MO_FULL,                       NAME_LITERAL + 0

    .text

// middleout_build: MiddleoutIterator.__init__ + build().
// Frame: [sp] input coord, [sp + 8] scene, [sp + 16] [input symbol].
middleout_build:
    sub     sp, sp, #96
    stp     x19, x20, [sp, #32]
    stp     x21, x22, [sp, #48]
    stp     x23, x24, [sp, #64]
    str     x30, [sp, #80]
    bl      mo_final_color_map
    mov     x9, #NONE
    STX     x9, mo_last_final
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x23, x9
    mov     x24, x2
    LDX     x22, effect_config
    mov     x19, #0
middleout_build.char:
    cmp     x19, x24
    b.hs    middleout_build.done
    ldr     w21, [x23, x19, lsl #2]
    mov     w0, w21
    bl      char_input_coord
    str     x9, [sp]                    // input coord
    // motion.set_coordinate(canvas.center)
    LDX     x1, center_row
    LDW     w9, center_col
    orr     x1, x9, x1, lsl #32
    mov     w0, w21
    bl      set_coordinate
    // the center path (auto id) to the center line
    ldr     d0, [x22, #MIDDLEOUT.center_speed]
    mov     w0, w21
    ldr     w1, [x22, #MIDDLEOUT.center_easing]
    MOV64   x2, NONE_I64
    mov     w3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w20, w9                     // center path
    ldr     x1, [sp]
    ldr     x9, [x22, #MIDDLEOUT.expand_direction]
    cbnz    x9, middleout_build.horizontal
    // vertical: (input column, center_row)
    mov     w9, w1
    LDX     x1, center_row
    orr     x1, x9, x1, lsl #32
    b       middleout_build.waypoint
middleout_build.horizontal:
    // horizontal: (center_column, input row)
    and     x1, x1, #0xffffffff00000000
    LDW     w9, center_col
    orr     x1, x1, x9
middleout_build.waypoint:
    mov     w0, w20
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // the "full" path home
    ldr     d0, [x22, #MIDDLEOUT.full_speed]
    mov     w0, w21
    ldr     w1, [x22, #MIDDLEOUT.full_easing]
    MOV64   x2, NONE_I64
    mov     w3, #0
    mov     w4, #0
    MOV64   w5, MO_FULL
    bl      path_new
    mov     w0, w9
    ldr     x1, [sp]
    mov     x2, #0
    mov     w3, #0
    MOV64   w4, MO_FULL
    bl      path_new_waypoint
    // the "full" scene
    mov     w0, w21
    MOV64   w1, MO_FULL
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     x9, [sp, #8]                // scene
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    middleout_build.final
    bl      mo_scene_dynamic
    b       middleout_build.appearance
middleout_build.final:
    // starting color -> final gradient color in 10 steps, 6 ticks each
    ldr     x9, [sp]
    asr     x2, x9, #32
    LDX     x10, text_bottom
    sub     x2, x2, x10
    LDX     x10, mo_final_map_width
    mul     x2, x2, x10
    sxtw    x9, w9
    add     x9, x9, x2
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, mo_final_map
    ldr     x9, [x3, x9, lsl #3]
    LDX     x10, mo_last_final
    cmp     x9, x10
    b.eq    middleout_build.apply
    STX     x9, mo_last_final
    ADRG    x0, mo_pair
    str     x9, [x0, #8]
    ldr     x9, [x22, #MIDDLEOUT.starting_color]
    str     x9, [x0]
    mov     w1, #2
    ADRG    x2, mo_ten_steps
    mov     w3, #1
    ADRG    x4, mo_fg_spectrum
    bl      gradient_new
    STX     x9, mo_fg_len
middleout_build.apply:
    // apply_gradient_to_symbols([input symbol], 6, fg spectrum): a frame per
    // color, the visuals shared by symbol and final color
    LDX     x0, ch_sym
    ldr     x0, [x0, x21, lsl #3]
    ADRG    x1, mo_fg_spectrum
    LDX     x2, mo_fg_len
    mov     x3, #NONE
    LDX     x4, mo_last_final
    bl      visual_run
    ldr     w0, [sp, #8]
    mov     x1, x9
    mov     w3, #6
    bl      visual_frames
middleout_build.appearance:
    mov     w0, w21
    mov     w1, w20
    bl      path_activate
    mov     w0, w21
    mov     x1, #0
    ldr     x2, [x22, #MIDDLEOUT.starting_color]
    mov     x3, #NONE
    bl      set_appearance
    mov     w0, w21
    bl      set_visible
    mov     w0, w21
    bl      active_insert
    add     x19, x19, #1
    b       middleout_build.char
middleout_build.done:
    STB     wzr, mo_phase
    ldr     x30, [sp, #80]
    ldp     x23, x24, [sp, #64]
    ldp     x21, x22, [sp, #48]
    ldp     x19, x20, [sp, #32]
    add     sp, sp, #96
    ret

// mo_scene_dynamic(w21=slot, x22=config; the build frame at [sp] of the
// caller): the "full" scene under --existing-color-handling dynamic -
// gradients from the starting color to the input colors, or one colorless
// frame.
mo_scene_dynamic:
    stp     x19, x20, [sp, #-32]!
    str     x30, [sp, #16]
    // the build frame is now at [sp + 32]
    LDX     x9, ch_fg
    ldr     x19, [x9, x21, lsl #3]      // input fg
    LDX     x9, ch_bg
    ldr     x20, [x9, x21, lsl #3]      // input bg
    LDX     x9, ch_sym
    ldr     x9, [x9, x21, lsl #3]
    str     x9, [sp, #32 + 16]          // [input symbol]
    cmn     x19, #1                     // NONE
    b.ne    mo_scene_dynamic.gradients
    cmn     x20, #1                     // NONE
    b.ne    mo_scene_dynamic.gradients
    ldr     w0, [sp, #32 + 8]
    mov     x1, x9
    mov     w2, #6
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    ldr     x30, [sp, #16]
    ldp     x19, x20, [sp], #32
    ret
mo_scene_dynamic.gradients:
    ldr     x9, [x22, #MIDDLEOUT.starting_color]
    STX     x9, mo_pair
    cmn     x19, #1                     // NONE
    b.eq    mo_scene_dynamic.bg
    ADRG    x0, mo_pair
    str     x19, [x0, #8]
    mov     w1, #2
    ADRG    x2, mo_ten_steps
    mov     w3, #1
    ADRG    x4, mo_fg_spectrum
    bl      gradient_new
    STX     x9, mo_fg_len
mo_scene_dynamic.bg:
    cmn     x20, #1                     // NONE
    b.eq    mo_scene_dynamic.apply
    ADRG    x0, mo_pair
    str     x20, [x0, #8]
    mov     w1, #2
    ADRG    x2, mo_ten_steps
    mov     w3, #1
    ADRG    x4, mo_bg_spectrum
    bl      gradient_new
    STX     x9, mo_bg_len
mo_scene_dynamic.apply:
    // the fg spectrum no longer matches mo_last_final
    mov     x9, #NONE
    STX     x9, mo_last_final
    ldr     w0, [sp, #32 + 8]
    add     x1, sp, #32 + 16
    mov     w2, #1
    mov     w3, #6
    mov     x4, #0
    mov     w5, #0
    cmn     x19, #1                     // NONE
    b.eq    mo_scene_dynamic.no_fg
    ADRG    x4, mo_fg_spectrum
    LDW     w5, mo_fg_len
mo_scene_dynamic.no_fg:
    mov     x6, #0
    mov     w7, #0
    cmn     x20, #1                     // NONE
    b.eq    mo_scene_dynamic.no_bg
    ADRG    x6, mo_bg_spectrum
    LDW     w7, mo_bg_len
mo_scene_dynamic.no_bg:
    bl      scene_apply_gradient
    ldr     x30, [sp, #16]
    ldp     x19, x20, [sp], #32
    ret

// mo_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
mo_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #MIDDLEOUT.final_steps]
    ldr     x3, [x19, #MIDDLEOUT.final_step_count]
    ldr     x1, [x19, #MIDDLEOUT.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, mo_final_spectrum
    ldr     x0, [x19, #MIDDLEOUT.final_stops]
    ldr     x1, [x19, #MIDDLEOUT.final_stop_count]
    ldr     x2, [x19, #MIDDLEOUT.final_steps]
    ldr     x3, [x19, #MIDDLEOUT.final_step_count]
    LDX     x4, mo_final_spectrum
    bl      gradient_new
    LDX     x0, mo_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, mo_final_map_width
    ldr     x6, [x19, #MIDDLEOUT.final_direction]
    bl      gradient_map
    STX     x9, mo_final_map
    POP2    x19, x30
    ret

// middleout_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
middleout_next_frame:
    stp     x19, x20, [sp, #-32]!
    str     x30, [sp, #16]
    LDB     w9, mo_phase
    cbnz    w9, middleout_next_frame.tick
    bl      active_empty
    cbz     w9, middleout_next_frame.tick
    // the center line has settled: every character goes home, ascending
    mov     w9, #1
    STB     w9, mo_phase
    mov     x19, #0
middleout_next_frame.full:
    LDX     x10, input_count
    cmp     x19, x10
    b.hs    middleout_next_frame.tick
    LDX     x9, input_chars
    ldr     w20, [x9, x19, lsl #2]
    mov     w0, w20
    bl      active_insert
    mov     w0, w20
    MOV64   w1, MO_FULL
    bl      path_activate_name
    mov     w0, w20
    MOV64   w1, MO_FULL
    bl      scene_activate_name
    add     x19, x19, #1
    b       middleout_next_frame.full
middleout_next_frame.tick:
    bl      active_empty
    cbnz    w9, middleout_next_frame.finished
    bl      update
    mov     w9, #1
    b       middleout_next_frame.out
middleout_next_frame.finished:
    mov     w9, #0
middleout_next_frame.out:
    ldr     x30, [sp, #16]
    ldp     x19, x20, [sp], #32
    ret

    .section .rodata
    .balign 8
mo_ten_steps:       .quad 10

    TSTATE
    .balign 8
mo_final_spectrum:  .skip 8
mo_final_map:       .skip 8
mo_final_map_width: .skip 8
mo_last_final:      .skip 8
mo_pair:            .skip 8 * 2
mo_fg_spectrum:     .skip 8 * 16
mo_bg_spectrum:     .skip 8 * 16
mo_fg_len:          .skip 8
mo_bg_len:          .skip 8
mo_phase:           .skip 1

    .text
