// effects/pour.s - "Pours the characters back and forth from the top,
// bottom, left, or right" (src/effects/pour.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Pour).
//
// Characters are built group by group in Rust's order, so the speed draws
// and the auto-numbered path and scene names line up. pending_groups is the
// group array walked by index: odd groups pour from their far end.

.equ HAVE_pour, 1

.equ POUR.direction,        0           // PourDirection: up, down, left, right
.equ POUR.speed,            8           // characters per pour
.equ POUR.move_min,         16          // f64
.equ POUR.move_max,         24          // f64
.equ POUR.gap,              32
.equ POUR.starting_color,   40
.equ POUR.final_stops,      48          // *const u64
.equ POUR.final_stop_count, 56
.equ POUR.final_steps,      64          // *const i64
.equ POUR.final_step_count, 72
.equ POUR.final_frames,     80          // fits i32 (marshal checks)
.equ POUR.final_direction,  88
.equ POUR.easing,           96          // easing id
.equ POUR_size,             104

.equ POUR_UP,               0
.equ POUR_DOWN,             1
.equ POUR_LEFT,             2
.equ POUR_RIGHT,            3

    .text

// pour_build: Pour::build.
pour_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    bl      pour_final_map
    // the per-character pour gradient: starting color -> the final color
    ldr     x0, [x19, #POUR.final_steps]
    ldr     x3, [x19, #POUR.final_step_count]
    mov     w1, #2
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, pour_pair_spectrum
    ldr     x9, [x19, #POUR.starting_color]
    STX     x9, pour_pair_stops
    mov     x9, #NONE
    STX     x9, pour_last_fg
    // groups: down pours rows bottom to top, up top to bottom, left
    // columns left to right, right right to left
    ldr     x9, [x19, #POUR.direction]
    ADRG    x3, pour_groupings
    ldrb    w1, [x3, x9]
    mov     w0, #FILTER_INPUT
    bl      get_characters_grouped
    STX     x9, pour_groups
    STX     x2, pour_group_count
    mov     x21, #0                     // group index
pour_build.group:
    LDX     x9, pour_group_count
    cmp     x21, x9
    b.hs    pour_build.grouped
    lsl     x22, x21, #4
    LDX     x9, pour_groups
    add     x22, x22, x9
    ldr     x23, [x22, #8]              // count
    ldr     x22, [x22]                  // slots
pour_build.char:
    cbz     x23, pour_build.next_group
    ldr     w0, [x22]
    bl      pour_character
    add     x22, x22, #4
    sub     x23, x23, #1
    b       pour_build.char
pour_build.next_group:
    add     x21, x21, #1
    b       pour_build.group
pour_build.grouped:
    // current_group = pending_groups.remove(0)
    STX     xzr, pour_next_group
    STX     xzr, pour_left
    bl      pour_take_group
    STX     xzr, pour_gap_left
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// pour_character(w0=slot), x19 = config: hide it, move it to its pour
// start, give it a path to its input coordinate and its color scene.
pour_character:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w20, w0
    mov     w1, #0
    bl      set_visibility
    mov     w0, w20
    bl      char_input_coord
    mov     x21, x9                     // input coordinate
    ldr     x3, [x19, #POUR.direction]
    cmp     w3, #POUR_DOWN
    b.eq    pour_character.down
    cmp     w3, #POUR_UP
    b.eq    pour_character.up
    // left / right keep the row and start at the right / left edge
    lsr     x1, x9, #32
    lsl     x1, x1, #32
    mov     w9, #1                      // canvas.left
    cmp     w3, #POUR_RIGHT
    b.eq    pour_character.row_start
    LDW     w9, canvas_right
pour_character.row_start:
    orr     x1, x1, x9
    b       pour_character.start
pour_character.down:
    LDX     x1, canvas_top
    b       pour_character.column_start
pour_character.up:
    mov     w1, #1                      // canvas.bottom
pour_character.column_start:
    lsl     x1, x1, #32
    mov     w9, w21
    orr     x1, x1, x9
pour_character.start:
    mov     w0, w20
    bl      set_coordinate
    ldr     d0, [x19, #POUR.move_min]
    ldr     d1, [x19, #POUR.move_max]
    bl      rng_uniform
    mov     w0, w20
    ldr     w1, [x19, #POUR.easing]
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w22, w9
    mov     w0, w9
    mov     x1, x21
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w20
    mov     w1, w22
    bl      path_activate
    mov     w0, w20
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w22, w9                     // scene
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    pour_character.dynamic
    // final fg from the coordinate map
    asr     x9, x21, #32
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, pour_map_width
    mul     x9, x9, x10
    sxtw    x3, w21
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, pour_map
    ldr     x9, [x3, x9, lsl #3]
    LDX     x10, pour_last_fg
    cmp     x9, x10
    b.eq    pour_character.gradient
    STX     x9, pour_last_fg
    ADRG    x0, pour_pair_stops
    str     x9, [x0, #8]
    mov     w1, #2
    ldr     x2, [x19, #POUR.final_steps]
    ldr     x3, [x19, #POUR.final_step_count]
    LDX     x4, pour_pair_spectrum
    bl      gradient_new
    STW     w9, pour_pair_len
pour_character.gradient:
    // apply_gradient_to_symbols with one symbol and an fg gradient: one
    // frame per spectrum color
    mov     w23, #0
    LDX     x9, ch_sym
    ldr     x24, [x9, x20, lsl #3]
pour_character.color:
    LDW     w9, pour_pair_len
    cmp     w23, w9
    b.hs    pour_character.activate
    LDX     x3, pour_pair_spectrum
    ldr     x3, [x3, x23, lsl #3]
    mov     x4, #NONE
    mov     x1, x24
    mov     w0, w22
    ldr     w2, [x19, #POUR.final_frames]
    mov     w5, #0
    bl      scene_add_frame
    add     w23, w23, #1
    b       pour_character.color
pour_character.dynamic:
    // with_steps([starting color, input fg/bg], 10) for each present color
    LDX     x9, ch_sym
    ldr     x9, [x9, x20, lsl #3]
    STX     x9, pour_symbol
    LDX     x9, ch_fg
    ldr     x0, [x9, x20, lsl #3]
    ADRG    x1, pour_fg_spectrum
    bl      pour_dynamic_gradient
    mov     x23, x9                     // fg spectrum or 0
    mov     x24, x2                     // fg count
    LDX     x9, ch_bg
    ldr     x0, [x9, x20, lsl #3]
    ADRG    x1, pour_bg_spectrum
    bl      pour_dynamic_gradient
    orr     x3, x23, x9
    cbz     x3, pour_character.plain
    mov     x7, x2
    mov     x6, x9
    mov     w0, w22
    ADRG    x1, pour_symbol
    mov     w2, #1
    ldr     w3, [x19, #POUR.final_frames]
    mov     x4, x23
    mov     x5, x24
    bl      scene_apply_gradient
    b       pour_character.activate
pour_character.plain:
    mov     w0, w22
    LDX     x1, pour_symbol
    ldr     w2, [x19, #POUR.final_frames]
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
pour_character.activate:
    mov     w0, w20
    mov     w1, w22
    bl      scene_activate
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// pour_dynamic_gradient(x0=color or NONE, x1=spectrum out) -> x9 =
// spectrum or 0, x2 = its length: Gradient::with_steps([starting color,
// color], 10) when the color is present. x19 = config.
pour_dynamic_gradient:
    mov     x9, #0
    mov     x2, #0
    cmn     x0, #1                      // NONE
    b.eq    pour_dynamic_gradient.absent
    PUSH2   x1, x30
    ldr     x9, [x19, #POUR.starting_color]
    ADRG    x3, pour_pair_stops
    str     x9, [x3]
    str     x0, [x3, #8]
    mov     x0, x3
    mov     x4, x1
    mov     w1, #2
    ADRG    x2, pour_ten_steps
    mov     w3, #1
    bl      gradient_new
    mov     w2, w9
    POP2    x9, x30
pour_dynamic_gradient.absent:
    ret

// pour_final_map (x19 = config): Gradient::new(final stops, final steps) and
// its coordinate mapping over the text rectangle.
pour_final_map:
    PUSH1   x30
    ldr     x0, [x19, #POUR.final_steps]
    ldr     x3, [x19, #POUR.final_step_count]
    ldr     x1, [x19, #POUR.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, pour_spectrum
    ldr     x0, [x19, #POUR.final_stops]
    ldr     x1, [x19, #POUR.final_stop_count]
    ldr     x2, [x19, #POUR.final_steps]
    ldr     x3, [x19, #POUR.final_step_count]
    LDX     x4, pour_spectrum
    bl      gradient_new
    LDX     x0, pour_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, pour_map_width
    ldr     x6, [x19, #POUR.final_direction]
    bl      gradient_map
    STX     x9, pour_map
    POP1    x30
    ret

// pour_take_group: current_group = pending_groups.remove(0), when a pending
// group is left. Even groups pour in order, odd ones reversed. Clobbers x9,
// x3, x2, x10 (and x16).
pour_take_group:
    LDX     x9, pour_next_group
    LDX     x3, pour_group_count
    cmp     x9, x3
    b.hs    pour_take_group.none
    add     x3, x9, #1
    STX     x3, pour_next_group
    lsl     x3, x9, #4
    LDX     x2, pour_groups
    add     x3, x3, x2
    ldr     x2, [x3, #8]
    STX     x2, pour_left
    ldr     x3, [x3]
    mov     x10, #4
    tbz     w9, #0, pour_take_group.set
    add     x3, x3, x2, lsl #2
    sub     x3, x3, #4
    mov     x10, #-4
pour_take_group.set:
    STX     x10, pour_stride
    STX     x3, pour_cursor
pour_take_group.none:
    ret

// pour_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
pour_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x9, pour_next_group
    LDX     x3, pour_group_count
    cmp     x9, x3
    b.lo    pour_next_frame.run
    LDX     x9, pour_left
    cbnz    x9, pour_next_frame.run
    bl      active_empty
    cbnz    w9, pour_next_frame.finished
pour_next_frame.run:
    LDX     x9, pour_left
    cbnz    x9, pour_next_frame.pour
    bl      pour_take_group
    LDX     x9, pour_left
    cbz     x9, pour_next_frame.update
pour_next_frame.pour:
    LDX     x9, pour_gap_left
    cbz     x9, pour_next_frame.release
    sub     x9, x9, #1
    STX     x9, pour_gap_left
    b       pour_next_frame.update
pour_next_frame.release:
    // pour_speed characters (fewer when the group runs out)
    LDX     x19, effect_config
    ldr     x21, [x19, #POUR.speed]
pour_next_frame.next:
    cmp     x21, #0
    b.le    pour_next_frame.released
    LDX     x9, pour_left
    cbz     x9, pour_next_frame.released
    sub     x21, x21, #1
    sub     x9, x9, #1
    STX     x9, pour_left
    LDX     x9, pour_cursor
    ldr     w22, [x9]
    LDX     x3, pour_stride
    add     x9, x9, x3
    STX     x9, pour_cursor
    mov     w0, w22
    bl      set_visible
    mov     w0, w22
    bl      active_insert
    b       pour_next_frame.next
pour_next_frame.released:
    ldr     x9, [x19, #POUR.gap]
    STX     x9, pour_gap_left
pour_next_frame.update:
    bl      update
    mov     w9, #1
    b       pour_next_frame.out
pour_next_frame.finished:
    mov     w9, #0
pour_next_frame.out:
    POP2    x22, x30
    POP2    x19, x21
    ret

    .section .rodata
    .balign 8
pour_ten_steps: .quad 10
// CharacterGroup per PourDirection
pour_groupings: .byte GROUP_ROW_TOP_TO_BOTTOM, GROUP_ROW_BOTTOM_TO_TOP, GROUP_COLUMN_L2R, GROUP_COLUMN_R2L

    TSTATE
    .balign 8
pour_groups:            .skip 8
pour_group_count:       .skip 8
pour_next_group:        .skip 8
pour_cursor:            .skip 8
pour_stride:            .skip 8
pour_left:              .skip 8
pour_gap_left:          .skip 8
pour_spectrum:          .skip 8
pour_map:               .skip 8
pour_map_width:         .skip 8
pour_pair_stops:        .skip 8 * 2
pour_pair_spectrum:     .skip 8
pour_pair_len:          .skip 8
pour_last_fg:           .skip 8
pour_symbol:            .skip 8
pour_fg_spectrum:       .skip 8 * 16
pour_bg_spectrum:       .skip 8 * 16

    .text
