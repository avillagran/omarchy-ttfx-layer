// effects/spray.s - "Draws the characters spawning at varying rates from a
// single point" (src/effects/spray.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Spray):
//
// Every character gets its path, events and droplet scene in Rust's order,
// so the speed, spectrum and shuffle draws line up.

.equ HAVE_spray, 1

.equ SPRAY.position,            0       // SprayPosition: n ne e se s sw w nw center
.equ SPRAY.volume,              8       // f64
.equ SPRAY.speed_min,           16      // f64
.equ SPRAY.speed_max,           24      // f64
.equ SPRAY.easing,              32
.equ SPRAY.final_stops,         40      // *const u64
.equ SPRAY.final_stop_count,    48
.equ SPRAY.final_steps,         56      // *const i64
.equ SPRAY.final_step_count,    64
.equ SPRAY.final_direction,     72
.equ SPRAY_size,                80

.equ SPRAY_N,                   0
.equ SPRAY_NE,                  1
.equ SPRAY_E,                   2
.equ SPRAY_SE,                  3
.equ SPRAY_S,                   4
.equ SPRAY_SW,                  5
.equ SPRAY_W,                   6
.equ SPRAY_NW,                  7
.equ SPRAY_CENTER,              8

    .text

// spray_build: Spray::build.
// Locals: [sp + 64] the speed, then the droplet scene (u32); [sp + 68] the
// path's name.
spray_build:
    stp     x19, x20, [sp, #-80]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    bl      spray_final_color_map
    bl      spray_origin
    STX     x9, spray_origin_coord
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x23, x9
    mov     x24, x2
    lsl     x0, x2, #2
    add     x0, x0, #4
    bl      alloc
    STX     x9, spray_pending
    LDX     x22, effect_config
    mov     x19, #0
spray_build.char:
    cmp     x19, x24
    b.hs    spray_build.shuffle
    ldr     w21, [x23, x19, lsl #2]
    // speed = uniform(range); start at the origin on a one-waypoint path
    ldr     d0, [x22, #SPRAY.speed_min]
    ldr     d1, [x22, #SPRAY.speed_max]
    bl      rng_uniform
    str     d0, [sp, #64]
    mov     w0, w21
    LDX     x1, spray_origin_coord
    bl      set_coordinate
    mov     w0, w21
    ldr     d0, [sp, #64]
    ldr     w1, [x22, #SPRAY.easing]
    MOV64   x2, NONE_I64
    mov     w3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w20, w9                     // path index
    mov     w0, w21
    bl      char_input_coord
    mov     x1, x9
    mov     w0, w20
    mov     w2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // activated -> layer 1, complete -> layer 0
    PATH_PTR x9, x20
    ldr     w9, [x9, #PA_NAME]
    str     w9, [sp, #68]
    mov     w0, w21
    mov     w1, #EV_PATH_ACTIVATED
    mov     w2, #CALLER_PATH
    ldr     w3, [sp, #68]
    mov     w4, #ACT_SET_LAYER
    mov     w5, #1
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    ldr     w3, [sp, #68]
    mov     w4, #ACT_SET_LAYER
    mov     w5, #0
    mov     x6, #0
    bl      event_register
    // the droplet scene
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     w9, [sp, #64]
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    spray_build.dynamic
    // with_steps([choice(final spectrum), final color], 7), 20 frames each
    LDX     x0, spray_spectrum_len
    bl      rng_below
    mov     x20, x9                     // the start color's index
    LDX     x3, spray_spectrum
    ldr     x9, [x3, x9, lsl #3]
    ADRG    x3, spray_pair
    str     x9, [x3]
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x21, lsl #2]
    LDX     x3, text_bottom
    sub     x9, x9, x3
    LDX     x3, spray_map_width
    mul     x9, x9, x3
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x21, lsl #2]
    add     x9, x9, x3
    LDX     x3, text_left
    sub     x9, x9, x3
    LDX     x3, spray_map
    ldr     x9, [x3, x9, lsl #3]
    ADRG    x3, spray_pair
    str     x9, [x3, #8]
    // apply_gradient_to_symbols([input symbol], 20, the pair's spectrum):
    // a frame per color, the visuals shared by symbol and pair (colors fit
    // in 41 bits, so the index above them keys the pair exactly)
    lsl     x20, x20, #48
    orr     x20, x20, x9
    LDX     x0, ch_sym
    ldr     x0, [x0, x21, lsl #3]
    mov     x1, x20
    mov     x2, #NONE
    bl      visual_run_find
    cbnz    x9, spray_build.frames
    ADRG    x0, spray_pair
    mov     w1, #2
    ADRG    x2, spray_seven
    mov     w3, #1
    ADRG    x4, spray_pair_spectrum
    bl      gradient_new
    LDX     x0, ch_sym
    ldr     x0, [x0, x21, lsl #3]
    ADRG    x1, spray_pair_spectrum
    mov     w2, w9
    mov     x3, #NONE
    mov     x4, x20
    bl      visual_run
spray_build.frames:
    ldr     w0, [sp, #64]
    mov     x1, x9
    mov     w3, #20
    bl      visual_frames
    b       spray_build.activate
spray_build.dynamic:
    // the input colors on the input symbol, 7 frames of 20
    mov     w20, #0
spray_build.dynamic_frame:
    ldr     w0, [sp, #64]
    LDX     x1, ch_sym
    ldr     x1, [x1, x21, lsl #3]
    mov     w2, #20
    LDX     x3, ch_fg
    ldr     x3, [x3, x21, lsl #3]
    LDX     x4, ch_bg
    ldr     x4, [x4, x21, lsl #3]
    mov     w5, #0
    bl      scene_add_frame
    add     w20, w20, #1
    cmp     w20, #7
    b.lo    spray_build.dynamic_frame
spray_build.activate:
    mov     w0, w21
    ldr     w1, [sp, #64]
    bl      scene_activate
    mov     w0, w21
    ldr     w1, [sp, #68]               // the path's name
    bl      path_activate_name
    LDX     x9, spray_pending
    str     w21, [x9, x19, lsl #2]
    add     x19, x19, #1
    b       spray_build.char
spray_build.shuffle:
    STX     x24, spray_pending_count
    LDX     x0, spray_pending
    mov     x1, x24
    bl      rng_shuffle32
    // volume = max(int(len * spray_volume), 1)
    scvtf   d0, x24
    ldr     d1, [x22, #SPRAY.volume]
    fmul    d0, d0, d1
    fcvtzs  x9, d0
    mov     x3, #1
    cmp     x9, x3
    csel    x9, x3, x9, lt
    STX     x9, spray_volume
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #80
    ret

// spray_origin -> x9 = the spray origin for the configured position (packed).
spray_origin:
    LDX     x9, effect_config
    ldr     x9, [x9, #SPRAY.position]
    LDX     x3, canvas_right
    LDX     x2, canvas_top
    cmp     w9, #SPRAY_CENTER
    b.eq    spray_origin.center
    // column: n/s at right // 2, the west side at left, the east at right - 1
    asr     x4, x3, #1
    cmp     w9, #SPRAY_N
    b.eq    spray_origin.row
    cmp     w9, #SPRAY_S
    b.eq    spray_origin.row
    mov     w4, #1
    cmp     w9, #SPRAY_SW
    b.hs    spray_origin.row            // sw w nw
    sub     x4, x3, #1                  // ne e se
spray_origin.row:
    // row: the north side at top, w/e at top // 2, the south side at bottom
    mov     x5, x2
    cmp     w9, #SPRAY_N
    b.eq    spray_origin.pack
    cmp     w9, #SPRAY_NE
    b.eq    spray_origin.pack
    cmp     w9, #SPRAY_NW
    b.eq    spray_origin.pack
    asr     x5, x5, #1
    cmp     w9, #SPRAY_E
    b.eq    spray_origin.pack
    cmp     w9, #SPRAY_W
    b.eq    spray_origin.pack
    mov     w5, #1
spray_origin.pack:
    lsl     x9, x5, #32
    mov     w4, w4
    orr     x9, x9, x4
    ret
spray_origin.center:
    LDX     x9, center_row
    lsl     x9, x9, #32
    LDW     w3, center_col
    orr     x9, x9, x3
    ret

// spray_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
spray_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #SPRAY.final_steps]
    ldr     x3, [x19, #SPRAY.final_step_count]
    ldr     x1, [x19, #SPRAY.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, spray_spectrum
    ldr     x0, [x19, #SPRAY.final_stops]
    ldr     x1, [x19, #SPRAY.final_stop_count]
    ldr     x2, [x19, #SPRAY.final_steps]
    ldr     x3, [x19, #SPRAY.final_step_count]
    LDX     x4, spray_spectrum
    bl      gradient_new
    mov     w9, w9
    STX     x9, spray_spectrum_len
    LDX     x0, spray_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, spray_map_width
    ldr     x6, [x19, #SPRAY.final_direction]
    bl      gradient_map
    STX     x9, spray_map
    POP2    x19, x30
    ret

// spray_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
spray_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x9, spray_pending_count
    cbnz    x9, spray_next_frame.release
    bl      active_empty
    cbnz    w9, spray_next_frame.finished
    b       spray_next_frame.update
spray_next_frame.release:
    mov     x0, #1
    LDX     x1, spray_volume
    bl      rng_randint
    mov     x19, x9
spray_next_frame.pop:
    cmp     x19, #0
    b.le    spray_next_frame.update
    sub     x19, x19, #1
    LDX     x9, spray_pending_count
    cbz     x9, spray_next_frame.pop
    sub     x9, x9, #1
    STX     x9, spray_pending_count
    LDX     x3, spray_pending
    ldr     w21, [x3, x9, lsl #2]
    mov     w0, w21
    bl      set_visible
    mov     w0, w21
    bl      active_insert
    b       spray_next_frame.pop
spray_next_frame.update:
    bl      update
    mov     w9, #1
    POP2    x22, x30
    POP2    x19, x21
    ret
spray_next_frame.finished:
    mov     w9, #0
    POP2    x22, x30
    POP2    x19, x21
    ret

    .section .rodata
    .balign 8
spray_seven:        .quad 7

    TSTATE
    .balign 8
spray_spectrum:         .skip 8
spray_spectrum_len:     .skip 8
spray_map:              .skip 8
spray_map_width:        .skip 8
spray_origin_coord:     .skip 8
spray_pending:          .skip 8
spray_pending_count:    .skip 8
spray_volume:           .skip 8
spray_pair:             .skip 16
spray_pair_spectrum:    .skip 128
