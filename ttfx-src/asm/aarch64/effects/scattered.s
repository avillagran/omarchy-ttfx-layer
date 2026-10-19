// effects/scattered.s - "Text is scattered across the canvas and moves into
// position" (src/effects/scattered.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Scattered):
//
// Each character gets, in Rust's order: a random start coordinate, one auto
// path to its input coordinate with SetLayer events, and one distance-synced
// auto scene fading from the final gradient's first color to its own. Nothing
// draws from the RNG after build.

.equ HAVE_scattered, 1

.equ SCATTERED.movement_speed,      0       // f64
.equ SCATTERED.movement_easing,     8
.equ SCATTERED.final_stops,         16      // *const u64
.equ SCATTERED.final_stop_count,    24
.equ SCATTERED.final_steps,         32      // *const i64
.equ SCATTERED.final_step_count,    40
.equ SCATTERED.final_frames,        48
.equ SCATTERED.final_direction,     56
.equ SCATTERED_size,                64

.equ SCAT_HOLD_FRAMES,  25

    .text

// scattered_build: Scattered::build.
scattered_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56]: the path's name
    bl      scat_final_color_map
    mov     x9, #NONE
    STX     x9, scat_last_fg
    LDX     x22, input_chars
    LDX     x23, input_count
    LDX     x24, effect_config
    mov     x19, #0
scattered_build.char:
    cmp     x19, x23
    b.hs    scattered_build.built
    ldr     w21, [x22, x19, lsl #2]
    // final colors: the input colors under dynamic, else the mapped gradient
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    scattered_build.mapped
    LDX     x9, ch_fg
    ldr     x9, [x9, x21, lsl #3]
    STX     x9, scat_fg
    LDX     x9, ch_bg
    ldr     x9, [x9, x21, lsl #3]
    STX     x9, scat_bg
    b       scattered_build.start
scattered_build.mapped:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x21, lsl #2]
    LDX     x3, text_bottom
    sub     x9, x9, x3
    LDX     x3, scat_map_width
    mul     x9, x9, x3
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x21, lsl #2]
    add     x9, x9, x3
    LDX     x3, text_left
    sub     x9, x9, x3
    LDX     x3, scat_map
    ldr     x9, [x3, x9, lsl #3]
    STX     x9, scat_fg
    mov     x9, #NONE
    STX     x9, scat_bg
scattered_build.start:
    // start coordinate: (1, 1) on a tiny canvas, else canvas.random_coord
    MOV64   x9, ((1 << 32) | 1)
    LDX     x3, canvas_right
    cmp     x3, #2
    b.lt    scattered_build.place
    LDX     x3, canvas_top
    cmp     x3, #2
    b.lt    scattered_build.place
    mov     w0, #0
    mov     w1, #0
    bl      canvas_random_coord
scattered_build.place:
    mov     w0, w21
    mov     x1, x9
    bl      set_coordinate
    // the path home
    mov     w0, w21
    ldr     d0, [x24, #SCATTERED.movement_speed]
    ldr     w1, [x24, #SCATTERED.movement_easing]
    mov     x2, #NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w20, w9                     // path index
    LDX     x1, ch_irow
    ldr     w1, [x1, x21, lsl #2]
    lsl     x1, x1, #32
    LDX     x3, ch_icol
    ldr     w3, [x3, x21, lsl #2]
    orr     x1, x1, x3
    mov     w0, w20
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    PATH_PTR x9, x20
    ldr     w9, [x9, #PA_NAME]
    str     w9, [sp, #56]               // path name
    mov     w0, w21
    mov     w1, #EV_PATH_ACTIVATED
    mov     w2, #CALLER_PATH
    ldr     w3, [sp, #56]
    mov     w4, #ACT_SET_LAYER
    mov     x5, #1
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    ldr     w3, [sp, #56]
    mov     w4, #ACT_SET_LAYER
    mov     x5, #0
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, w20
    bl      path_activate
    mov     w0, w21
    bl      set_visible
    // the gradient scene, synced to the path's distance
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #SCF_SYNC_DISTANCE
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9                     // scene index
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    scattered_build.gradient
    mov     w0, w20
    LDX     x1, ch_sym
    ldr     x1, [x1, x21, lsl #3]
    ldr     x2, [x24, #SCATTERED.final_frames]
    LDX     x3, scat_fg
    LDX     x4, scat_bg
    mov     w5, #0
    bl      scene_add_frame
    b       scattered_build.activate
scattered_build.gradient:
    // Gradient::with_steps([spectrum[0], final fg], 10); consecutive
    // characters mostly share their final color, so the last one is kept
    LDX     x9, scat_fg
    LDX     x3, scat_last_fg
    cmp     x9, x3
    b.eq    scattered_build.apply
    STX     x9, scat_last_fg
    ADRG    x0, scat_pair
    str     x9, [x0, #8]
    LDX     x9, scat_spectrum
    ldr     x9, [x9]
    str     x9, [x0]
    mov     w1, #2
    ADRG    x2, scat_ten_steps
    mov     w3, #1
    ADRG    x4, scat_char_spectrum
    bl      gradient_new
    STX     x9, scat_char_len
scattered_build.apply:
    // apply_gradient_to_symbols([symbol], final frames, the spectrum): a
    // frame per color, the visuals shared by symbol and final color
    LDX     x0, ch_sym
    ldr     x0, [x0, x21, lsl #3]
    ADRG    x1, scat_char_spectrum
    LDX     x2, scat_char_len
    mov     x3, #NONE
    LDX     x4, scat_last_fg
    bl      visual_run
    mov     w0, w20
    mov     x1, x9
    ldr     x3, [x24, #SCATTERED.final_frames]
    bl      visual_frames
scattered_build.activate:
    mov     w0, w21
    mov     w1, w20
    bl      scene_activate
    mov     w0, w21
    bl      active_insert
    add     x19, x19, #1
    b       scattered_build.char
scattered_build.built:
    mov     x9, #SCAT_HOLD_FRAMES
    STX     x9, scat_hold
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// scat_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
scat_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #SCATTERED.final_steps]
    ldr     x3, [x19, #SCATTERED.final_step_count]
    ldr     x1, [x19, #SCATTERED.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, scat_spectrum
    ldr     x0, [x19, #SCATTERED.final_stops]
    ldr     x1, [x19, #SCATTERED.final_stop_count]
    ldr     x2, [x19, #SCATTERED.final_steps]
    ldr     x3, [x19, #SCATTERED.final_step_count]
    LDX     x4, scat_spectrum
    bl      gradient_new
    LDX     x0, scat_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, scat_map_width
    ldr     x6, [x19, #SCATTERED.final_direction]
    bl      gradient_map
    STX     x9, scat_map
    POP2    x19, x30
    ret

// scattered_next_frame -> w9 = 1 for a frame, 0 when done. The first 25
// frames hold the scattered start without ticking.
scattered_next_frame:
    PUSH1   x30
    bl      active_empty
    cbnz    w9, scattered_next_frame.finished
    LDX     x9, scat_hold
    cbz     x9, scattered_next_frame.update
    sub     x9, x9, #1
    STX     x9, scat_hold
    b       scattered_next_frame.frame
scattered_next_frame.update:
    bl      update
scattered_next_frame.frame:
    mov     w9, #1
    POP1    x30
    ret
scattered_next_frame.finished:
    mov     w9, #0
    POP1    x30
    ret

    .section .rodata
    .balign 8
scat_ten_steps:     .quad 10

    TSTATE
    .balign 8
scat_spectrum:      .skip 8
scat_map:           .skip 8
scat_map_width:     .skip 8
scat_hold:          .skip 8
scat_fg:            .skip 8
scat_bg:            .skip 8
scat_last_fg:       .skip 8
scat_symbol:        .skip 8
scat_pair:          .skip 8 * 2
scat_char_spectrum: .skip 8 * 16
scat_char_len:      .skip 8

    .text
