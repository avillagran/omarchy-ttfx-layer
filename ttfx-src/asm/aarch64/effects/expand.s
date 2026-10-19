// effects/expand.s - "Expands the text from a single point"
// (src/effects/expand.rs).
//
// Every character starts at the canvas center and travels to its input
// coordinate on one eased path (layer 1 while moving, 0 on arrival) while a
// distance-synced scene fades it from the first final-gradient color to its
// own final color. No RNG draws.

.equ HAVE_expand, 1

.equ EXPAND.easing,             0
.equ EXPAND.speed,              8       // f64
.equ EXPAND.final_stops,        16      // *const u64
.equ EXPAND.final_stop_count,   24
.equ EXPAND.final_steps,        32      // *const i64
.equ EXPAND.final_step_count,   40
.equ EXPAND.final_direction,    48
.equ EXPAND_size,               56

    .text

// expand_build: Expand::build.
expand_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    bl      ex_final_color_map
    mov     x9, #NONE
    STX     x9, ex_last_color
    // both of Rust's passes use TopToBottomLeftToRight, which draws nothing;
    // the first only fills the final color map, read here directly
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x23, x9
    mov     x24, x2
    mov     x19, #0
expand_build.char:
    cmp     x19, x24
    b.hs    expand_build.built
    ldr     w21, [x23, x19, lsl #2]
    // motion.set_coordinate(canvas center)
    LDX     x1, center_row
    lsl     x1, x1, #32
    LDW     w9, center_col
    orr     x1, x1, x9
    mov     w0, w21
    bl      set_coordinate
    // new_path(movement_speed, expand_easing) with the input coordinate
    LDX     x9, effect_config
    ldr     d0, [x9, #EXPAND.speed]
    ldr     w1, [x9, #EXPAND.easing]
    mov     w0, w21
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w20, w9
    mov     w0, w21
    bl      char_input_coord
    mov     x1, x9
    mov     w0, w20
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w21
    bl      set_visible
    mov     w0, w21
    bl      active_insert
    // the path's (auto) name for the events
    PATH_PTR x22, x20
    ldr     w22, [x22, #PA_NAME]
    mov     w0, w21
    mov     w1, #EV_PATH_ACTIVATED
    mov     w2, #CALLER_PATH
    mov     w3, w22
    mov     w4, #ACT_SET_LAYER
    mov     x5, #1
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w3, w22
    mov     w4, #ACT_SET_LAYER
    mov     x5, #0
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, w20
    bl      path_activate
    // gradient scene, synced to distance
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #SCF_SYNC_DISTANCE
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    expand_build.dynamic
    // final color at the input coordinate; consecutive characters mostly
    // share it, so the two-stop gradient is rebuilt only when it changes
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x21, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, ex_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x21, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, ex_map
    ldr     x9, [x3, x9, lsl #3]
    LDX     x10, ex_last_color
    cmp     x9, x10
    b.eq    expand_build.apply
    STX     x9, ex_last_color
    ADRG    x0, ex_fg_spectrum
    mov     x1, x9
    bl      ex_pair_gradient
    STW     w9, ex_fg_len
expand_build.apply:
    // apply_gradient_to_symbols([symbol], 5, fg spectrum): a frame per
    // color, the visuals shared by symbol and final color
    LDX     x0, ch_sym
    ldr     x0, [x0, x21, lsl #3]
    ADRG    x1, ex_fg_spectrum
    LDW     w2, ex_fg_len
    mov     x3, #NONE
    LDX     x4, ex_last_color
    bl      visual_run
    mov     w0, w20
    mov     x1, x9
    mov     w3, #5
    bl      visual_frames
expand_build.activate:
    mov     w0, w21
    mov     w1, w20
    bl      scene_activate
    add     x19, x19, #1
    b       expand_build.char
expand_build.dynamic:
    bl      ex_dynamic_scene
    b       expand_build.activate
expand_build.built:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// ex_dynamic_scene(w21=slot, w20=scene): the existing-color-handling
// dynamic branch - gradients from the first final color to each input color
// present (duration 1), or one uncolored frame when there are none.
ex_dynamic_scene:
    PUSH2   x22, x23
    PUSH2   x30, xzr
    mov     x9, #NONE                   // the fg spectrum is reused below
    STX     x9, ex_last_color
    mov     x22, #0                     // fg spectrum or 0
    mov     x23, #0                     // bg spectrum or 0
    LDX     x9, ch_fg
    ldr     x1, [x9, x21, lsl #3]
    cmn     x1, #1                      // NONE
    b.eq    ex_dynamic_scene.bg
    ADRG    x0, ex_fg_spectrum
    bl      ex_pair_gradient
    STW     w9, ex_fg_len
    ADRG    x22, ex_fg_spectrum
ex_dynamic_scene.bg:
    LDX     x9, ch_bg
    ldr     x1, [x9, x21, lsl #3]
    cmn     x1, #1                      // NONE
    b.eq    ex_dynamic_scene.apply
    ADRG    x0, ex_bg_spectrum
    bl      ex_pair_gradient
    STW     w9, ex_bg_len
    ADRG    x23, ex_bg_spectrum
ex_dynamic_scene.apply:
    orr     x9, x22, x23
    cbz     x9, ex_dynamic_scene.plain
    mov     w0, w20
    LDX     x1, ch_sym
    add     x1, x1, x21, lsl #3
    mov     x2, #1
    mov     w3, #1
    mov     x4, x22
    mov     x5, #0
    cbz     x22, ex_dynamic_scene.no_fg
    LDW     w5, ex_fg_len
ex_dynamic_scene.no_fg:
    mov     x7, #0
    cbz     x23, ex_dynamic_scene.no_bg
    LDW     w7, ex_bg_len
ex_dynamic_scene.no_bg:
    mov     x6, x23
    bl      scene_apply_gradient
    b       ex_dynamic_scene.done
ex_dynamic_scene.plain:
    mov     w0, w20
    LDX     x1, ch_sym
    ldr     x1, [x1, x21, lsl #3]
    mov     w2, #1
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
ex_dynamic_scene.done:
    POP2    x30, xzr
    POP2    x22, x23
    ret

// ex_pair_gradient(x0=out spectrum, x1=end color) -> w9 = length:
// Gradient::with_steps([final spectrum[0], end], 10).
ex_pair_gradient:
    mov     x4, x0
    LDX     x9, ex_final_spectrum
    ldr     x9, [x9]
    ADRG    x0, ex_pair_stops
    stp     x9, x1, [x0]
    mov     w1, #2
    ADRG    x2, ex_ten_steps
    mov     w3, #1
    b       gradient_new

// ex_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
ex_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #EXPAND.final_steps]
    ldr     x3, [x19, #EXPAND.final_step_count]
    ldr     x1, [x19, #EXPAND.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, ex_final_spectrum
    ldr     x0, [x19, #EXPAND.final_stops]
    ldr     x1, [x19, #EXPAND.final_stop_count]
    ldr     x2, [x19, #EXPAND.final_steps]
    ldr     x3, [x19, #EXPAND.final_step_count]
    LDX     x4, ex_final_spectrum
    bl      gradient_new
    LDX     x0, ex_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, ex_map_width
    ldr     x6, [x19, #EXPAND.final_direction]
    bl      gradient_map
    STX     x9, ex_map
    POP2    x19, x30
    ret

// expand_next_frame -> w9 = 1 while characters are active, 0 when done.
expand_next_frame:
    PUSH1   x30
    bl      active_empty
    cbnz    w9, expand_next_frame.done
    bl      update
    mov     w9, #1
    POP1    x30
    ret
expand_next_frame.done:
    mov     w9, #0
    POP1    x30
    ret

    .section .rodata
    .balign 8
ex_ten_steps:   .8byte 10

    TSTATE
    .balign 8
ex_final_spectrum:  .skip 8
ex_map:             .skip 8
ex_map_width:       .skip 8
ex_last_color:      .skip 8
ex_pair_stops:      .skip 16
ex_fg_spectrum:     .skip 8*16
ex_bg_spectrum:     .skip 8*16
ex_fg_len:          .skip 4
ex_bg_len:          .skip 4

    .text
