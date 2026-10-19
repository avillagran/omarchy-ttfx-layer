// effects/bubbles.s - "Characters are formed into bubbles that float down
// and pop into position" (src/effects/bubbles.rs).
//
// Config (src/asm/effects.rs, the Bubbles arm). --movement-easing is parsed
// but never read upstream, so it is not passed.
//
// A bubble is a run of characters (rows bottom to top) around an invisible
// anchor character whose path carries it to the floor; every move places the
// characters on a circle around the anchor. The rainbow sheen scene is made
// looping up front: Rust sets is_looping right after activating it, and
// activation does not read the flag.

.equ HAVE_bubbles, 1

.equ BUBBLES.rainbow,           0
.equ BUBBLES.colors,            8       // *const u64 bubble colors
.equ BUBBLES.color_count,       16
.equ BUBBLES.pop_color,         24
.equ BUBBLES.speed,             32      // f64 bubble speed
.equ BUBBLES.delay,             40
.equ BUBBLES.pop_condition,     48      // 0 row, 1 bottom, 2 anywhere
.equ BUBBLES.final_stops,       56      // *const u64
.equ BUBBLES.final_stop_count,  64
.equ BUBBLES.final_steps,       72      // *const i64
.equ BUBBLES.final_step_count,  80
.equ BUBBLES.final_direction,   88
.equ BUBBLES_size,              96

// BubblesIterator.Bubble
.equ BUB.chars,     0                   // *u32 slots (a slice of the row list)
.equ BUB.n,         8
.equ BUB.radius,    16
.equ BUB.lowest,    24                  // lowest_row
.equ BUB.anchor,    32
.equ BUB.landed,    36
.equ BUB.trig,      40                  // radius * (cos, sin) per point, or 0
.equ BUB_size,      48

.equ BUB_POP_ROW,           0
.equ BUB_POP_ANYWHERE,      2

// scene names
.equ BUB_SCN_POP1,          NAME_LITERAL + 0
// path names
.equ BUB_PATH_FINAL,        NAME_LITERAL + 0
.equ BUB_PATH_POP_OUT,      NAME_LITERAL + 1

.equ BUB_EASE_OUT_EXPO,     17
.equ BUB_EASE_IN_OUT_EXPO,  18

    .text

// bubbles_build: Bubbles::new (the rainbow gradient) + Bubbles::build.
bubbles_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    ADRG    x0, bub_rainbow_stops
    mov     w1, #7
    ADRG    x2, bub_five_steps
    mov     w3, #1
    ADRG    x4, bub_rainbow
    bl      gradient_new
    STX     x9, bub_rainbow_len
    bl      bub_final_map
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    cset    w9, eq
    STB     w9, bub_dynamic
    // every input character: layer, pop scenes, final scene and path
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    mov     x23, #0
bubbles_build.setup:
    cmp     x23, x22
    b.hs    bubbles_build.rows
    ldr     w0, [x21, x23, lsl #2]
    bl      bub_setup_char
    add     x23, x23, #1
    b       bubbles_build.setup
bubbles_build.rows:
    // unbubbled_chars: the row groups, bottom to top, flattened
    mov     w0, #FILTER_INPUT
    mov     w1, #GROUP_ROW_BOTTOM_TO_TOP
    bl      get_characters_grouped
    mov     x21, x9
    mov     x22, x2
    mov     x23, #0                     // total
    mov     x3, #0
bubbles_build.sum:
    cmp     x3, x22
    b.hs    bubbles_build.summed
    add     x9, x21, x3, lsl #4
    ldr     x9, [x9, #8]
    add     x23, x23, x9
    add     x3, x3, #1
    b       bubbles_build.sum
bubbles_build.summed:
    lsl     x0, x23, #2
    add     x0, x0, #64
    bl      alloc
    mov     x24, x9                     // the flat list
    mov     x19, #0                     // group
    mov     x20, #0                     // position in the flat list
bubbles_build.group:
    cmp     x19, x22
    b.hs    bubbles_build.flat
    add     x9, x21, x19, lsl #4
    ldr     x1, [x9]
    ldr     x3, [x9, #8]
bubbles_build.copy:
    cbz     x3, bubbles_build.next_group
    ldr     w2, [x1]
    str     w2, [x24, x20, lsl #2]
    add     x1, x1, #4
    add     x20, x20, #1
    sub     x3, x3, #1
    b       bubbles_build.copy
bubbles_build.next_group:
    add     x19, x19, #1
    b       bubbles_build.group
bubbles_build.flat:
    add     x0, x23, #1
    mov     x9, #BUB_size
    mul     x0, x0, x9
    bl      alloc
    STX     x9, bub_list
    lsl     x0, x23, #3
    add     x0, x0, #64
    bl      alloc
    STX     x9, bub_anim
    // take bubbles off the front until nothing is left
    mov     x19, #0                     // taken
bubbles_build.bubble:
    subs    x21, x23, x19               // remaining
    b.eq    bubbles_build.state
    cmp     x21, #5
    b.lo    bubbles_build.take
    mov     x1, x21
    mov     x9, #20
    cmp     x1, x9
    csel    x1, x9, x1, hi
    mov     x0, #5
    bl      rng_randint
    mov     x21, x9
bubbles_build.take:
    mov     x0, #1
    LDX     x1, canvas_right
    bl      rng_randint
    LDX     x2, canvas_top
    add     x2, x2, #10
    lsl     x2, x2, #32
    mov     w9, w9
    orr     x2, x2, x9                  // bubble_origin
    add     x0, x24, x19, lsl #2
    mov     x1, x21
    bl      bub_make
    add     x19, x19, x21
    b       bubbles_build.bubble
bubbles_build.state:
    STX     xzr, bub_next
    STX     xzr, bub_anim_count
    STX     xzr, bub_steps
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// bub_setup_char(w0=slot): the per-character part of Bubbles::build.
bub_setup_char:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w19, w0
    mov     x1, #1
    bl      set_layer
    mov     w0, w19
    MOV64   w1, BUB_SCN_POP1
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w21, w9                     // pop_1
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w22, w9                     // pop_2
    LDX     x9, effect_config
    ldr     x3, [x9, #BUBBLES.pop_color]
    mov     w0, w21
    LDX     x1, bub_sym_star
    mov     w2, #9
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    LDX     x9, effect_config
    ldr     x3, [x9, #BUBBLES.pop_color]
    mov     w0, w22
    LDX     x1, bub_sym_tick
    mov     w2, #9
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w23, w9                     // final scene
    LDX     x9, ch_sym
    ldr     x9, [x9, w19, uxtw #3]
    STX     x9, bub_sym
    LDB     w9, bub_dynamic
    cbz     w9, bub_setup_char.mapped
    // dynamic: pop color -> the input colors, whichever exist
    mov     x20, #0                     // fg spectrum
    mov     x24, #0                     // bg spectrum
    LDX     x9, ch_fg
    ldr     x1, [x9, w19, uxtw #3]
    cmn     x1, #1                      // NONE
    b.eq    bub_setup_char.bg
    ADRG    x0, bub_fg_spectrum
    bl      bub_pop_gradient
    STX     x9, bub_fg_len
    ADRG    x20, bub_fg_spectrum
bub_setup_char.bg:
    LDX     x9, ch_bg
    ldr     x1, [x9, w19, uxtw #3]
    cmn     x1, #1                      // NONE
    b.eq    bub_setup_char.dynamic_frames
    ADRG    x0, bub_bg_spectrum
    bl      bub_pop_gradient
    STX     x9, bub_bg_len
    ADRG    x24, bub_bg_spectrum
bub_setup_char.dynamic_frames:
    orr     x9, x20, x24
    cbnz    x9, bub_setup_char.dynamic_gradient
    mov     w0, w23
    LDX     x1, bub_sym
    mov     w2, #6
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       bub_setup_char.events
bub_setup_char.dynamic_gradient:
    LDX     x7, bub_bg_len
    mov     x6, x24
    mov     w0, w23
    ADRG    x1, bub_sym
    mov     x2, #1
    mov     w3, #6
    mov     x4, x20
    LDX     x5, bub_fg_len
    bl      scene_apply_gradient
    b       bub_setup_char.events
bub_setup_char.mapped:
    mov     w0, w19
    bl      bub_final_color
    mov     x1, x9
    ADRG    x0, bub_fg_spectrum
    bl      bub_pop_gradient
    mov     x6, #0
    mov     x7, #0
    mov     w0, w23
    ADRG    x1, bub_sym
    mov     x2, #1
    mov     w3, #6
    ADRG    x4, bub_fg_spectrum
    mov     x5, x9
    bl      scene_apply_gradient
bub_setup_char.events:
    // pop_1 complete -> pop_2, pop_2 complete -> final
    MOV64   x12, SC_NAME
    SCENE_PTR x9, x21
    ldr     w3, [x9, x12]
    SCENE_PTR x9, x22
    ldr     w5, [x9, x12]
    SCENE_PTR x9, x23
    ldr     w20, [x9, x12]
    mov     x6, #0
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    mov     w4, #ACT_ACTIVATE_SCENE
    bl      event_register
    MOV64   x12, SC_NAME
    SCENE_PTR x9, x22
    ldr     w3, [x9, x12]
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    mov     w4, #ACT_ACTIVATE_SCENE
    mov     w5, w20
    mov     x6, #0
    bl      event_register
    // the final path home, then layer 0
    mov     w0, w19
    LDD     d0, bub_speed_pop
    mov     w1, #BUB_EASE_IN_OUT_EXPO
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, BUB_PATH_FINAL
    bl      path_new
    mov     w20, w9
    mov     w0, w19
    bl      char_input_coord
    mov     x1, x9
    mov     w0, w20
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     x6, #0
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    MOV64   w3, BUB_PATH_FINAL
    mov     w4, #ACT_SET_LAYER
    mov     x5, #0
    bl      event_register
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// bub_pop_gradient(x0=spectrum out, x1=color) -> x9 = length:
// Gradient::with_steps([pop_color, color], 8).
bub_pop_gradient:
    str     x30, [sp, #-16]!
    LDX     x9, effect_config
    ldr     x9, [x9, #BUBBLES.pop_color]
    ADRG    x2, bub_pair_stops
    stp     x9, x1, [x2]
    mov     x4, x0
    mov     x0, x2
    mov     w1, #2
    ADRG    x2, bub_eight_steps
    mov     w3, #1
    bl      gradient_new
    mov     w9, w9
    ldr     x30, [sp], #16
    ret

// bub_final_color(w0=slot) -> x9: character_final_color_map, the final
// gradient at the input coordinate.
bub_final_color:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, w0, uxtw #2]
    LDX     x3, text_bottom
    sub     x9, x9, x3
    LDX     x3, bub_final_map_width
    mul     x9, x9, x3
    LDX     x3, ch_icol
    ldrsw   x3, [x3, w0, uxtw #2]
    add     x9, x9, x3
    LDX     x3, text_left
    sub     x9, x9, x3
    LDX     x3, bub_final_map_ptr
    ldr     x9, [x3, x9, lsl #3]
    ret

// bub_final_map: Gradient::new(final stops, final steps) and its coordinate
// mapping over the text rectangle.
bub_final_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #BUBBLES.final_steps]
    ldr     x3, [x19, #BUBBLES.final_step_count]
    ldr     x1, [x19, #BUBBLES.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, bub_final_spectrum
    ldr     x0, [x19, #BUBBLES.final_stops]
    ldr     x1, [x19, #BUBBLES.final_stop_count]
    ldr     x2, [x19, #BUBBLES.final_steps]
    ldr     x3, [x19, #BUBBLES.final_step_count]
    LDX     x4, bub_final_spectrum
    bl      gradient_new
    LDX     x0, bub_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, bub_final_map_width
    ldr     x6, [x19, #BUBBLES.final_direction]
    bl      gradient_map
    STX     x9, bub_final_map_ptr
    POP2    x19, x30
    ret

// bub_make(x0=chars, x1=count, x2=origin): Bubble.__init__ with
// make_waypoints and make_gradients; appends the bubble to bub_list.
// [sp, #56]: the sheen scene.
bub_make:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     x21, x0
    mov     x22, x1
    mov     x23, x2
    LDX     x9, bub_count
    add     x3, x9, #1
    STX     x3, bub_count
    mov     x3, #BUB_size
    LDX     x19, bub_list
    madd    x19, x9, x3, x19
    str     x21, [x19, #BUB.chars]
    str     x22, [x19, #BUB.n]
    mov     x3, #5
    udiv    x9, x22, x3
    mov     x3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    str     x9, [x19, #BUB.radius]
    LDX     x0, bub_sym_space
    mov     x1, x23
    bl      add_character
    str     w9, [x19, #BUB.anchor]
    // lowest_row: the bubble's lowest input row, or the canvas bottom
    mov     x9, #1
    str     x9, [x19, #BUB.lowest]
    LDX     x9, effect_config
    ldr     x9, [x9, #BUBBLES.pop_condition]
    cmp     x9, #BUB_POP_ROW
    b.ne    bub_make.coords
    LDX     x2, ch_irow
    mov     x9, #0x7fffffffffffffff
    mov     x3, #0
bub_make.min_row:
    cmp     x3, x22
    b.hs    bub_make.lowest
    ldr     w1, [x21, x3, lsl #2]
    ldrsw   x1, [x2, x1, lsl #2]
    cmp     x1, x9
    csel    x9, x1, x9, lt
    add     x3, x3, #1
    b       bub_make.min_row
bub_make.lowest:
    str     x9, [x19, #BUB.lowest]
bub_make.coords:
    mov     x0, x19
    bl      bub_set_coords
    str     wzr, [x19, #BUB.landed]
    // make_waypoints: the anchor floats to a random column on the floor
    mov     x0, #1
    LDX     x1, canvas_right
    bl      rng_randint
    ldr     x23, [x19, #BUB.lowest]
    lsl     x23, x23, #32
    mov     w9, w9
    orr     x23, x23, x9
    ldr     w0, [x19, #BUB.anchor]
    LDX     x9, effect_config
    ldr     d0, [x9, #BUBBLES.speed]
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w20, w9
    mov     w0, w9
    mov     x1, x23
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    ldr     w0, [x19, #BUB.anchor]
    mov     w1, w20
    bl      path_activate
    // make_gradients
    LDX     x9, effect_config
    ldr     x3, [x9, #BUBBLES.rainbow]
    cbnz    x3, bub_make.rainbow
    ldr     x0, [x9, #BUBBLES.color_count]
    bl      rng_below
    LDX     x3, effect_config
    ldr     x3, [x3, #BUBBLES.colors]
    ldr     x23, [x3, x9, lsl #3]       // bubble_color
    mov     x24, #0
bub_make.plain:
    cmp     x24, x22
    b.hs    bub_make.done
    ldr     w20, [x21, x24, lsl #2]
    add     x24, x24, #1
    mov     w0, w20
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     w9, [sp, #56]
    mov     w0, w9
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    mov     w2, #1
    mov     x3, x23
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w20
    ldr     w1, [sp, #56]
    bl      scene_activate
    b       bub_make.plain
bub_make.rainbow:
    // frame j of character k is spectrum[(rot + j) % len], where rot sums
    // the growing offsets (the list is rotated by each one in turn)
    STX     xzr, bub_rot
    mov     x23, #0                     // gradient_offset
    mov     x24, #0
bub_make.sheen:
    cmp     x24, x22
    b.hs    bub_make.done
    ldr     w20, [x21, x24, lsl #2]
    add     x24, x24, #1
    mov     w0, w20
    mov     w1, #AUTO
    mov     w2, #SCF_LOOPING
    mov     w3, #NONE
    bl      scene_new
    str     w9, [sp, #56]
    mov     x19, #0                     // j (the bubble record is done)
bub_make.step:
    LDX     x9, bub_rainbow_len
    cmp     x19, x9
    b.hs    bub_make.rotate
    LDX     x3, bub_rot
    add     x3, x3, x19
    udiv    x2, x3, x9
    msub    x2, x2, x9, x3
    ADRG    x9, bub_rainbow
    ldr     x3, [x9, x2, lsl #3]
    ldr     w0, [sp, #56]
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    mov     w2, #4
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    add     x19, x19, #1
    b       bub_make.step
bub_make.rotate:
    LDX     x3, bub_rainbow_len
    add     x9, x23, #2
    udiv    x2, x9, x3
    msub    x23, x2, x3, x9
    LDX     x9, bub_rot
    add     x9, x9, x23
    udiv    x2, x9, x3
    msub    x2, x2, x3, x9
    STX     x2, bub_rot
    mov     w0, w20
    ldr     w1, [sp, #56]
    bl      scene_activate
    b       bub_make.sheen
bub_make.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// bub_set_coords(x0=bubble): Bubble.set_character_coordinates:
// find_coords_on_circle(anchor, radius, n, false). Its trig does not depend
// on the anchor, so radius * cos and radius * sin of each point's angle are
// made once per bubble (the same sincos of the same angles) and each move
// only adds the anchor and rounds, in Rust's order.
// [sp, #48] origin column, [sp, #56] row (f64).
bub_set_coords:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x30, [sp, #32]
    mov     x19, x0
    ldr     x9, [x19, #BUB.radius]
    cbz     x9, bub_set_coords.moved    // no points at all
    ldr     x21, [x19, #BUB.trig]
    cbnz    x21, bub_set_coords.placed
    bl      bub_trig
    mov     x21, x9
bub_set_coords.placed:
    ldr     w0, [x19, #BUB.anchor]
    bl      char_coord
    sxtw    x3, w9
    scvtf   d0, x3
    str     d0, [sp, #48]
    asr     x9, x9, #32
    scvtf   d0, x9
    str     d0, [sp, #56]
    mov     x20, #0
bub_set_coords.char:
    ldr     x9, [x19, #BUB.n]
    cmp     x20, x9
    b.hs    bub_set_coords.moved
    // x = column + radius * cos; x += x - column; y = row + radius * sin
    add     x9, x21, x20, lsl #4
    ldr     d0, [x9]
    ldr     d2, [sp, #48]
    fadd    d0, d0, d2
    fsub    d1, d0, d2
    fadd    d0, d0, d1
    ROUND_HALF_EVEN
    mov     w22, w9
    add     x9, x21, x20, lsl #4
    ldr     d0, [x9, #8]
    ldr     d2, [sp, #56]
    fadd    d0, d0, d2
    ROUND_HALF_EVEN
    mov     x23, x9
    orr     x22, x22, x9, lsl #32
    ldr     x9, [x19, #BUB.chars]
    ldr     w0, [x9, x20, lsl #2]
    mov     x1, x22
    bl      set_coordinate
    sxtw    x9, w23
    ldr     x3, [x19, #BUB.lowest]
    cmp     x9, x3
    b.ne    bub_set_coords.next
    mov     w9, #1
    str     w9, [x19, #BUB.landed]
bub_set_coords.next:
    add     x20, x20, #1
    b       bub_set_coords.char
bub_set_coords.moved:
    LDX     x9, effect_config
    ldr     x9, [x9, #BUBBLES.pop_condition]
    cmp     x9, #BUB_POP_ANYWHERE
    b.ne    bub_set_coords.done
    bl      rng_random
    LDD     d1, bub_pop_chance
    fcmp    d1, d0
    b.le    bub_set_coords.done
    mov     w9, #1
    str     w9, [x19, #BUB.landed]
bub_set_coords.done:
    ldp     x23, x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// bub_trig(x19=bubble) -> x9 = n (radius * cos(a_i), radius * sin(a_i))
// pairs, a_i = (2 pi / n) * i as find_coords_on_circle computes them; kept
// in BUB.trig. Clobbers C except x19-x24.
// [sp] sin, [sp, #8] cos, [sp, #16] angle_step, [sp, #24] radius (f64).
bub_trig:
    sub     sp, sp, #64
    stp     x21, x22, [sp, #32]
    stp     x23, x30, [sp, #48]
    ldr     x0, [x19, #BUB.n]
    lsl     x0, x0, #4
    add     x0, x0, #16
    bl      alloc
    mov     x21, x9
    str     x9, [x19, #BUB.trig]
    ldr     x22, [x19, #BUB.n]
    cmp     x22, #0
    b.le    bub_trig.done
    scvtf   d1, x22
    LDD     d0, bub_two_pi
    fdiv    d0, d0, d1
    str     d0, [sp, #16]               // angle_step
    ldr     x9, [x19, #BUB.radius]
    scvtf   d0, x9
    str     d0, [sp, #24]
    mov     x23, #0
bub_trig.point:
    cmp     x23, x22
    b.ge    bub_trig.done
    scvtf   d0, x23
    ldr     d1, [sp, #16]
    fmul    d0, d0, d1                  // angle
    mov     x0, sp
    add     x1, sp, #8
    CCALL   sincos
    ldr     d0, [sp, #8]
    ldr     d1, [sp, #24]
    fmul    d0, d0, d1                  // radius * cos
    str     d0, [x21]
    ldr     d0, [sp]
    fmul    d0, d0, d1                  // radius * sin
    str     d0, [x21, #8]
    add     x21, x21, #16
    add     x23, x23, #1
    b       bub_trig.point
bub_trig.done:
    ldr     x9, [x19, #BUB.trig]
    ldp     x23, x30, [sp, #48]
    ldp     x21, x22, [sp, #32]
    add     sp, sp, #64
    ret

// bub_pop(x0=bubble): Bubble.pop - each character (zipped with the unique
// points of a wider circle) gets a pop_out path that hands over to "final",
// then all of them start pop_1 and pop_out and join the active set.
bub_pop:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    stp     x23, x30, [sp, #32]
    mov     x19, x0
    ldr     w0, [x19, #BUB.anchor]
    bl      char_coord
    mov     x0, x9
    ldr     x1, [x19, #BUB.radius]
    add     x1, x1, #3
    ldr     x2, [x19, #BUB.n]
    mov     w3, #1
    bl      find_coords_on_circle
    mov     x21, x9
    mov     x22, x2
    ldr     x9, [x19, #BUB.n]
    cmp     x22, x9
    csel    x22, x9, x22, hi
    mov     x23, #0
bub_pop.path:
    cmp     x23, x22
    b.hs    bub_pop.activate
    ldr     x9, [x19, #BUB.chars]
    ldr     w20, [x9, x23, lsl #2]
    mov     w0, w20
    LDD     d0, bub_speed_pop
    mov     w1, #BUB_EASE_OUT_EXPO
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, BUB_PATH_POP_OUT
    bl      path_new
    mov     w0, w9
    ldr     x1, [x21, x23, lsl #3]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     x6, #0
    mov     w0, w20
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    MOV64   w3, BUB_PATH_POP_OUT
    mov     w4, #ACT_ACTIVATE_PATH
    MOV64   w5, BUB_PATH_FINAL
    bl      event_register
    add     x23, x23, #1
    b       bub_pop.path
bub_pop.activate:
    mov     x23, #0
bub_pop.char:
    ldr     x9, [x19, #BUB.n]
    cmp     x23, x9
    b.hs    bub_pop.insert
    ldr     x9, [x19, #BUB.chars]
    ldr     w20, [x9, x23, lsl #2]
    mov     w0, w20
    MOV64   w1, BUB_SCN_POP1
    bl      scene_activate_name
    mov     w0, w20
    MOV64   w1, BUB_PATH_POP_OUT
    bl      path_activate_name
    add     x23, x23, #1
    b       bub_pop.char
bub_pop.insert:
    mov     x23, #0
bub_pop.active:
    ldr     x9, [x19, #BUB.n]
    cmp     x23, x9
    b.hs    bub_pop.done
    ldr     x9, [x19, #BUB.chars]
    ldr     w0, [x9, x23, lsl #2]
    bl      active_insert
    add     x23, x23, #1
    b       bub_pop.active
bub_pop.done:
    ldp     x23, x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// bub_move(x0=bubble): Bubble.move.
bub_move:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     x19, x0
    ldr     w0, [x19, #BUB.anchor]
    bl      motion_move
    mov     x0, x19
    bl      bub_set_coords
    mov     x21, #0
    ldr     x22, [x19, #BUB.chars]
bub_move.char:
    ldr     x9, [x19, #BUB.n]
    cmp     x21, x9
    b.hs    bub_move.done
    ldr     w0, [x22, x21, lsl #2]
    bl      step_animation
    add     x21, x21, #1
    b       bub_move.char
bub_move.done:
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// bubbles_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
bubbles_next_frame:
    stp     x19, x20, [sp, #-32]!
    stp     x21, x30, [sp, #16]
    LDX     x9, bub_anim_count
    cbnz    x9, bubbles_next_frame.frame
    LDX     x9, bub_next
    LDX     x3, bub_count
    cmp     x9, x3
    b.lo    bubbles_next_frame.frame
    bl      active_empty
    cbz     w9, bubbles_next_frame.frame
    mov     w9, #0
    b       bubbles_next_frame.out
bubbles_next_frame.frame:
    // release the next bubble every bubble_delay steps
    LDX     x19, bub_next
    LDX     x3, bub_count
    cmp     x19, x3
    b.hs    bubbles_next_frame.count
    LDX     x9, effect_config
    ldr     x9, [x9, #BUBBLES.delay]
    LDX     x3, bub_steps
    cmp     x3, x9
    b.lt    bubbles_next_frame.count
    add     x9, x19, #1
    STX     x9, bub_next
    mov     x3, #BUB_size
    LDX     x9, bub_list
    madd    x19, x19, x3, x9
    mov     x21, #0
bubbles_next_frame.show:
    ldr     x9, [x19, #BUB.n]
    cmp     x21, x9
    b.hs    bubbles_next_frame.shown
    ldr     x9, [x19, #BUB.chars]
    ldr     w0, [x9, x21, lsl #2]
    bl      set_visible
    add     x21, x21, #1
    b       bubbles_next_frame.show
bubbles_next_frame.shown:
    LDX     x9, bub_anim
    LDX     x3, bub_anim_count
    str     x19, [x9, x3, lsl #3]
    add     x3, x3, #1
    STX     x3, bub_anim_count
    STX     xzr, bub_steps
bubbles_next_frame.count:
    LDX     x9, bub_steps
    add     x9, x9, #1
    STX     x9, bub_steps
    // landed bubbles pop
    mov     x21, #0
bubbles_next_frame.landed:
    LDX     x9, bub_anim_count
    cmp     x21, x9
    b.hs    bubbles_next_frame.retain
    LDX     x9, bub_anim
    ldr     x0, [x9, x21, lsl #3]
    add     x21, x21, #1
    ldr     w9, [x0, #BUB.landed]
    cbz     w9, bubbles_next_frame.landed
    bl      bub_pop
    b       bubbles_next_frame.landed
bubbles_next_frame.retain:
    mov     x21, #0                     // read
    mov     x20, #0                     // write
    LDX     x9, bub_anim
    LDX     x2, bub_anim_count
bubbles_next_frame.keep:
    cmp     x21, x2
    b.hs    bubbles_next_frame.kept
    ldr     x0, [x9, x21, lsl #3]
    add     x21, x21, #1
    ldr     w3, [x0, #BUB.landed]
    cbnz    w3, bubbles_next_frame.keep
    str     x0, [x9, x20, lsl #3]
    add     x20, x20, #1
    b       bubbles_next_frame.keep
bubbles_next_frame.kept:
    STX     x20, bub_anim_count
    // the rest float on
    mov     x21, #0
bubbles_next_frame.move:
    LDX     x9, bub_anim_count
    cmp     x21, x9
    b.hs    bubbles_next_frame.update
    LDX     x9, bub_anim
    ldr     x0, [x9, x21, lsl #3]
    add     x21, x21, #1
    bl      bub_move
    b       bubbles_next_frame.move
bubbles_next_frame.update:
    bl      update
    mov     w9, #1
bubbles_next_frame.out:
    ldp     x21, x30, [sp, #16]
    ldp     x19, x20, [sp], #32
    ret

    .section .rodata
    .balign 8
bub_two_pi:         .8byte 0x401921FB54442D18   // 2 * pi
bub_rainbow_stops:  .8byte 0xe81416, 0xffa500, 0xfaeb36, 0x79c314, 0x487de7, 0x4b369d, 0x70369d
bub_five_steps:     .8byte 5
bub_eight_steps:    .8byte 8
bub_speed_pop:      .double 0.3
bub_pop_chance:     .double 0.002
bub_sym_star:       .8byte 0x10000002a          // "*"
bub_sym_tick:       .8byte 0x100000027          // "'"
bub_sym_space:      .8byte 0x100000020          // " "

    TSTATE
    .balign 8
bub_rainbow:            .skip 8 * 64
bub_rainbow_len:        .skip 8
bub_rot:                .skip 8
bub_final_spectrum:     .skip 8
bub_final_map_ptr:      .skip 8
bub_final_map_width:    .skip 8
bub_pair_stops:         .skip 8 * 2
bub_fg_spectrum:        .skip 8 * 16
bub_bg_spectrum:        .skip 8 * 16
bub_fg_len:             .skip 8
bub_bg_len:             .skip 8
bub_sym:                .skip 8
bub_list:               .skip 8
bub_count:              .skip 8
bub_next:               .skip 8
bub_anim:               .skip 8
bub_anim_count:         .skip 8
bub_steps:              .skip 8
bub_dynamic:            .skip 1

    .text
