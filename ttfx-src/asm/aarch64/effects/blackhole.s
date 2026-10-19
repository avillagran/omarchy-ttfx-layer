// effects/blackhole.s - "Characters are consumed by a black hole and
// explode outwards" (src/effects/blackhole.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Blackhole):
//
// Everything is created in exactly Rust's order, so every RNG draw and every
// auto-numbered scene and path name lines up. The starfield's visuals (7
// symbols x 7 colors, each with its 12-frame fade) are made once and reused.

.equ HAVE_blackhole, 1

.equ BLACKHOLE.blackhole_color,     0
.equ BLACKHOLE.star_colors,         8       // *const u64
.equ BLACKHOLE.star_count,          16
.equ BLACKHOLE.final_stops,         24      // *const u64
.equ BLACKHOLE.final_stop_count,    32
.equ BLACKHOLE.final_steps,         40      // *const i64
.equ BLACKHOLE.final_step_count,    48
.equ BLACKHOLE.final_direction,     56
.equ BLACKHOLE_size,                64

.equ BH_STAR_SYMBOLS,       7
.equ BH_STARFIELD_COLORS,   7           // with_steps(#4a4a4d -> #ffffff, 6)
.equ BH_FADE_FRAMES,        12          // 11 fade colors, then " "

// phases
.equ BH_PH_FORMING,         0
.equ BH_PH_CONSUMING,       1
.equ BH_PH_COLLAPSING,      2
.equ BH_PH_EXPLODING,       3
.equ BH_PH_COMPLETE,        4

// path names
.equ BH_P_BLACKHOLE,        NAME_LITERAL + 0
.equ BH_P_ROTATION,         NAME_LITERAL + 1
.equ BH_P_SINGULARITY,      NAME_LITERAL + 2
// scene names
.equ BH_S_BLACKHOLE,        NAME_LITERAL + 0

.equ BH_EASE_IN_OUT_SINE,   3
.equ BH_EASE_IN_CUBIC,      7
.equ BH_EASE_IN_EXPO,       16
.equ BH_EASE_OUT_EXPO,      17

// BH_CENTER -> x9 = canvas.center (packed). Clobbers x3.
.macro BH_CENTER
    LDX     x9, center_row
    lsl     x9, x9, #32
    LDW     w3, center_col
    orr     x9, x9, x3
.endm

    .text

// blackhole_build: BlackholeIterator.__init__ + build().
blackhole_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    // blackhole_radius = max(min(round(width * 0.3), round(height * 0.2)), 3)
    LDX     x9, canvas_right
    scvtf   d0, x9
    LDD     d1, bh_point3
    fmul    d0, d0, d1
    bl      round_half_even
    mov     x19, x9
    LDX     x9, canvas_top
    scvtf   d0, x9
    LDD     d1, bh_point2
    fmul    d0, d0, d1
    bl      round_half_even
    cmp     x9, x19
    csel    x9, x19, x9, gt
    mov     x3, #3
    cmp     x9, x3
    csel    x9, x3, x9, lt
    STX     x9, bh_radius
    bl      bh_final_color_map
    // ctx.preexisting_colors_present
    LDX     x21, input_chars
    LDX     x22, input_count
    mov     x19, #0
blackhole_build.present:
    cmp     x19, x22
    b.hs    blackhole_build.prepare
    ldr     w0, [x21, x19, lsl #2]
    LDX     x9, ch_fg
    LDX     x3, ch_bg
    ldr     x9, [x9, x0, lsl #3]
    ldr     x3, [x3, x0, lsl #3]
    and     x9, x9, x3
    add     x19, x19, #1
    cmn     x9, #1                      // NONE
    b.eq    blackhole_build.present
    mov     w9, #1
    STB     w9, bh_preexisting
blackhole_build.prepare:
    bl      bh_prepare
    // formation_delay = max(100 // len(blackhole_chars), 6)
    mov     x9, #100
    LDX     x3, bh_count
    udiv    x9, x9, x3
    mov     x3, #6
    cmp     x9, x3
    csel    x9, x3, x9, lt
    STX     x9, bh_formation_delay
    STX     x9, bh_f_delay
    mov     x9, #BH_PH_FORMING
    STX     x9, bh_phase
    STX     xzr, bh_form_pos
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// bh_make_visuals: the symbol tables, the starfield spectrum, each starfield
// color's fade to black, and every starfield visual.
bh_make_visuals:
    PUSH2   x19, x20
    PUSH2   x21, x30
    mov     w19, #0
bh_make_visuals.symbols:
    ADRG    x9, bh_star_codes
    ldr     w0, [x9, x19, lsl #2]
    bl      utf8_pack
    ADRG    x3, bh_symbols
    str     x9, [x3, x19, lsl #3]
    ADRG    x9, bh_unstable_codes
    ldr     w0, [x9, x19, lsl #2]
    bl      utf8_pack
    ADRG    x3, bh_unstable
    str     x9, [x3, x19, lsl #3]
    add     w19, w19, #1
    cmp     w19, #BH_STAR_SYMBOLS
    b.lo    bh_make_visuals.symbols
    mov     w0, #0x20                   // ' '
    bl      utf8_pack
    STX     x9, bh_space
    // starfield_colors = Gradient::with_steps([#4a4a4d, #ffffff], 6)
    ADRG    x0, bh_pair
    MOV64   x9, 0x4a4a4d
    str     x9, [x0]
    mov     x9, #0xffffff
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, bh_six
    mov     w3, #1
    ADRG    x4, bh_starfield
    bl      gradient_new
    // gradient_map[c] = Gradient::with_steps([starfield[c], #000000], 10)
    mov     w19, #0
bh_make_visuals.fades:
    ADRG    x9, bh_starfield
    ldr     x9, [x9, x19, lsl #3]
    ADRG    x0, bh_pair
    str     x9, [x0]
    str     xzr, [x0, #8]
    mov     w1, #2
    ADRG    x2, bh_ten_steps
    mov     w3, #1
    ADRG    x4, bh_fades
    add     x4, x4, x19, lsl #7         // 16 * 8 per color
    bl      gradient_new
    add     w19, w19, #1
    cmp     w19, #BH_STARFIELD_COLORS
    b.lo    bh_make_visuals.fades
    // visuals: index = symbol * 7 + color
    mov     w19, #0
bh_make_visuals.visual:
    mov     w3, #BH_STARFIELD_COLORS
    udiv    w9, w19, w3
    msub    w2, w9, w3, w19             // w9 = symbol, w2 = color
    mov     w21, w9
    mov     w20, w2
    ADRG    x9, bh_starfield
    ldr     x0, [x9, x20, lsl #3]
    mov     x1, #NONE
    ADRG    x9, bh_symbols
    ldr     x2, [x9, x21, lsl #3]
    mov     w3, #0
    bl      visual_make
    ADRG    x3, bh_star_vis
    str     w9, [x3, x19, lsl #2]
    // the consumed scene's frames: the color's fade, then " "
    lsl     w20, w20, #4                // fade row (16 colors)
    lsl     w21, w21, #8                // symbol * 256 | fade index << 4 | k
    orr     w21, w21, w20
bh_make_visuals.fade_frame:
    and     w9, w21, #15
    cmp     w9, #(BH_FADE_FRAMES - 1)
    b.eq    bh_make_visuals.space
    and     w3, w21, #0xff
    ADRG    x9, bh_fades
    ldr     x0, [x9, x3, lsl #3]
    mov     x1, #NONE
    lsr     w3, w21, #8
    ADRG    x9, bh_symbols
    ldr     x2, [x9, x3, lsl #3]
    mov     w3, #0
    bl      visual_make
    mov     w3, #BH_FADE_FRAMES
    mul     w3, w19, w3
    and     w2, w21, #15
    add     w3, w3, w2
    ADRG    x2, bh_fade_vis
    str     w9, [x2, x3, lsl #2]
    add     w21, w21, #1
    b       bh_make_visuals.fade_frame
bh_make_visuals.space:
    mov     x0, #NONE
    mov     x1, #NONE
    LDX     x2, bh_space
    mov     w3, #0
    bl      visual_make
    mov     w3, #BH_FADE_FRAMES
    mul     w3, w19, w3
    add     w3, w3, #(BH_FADE_FRAMES - 1)
    ADRG    x2, bh_fade_vis
    str     w9, [x2, x3, lsl #2]
    add     w19, w19, #1
    cmp     w19, #(BH_STAR_SYMBOLS * BH_STARFIELD_COLORS)
    b.lo    bh_make_visuals.visual
    POP2    x21, x30
    POP2    x19, x20
    ret

// bh_prepare: BlackholeIterator.prepare_blackhole.
// Locals: [sp] a starfield position, [sp + 8] its speed, [sp + 16] the
// first fade frame's index.
bh_prepare:
    sub     sp, sp, #96
    stp     x19, x20, [sp, #32]
    stp     x21, x22, [sp, #48]
    stp     x23, x24, [sp, #64]
    str     x30, [sp, #80]
    bl      bh_make_visuals
    // available_chars = input_characters; take radius * 3 at random
    LDX     x0, input_count
    lsl     x0, x0, #2
    add     x0, x0, #4
    bl      alloc
    mov     x21, x9                     // available
    mov     x0, x9
    LDX     x1, input_chars
    LDX     x3, input_count
    REP_MOVSD
    LDX     x0, input_count
    lsl     x0, x0, #2
    add     x0, x0, #4
    bl      alloc
    STX     x9, bh_chars
    LDX     x0, input_count
    lsl     x0, x0, #2
    add     x0, x0, #4
    bl      alloc
    STX     x9, bh_consume
    LDX     x22, input_count            // available count
    LDX     x9, bh_radius
    add     x23, x9, x9, lsl #1
bh_prepare.take:
    LDX     x9, bh_count
    cmp     x9, x23
    b.ge    bh_prepare.taken
    cbz     x22, bh_prepare.taken
    mov     x0, #0
    mov     x1, x22
    bl      rng_randrange
    ldr     w3, [x21, x9, lsl #2]
    LDX     x2, bh_chars
    LDX     x1, bh_count
    str     w3, [x2, x1, lsl #2]
    add     x1, x1, #1
    STX     x1, bh_count
    // available.remove(index)
    add     x0, x21, x9, lsl #2
    add     x1, x0, #4
    sub     x3, x22, #1
    sub     x3, x3, x9
    REP_MOVSD
    sub     x22, x22, #1
    b       bh_prepare.take
bh_prepare.taken:
    // membership bitmap over slots
    LDW     w0, char_count
    add     x0, x0, #63
    lsr     x0, x0, #6
    lsl     x0, x0, #3
    add     x0, x0, #8
    bl      alloc
    STX     x9, bh_bits
    mov     x3, #0
bh_prepare.bits:
    LDX     x10, bh_count
    cmp     x3, x10
    b.hs    bh_prepare.ring
    LDX     x2, bh_chars
    ldr     w2, [x2, x3, lsl #2]
    lsr     x10, x2, #6
    ldr     x11, [x9, x10, lsl #3]
    mov     x12, #1
    lsl     x12, x12, x2
    orr     x11, x11, x12
    str     x11, [x9, x10, lsl #3]
    add     x3, x3, #1
    b       bh_prepare.bits
bh_prepare.ring:
    BH_CENTER
    mov     x0, x9
    LDX     x1, bh_radius
    LDX     x2, bh_count
    mov     w3, #1
    bl      find_coords_on_circle
    LDX     x10, bh_count
    cmp     x2, x10
    b.lo    bh_prepare.short_ring
    mov     x23, x9                     // ring positions
    mov     x24, x10
    mov     x19, #0
bh_prepare.ring_char:
    cmp     x19, x24
    b.hs    bh_prepare.starfield
    LDX     x9, bh_chars
    ldr     w21, [x9, x19, lsl #2]
    // "blackhole": 0.7, in_out_sine, to the ring position
    mov     w0, w21
    LDD     d0, bh_speed_form
    mov     w1, #BH_EASE_IN_OUT_SINE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, BH_P_BLACKHOLE
    bl      path_new
    mov     w0, w9
    ldr     x1, [x23, x19, lsl #3]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // "blackhole" scene: "*" in the blackhole color
    mov     w0, w21
    MOV64   w1, BH_S_BLACKHOLE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w0, w9
    LDX     x1, bh_symbols              // "*"
    mov     w2, #1
    LDX     x3, effect_config
    ldr     x3, [x3, #BLACKHOLE.blackhole_color]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w21
    mov     w1, #EV_PATH_ACTIVATED
    mov     w2, #CALLER_PATH
    MOV64   w3, BH_P_BLACKHOLE
    mov     w4, #ACT_SET_LAYER
    mov     w5, #1
    mov     x6, #0
    bl      event_register
    // "blackhole_rotation": 0.45, looping, the ring from this position on
    mov     w0, w21
    LDD     d0, bh_speed_rotate
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #1
    MOV64   w5, BH_P_ROTATION
    bl      path_new
    mov     w20, w9
    mov     x22, #0
bh_prepare.rotation:
    cmp     x22, x24
    b.hs    bh_prepare.ring_next
    add     x9, x19, x22
    cmp     x9, x24
    b.lo    bh_prepare.index
    sub     x9, x9, x24
bh_prepare.index:
    mov     w0, w20
    ldr     x1, [x23, x9, lsl #3]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    add     x22, x22, #1
    b       bh_prepare.rotation
bh_prepare.ring_next:
    add     x19, x19, #1
    b       bh_prepare.ring_char
bh_prepare.starfield:
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x23, x9
    mov     x24, x2
    mov     x19, #0
bh_prepare.star:
    cmp     x19, x24
    b.hs    bh_prepare.shuffle
    ldr     w21, [x23, x19, lsl #2]
    mov     w0, w21
    bl      set_visible
    mov     x0, #BH_STAR_SYMBOLS
    bl      rng_below
    mov     w3, #BH_STARFIELD_COLORS
    mul     w20, w9, w3
    mov     x0, #BH_STARFIELD_COLORS
    bl      rng_below
    add     w20, w20, w9                // symbol * 7 + color
    // starting scene: the star
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w22, w9
    mov     w0, w9
    ADRG    x9, bh_star_vis
    ldr     w1, [x9, x20, lsl #2]
    mov     w2, #1
    bl      scene_add_frame_visual
    mov     w0, w21
    mov     w1, w22
    bl      scene_activate
    LDX     x9, bh_bits
    lsr     x3, x21, #6
    ldr     x3, [x9, x3, lsl #3]
    lsr     x3, x3, x21
    tbnz    x3, #0, bh_prepare.star_next
    // outside the blackhole: a random starfield position, then the singularity
    mov     w0, #0
    mov     w1, #0
    bl      canvas_random_coord
    str     x9, [sp]
    LDD     d0, bh_speed_min
    LDD     d1, bh_speed_max
    bl      rng_uniform
    str     d0, [sp, #8]
    mov     w0, w21
    ldr     x1, [sp]
    bl      set_coordinate
    mov     w0, w21
    ldr     d0, [sp, #8]
    mov     w1, #BH_EASE_IN_EXPO
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, BH_P_SINGULARITY
    bl      path_new
    mov     w22, w9
    BH_CENTER
    mov     x1, x9
    mov     w0, w22
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // consumed scene: the fade to black and " ", synced to distance
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #SCF_SYNC_DISTANCE
    mov     w3, #NONE
    bl      scene_new
    mov     w22, w9
    mov     w3, #BH_FADE_FRAMES
    mul     w20, w20, w3
    str     x20, [sp, #16]
bh_prepare.consumed_frame:
    mov     w0, w22
    ADRG    x9, bh_fade_vis
    ldr     w1, [x9, x20, lsl #2]
    mov     w2, #1
    bl      scene_add_frame_visual
    add     w20, w20, #1
    ldr     x9, [sp, #16]
    sub     x9, x20, x9
    cmp     w9, #BH_FADE_FRAMES
    b.lo    bh_prepare.consumed_frame
    mov     w0, w21
    mov     w1, #EV_PATH_ACTIVATED
    mov     w2, #CALLER_PATH
    MOV64   w3, BH_P_SINGULARITY
    mov     w4, #ACT_SET_LAYER
    mov     w5, #2
    mov     x6, #0
    bl      event_register
    SCENE_PTR x5, x22
    MOV64   x16, SC_NAME
    ldr     w5, [x5, x16]
    mov     w0, w21
    mov     w1, #EV_PATH_ACTIVATED
    mov     w2, #CALLER_PATH
    MOV64   w3, BH_P_SINGULARITY
    mov     w4, #ACT_ACTIVATE_SCENE
    mov     x6, #0
    bl      event_register
    LDX     x9, bh_consume
    LDX     x3, bh_consume_count
    str     w21, [x9, x3, lsl #2]
    add     x3, x3, #1
    STX     x3, bh_consume_count
bh_prepare.star_next:
    add     x19, x19, #1
    b       bh_prepare.star
bh_prepare.shuffle:
    LDX     x0, bh_consume
    LDX     x1, bh_consume_count
    bl      rng_shuffle32
    ldr     x30, [sp, #80]
    ldp     x23, x24, [sp, #64]
    ldp     x21, x22, [sp, #48]
    ldp     x19, x20, [sp, #32]
    add     sp, sp, #96
    ret
bh_prepare.short_ring:
    ADRG    x0, msg_bh_ring
    mov     w1, #msg_bh_ring_len
    b       fatal

// bh_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
bh_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #BLACKHOLE.final_steps]
    ldr     x3, [x19, #BLACKHOLE.final_step_count]
    ldr     x1, [x19, #BLACKHOLE.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, bh_final_spectrum
    ldr     x0, [x19, #BLACKHOLE.final_stops]
    ldr     x1, [x19, #BLACKHOLE.final_stop_count]
    ldr     x2, [x19, #BLACKHOLE.final_steps]
    ldr     x3, [x19, #BLACKHOLE.final_step_count]
    LDX     x4, bh_final_spectrum
    bl      gradient_new
    LDX     x0, bh_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, bh_final_map_width
    ldr     x6, [x19, #BLACKHOLE.final_direction]
    bl      gradient_map
    STX     x9, bh_final_map
    POP2    x19, x30
    ret

// bh_path_name(w9=path index) -> w9 = its name. Clobbers x16.
bh_path_name:
    ubfiz   x9, x9, #7, #32             // PATH_SIZE
    adrp    x16, paths
    ldr     x16, [x16, :lo12:paths]
    add     x9, x9, x16
    ldr     w9, [x9, #PA_NAME]
    ret

// bh_scene_name(w9=scene index) -> w9 = its name. Clobbers x16.
bh_scene_name:
    SCENE_PTR x9, x9
    MOV64   x16, SC_NAME
    ldr     w9, [x9, x16]
    ret

// blackhole_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
blackhole_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x9, bh_phase
    cmp     x9, #BH_PH_COMPLETE
    b.ne    blackhole_next_frame.phase
    bl      active_empty
    cbnz    w9, blackhole_next_frame.finished
    b       blackhole_next_frame.update
blackhole_next_frame.phase:
    cmp     x9, #BH_PH_FORMING
    b.eq    blackhole_next_frame.forming
    cmp     x9, #BH_PH_CONSUMING
    b.eq    blackhole_next_frame.consuming
    cmp     x9, #BH_PH_COLLAPSING
    b.eq    blackhole_next_frame.collapsing
    // exploding: once every blackhole character has stopped moving and animating
    mov     x19, #0
blackhole_next_frame.settled:
    LDX     x10, bh_count
    cmp     x19, x10
    b.hs    blackhole_next_frame.explode
    LDX     x9, bh_chars
    ldr     w0, [x9, x19, lsl #2]
    LDX     x9, ch_path
    ldr     w9, [x9, x0, lsl #2]
    cmn     w9, #1                      // NONE
    b.ne    blackhole_next_frame.update
    LDX     x9, ch_scene
    ldr     w9, [x9, x0, lsl #2]
    cmn     w9, #1                      // NONE
    b.ne    blackhole_next_frame.update
    add     x19, x19, #1
    b       blackhole_next_frame.settled
blackhole_next_frame.explode:
    bl      bh_explode
    mov     x9, #BH_PH_COMPLETE
    STX     x9, bh_phase
    b       blackhole_next_frame.update
blackhole_next_frame.collapsing:
    bl      bh_collapse
    mov     x9, #BH_PH_EXPLODING
    STX     x9, bh_phase
    b       blackhole_next_frame.update
blackhole_next_frame.forming:
    LDX     x19, bh_form_pos
    LDX     x10, bh_count
    cmp     x19, x10
    b.hs    blackhole_next_frame.formed
    LDX     x9, bh_f_delay
    cbz     x9, blackhole_next_frame.next_char
    sub     x9, x9, #1
    STX     x9, bh_f_delay
    b       blackhole_next_frame.update
blackhole_next_frame.next_char:
    add     x9, x19, #1
    STX     x9, bh_form_pos
    LDX     x9, bh_chars
    ldr     w21, [x9, x19, lsl #2]
    mov     w0, w21
    MOV64   w1, BH_P_BLACKHOLE
    bl      path_activate_name
    mov     w0, w21
    MOV64   w1, BH_S_BLACKHOLE
    bl      scene_activate_name
    mov     w0, w21
    bl      active_insert
    LDX     x9, bh_formation_delay
    STX     x9, bh_f_delay
    b       blackhole_next_frame.update
blackhole_next_frame.formed:
    bl      active_empty
    cbz     w9, blackhole_next_frame.update
    // rotate_blackhole
    mov     x19, #0
blackhole_next_frame.rotate:
    LDX     x10, bh_count
    cmp     x19, x10
    b.hs    blackhole_next_frame.rotating
    LDX     x9, bh_chars
    ldr     w21, [x9, x19, lsl #2]
    mov     w0, w21
    MOV64   w1, BH_P_ROTATION
    bl      path_activate_name
    mov     w0, w21
    bl      active_insert
    add     x19, x19, #1
    b       blackhole_next_frame.rotate
blackhole_next_frame.rotating:
    mov     x9, #BH_PH_CONSUMING
    STX     x9, bh_phase
    b       blackhole_next_frame.update
blackhole_next_frame.consuming:
    LDX     x22, bh_consume_count
    cbz     x22, blackhole_next_frame.check_consumed
    mov     x19, #0
blackhole_next_frame.consume:
    cmp     x19, x22
    b.hs    blackhole_next_frame.consumed
    LDX     x9, bh_consume
    ldr     w21, [x9, x19, lsl #2]
    mov     w0, w21
    MOV64   w1, BH_P_SINGULARITY
    bl      path_activate_name
    mov     w0, w21
    bl      active_insert
    add     x19, x19, #1
    b       blackhole_next_frame.consume
blackhole_next_frame.consumed:
    STX     xzr, bh_consume_count
    b       blackhole_next_frame.update
blackhole_next_frame.check_consumed:
    // every active character belongs to the blackhole
    LDW     w3, char_count
    add     x3, x3, #63
    lsr     x3, x3, #6
    LDX     x9, active_bits
    LDX     x2, bh_bits
    mov     x19, #0
blackhole_next_frame.word:
    cmp     x19, x3
    b.hs    blackhole_next_frame.collapse_next
    ldr     x4, [x2, x19, lsl #3]
    ldr     x5, [x9, x19, lsl #3]
    bics    x4, x5, x4
    b.ne    blackhole_next_frame.update
    add     x19, x19, #1
    b       blackhole_next_frame.word
blackhole_next_frame.collapse_next:
    mov     x9, #BH_PH_COLLAPSING
    STX     x9, bh_phase
blackhole_next_frame.update:
    bl      update
    mov     w9, #1
    b       blackhole_next_frame.out
blackhole_next_frame.finished:
    mov     w9, #0
blackhole_next_frame.out:
    POP2    x22, x30
    POP2    x19, x21
    ret

// bh_collapse: BlackholeIterator.collapse_blackhole.
// Locals: [sp] the point character's scene, [sp + 8] its frame counter.
bh_collapse:
    sub     sp, sp, #96
    stp     x19, x20, [sp, #32]
    stp     x21, x22, [sp, #48]
    stp     x23, x24, [sp, #64]
    str     x30, [sp, #80]
    BH_CENTER
    mov     x0, x9
    LDX     x1, bh_radius
    add     x1, x1, #3
    LDX     x2, bh_count
    mov     w3, #1
    bl      find_coords_on_circle
    LDX     x10, bh_count
    cmp     x2, x10
    b.lo    bh_collapse.short_ring
    mov     x23, x9
    mov     x19, #0
bh_collapse.char:
    LDX     x10, bh_count
    cmp     x19, x10
    b.hs    bh_collapse.done
    LDX     x9, bh_chars
    ldr     w21, [x9, x19, lsl #2]
    // expand to the wider ring, then collapse to the center
    mov     w0, w21
    LDD     d0, bh_speed_expand
    mov     w1, #BH_EASE_IN_EXPO
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w20, w9
    mov     w0, w9
    ldr     x1, [x23, x19, lsl #3]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w21
    LDD     d0, bh_speed_collapse
    mov     w1, #BH_EASE_IN_EXPO
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w22, w9
    BH_CENTER
    mov     x1, x9
    mov     w0, w22
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w9, w22
    bl      bh_path_name
    mov     w24, w9                     // collapse path name
    mov     w9, w20
    bl      bh_path_name
    mov     w3, w9
    mov     w0, w21
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w4, #ACT_ACTIVATE_PATH
    mov     w5, w24
    mov     x6, #0
    bl      event_register
    cbnz    x19, bh_collapse.activate
    // the point character: 3 x 7 unstable symbols in random star colors
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     x9, [sp]
    str     wzr, [sp, #8]
bh_collapse.point_frame:
    LDX     x9, effect_config
    ldr     x0, [x9, #BLACKHOLE.star_count]
    bl      rng_below
    LDX     x3, effect_config
    ldr     x3, [x3, #BLACKHOLE.star_colors]
    ldr     x3, [x3, x9, lsl #3]
    ldr     w9, [sp, #8]
    mov     w1, #BH_STAR_SYMBOLS
    udiv    w2, w9, w1
    msub    w2, w2, w1, w9
    ADRG    x9, bh_unstable
    ldr     x1, [x9, x2, lsl #3]
    ldr     w0, [sp]
    mov     w2, #3
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    ldr     w9, [sp, #8]
    add     w9, w9, #1
    str     w9, [sp, #8]
    cmp     w9, #(3 * BH_STAR_SYMBOLS)
    b.lo    bh_collapse.point_frame
    ldr     w9, [sp]
    bl      bh_scene_name
    mov     w5, w9
    mov     w0, w21
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w3, w24
    mov     w4, #ACT_ACTIVATE_SCENE
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w3, w24
    mov     w4, #ACT_SET_LAYER
    mov     w5, #3
    mov     x6, #0
    bl      event_register
bh_collapse.activate:
    mov     w0, w21
    mov     w1, w20
    bl      path_activate
    mov     w0, w21
    bl      active_insert
    add     x19, x19, #1
    b       bh_collapse.char
bh_collapse.done:
    ldr     x30, [sp, #80]
    ldp     x23, x24, [sp, #64]
    ldp     x21, x22, [sp, #48]
    ldp     x19, x20, [sp, #32]
    add     sp, sp, #96
    ret
bh_collapse.short_ring:
    ADRG    x0, msg_bh_ring
    mov     w1, #msg_bh_ring_len
    b       fatal

// bh_explode: BlackholeIterator.explode_singularity.
//
// find_coords_on_circle(input_coord, 3, 5) is the same five offsets for
// every integer origin: none of them is near a rounding boundary (the
// doubled x offsets are 6, +-1.854, +-4.854 and the y offsets 0, +-2.853,
// +-1.763), so they are computed once around (0, 0).
bh_explode:
    sub     sp, sp, #128
    stp     x19, x20, [sp, #64]
    stp     x21, x22, [sp, #80]
    stp     x23, x24, [sp, #96]
    str     x30, [sp, #112]
    // [sp] nearby coord, [sp+8] star color, [sp+16] explode scene,
    // [sp+24] cooling scene, [sp+32] input symbol, [sp+40] input coord,
    // [sp+48] input path, [sp+56] the cooling gradient's length
    mov     x0, #0
    mov     x1, #3
    mov     x2, #5
    mov     w3, #1
    bl      find_coords_on_circle
    cmp     x2, #5
    b.lo    bh_explode.short_ring
    mov     x1, x9
    ADRG    x0, bh_offsets
    mov     x3, #5
    REP_MOVSQ
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x23, x9
    mov     x24, x2
    mov     x19, #0
bh_explode.char:
    cmp     x19, x24
    b.hs    bh_explode.done
    ldr     w21, [x23, x19, lsl #2]
    mov     w0, w21
    bl      char_input_coord
    str     x9, [sp, #40]
    LDX     x9, ch_sym
    ldr     x9, [x9, x21, lsl #3]
    str     x9, [sp, #32]
    // nearby: one of the five circle points, speed randint(3, 4) / 10
    mov     x0, #0
    mov     x1, #5
    bl      rng_randrange
    ADRG    x3, bh_offsets
    ldr     x3, [x3, x9, lsl #3]
    ldr     x9, [sp, #40]
    lsr     x2, x9, #32
    lsr     x1, x3, #32
    add     w2, w2, w1                  // row
    add     w9, w9, w3                  // column
    orr     x9, x9, x2, lsl #32
    str     x9, [sp]
    mov     x0, #3
    mov     x1, #4
    bl      rng_randint
    scvtf   d0, x9
    LDD     d1, bh_ten
    fdiv    d0, d0, d1
    mov     w0, w21
    mov     w1, #BH_EASE_OUT_EXPO
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w20, w9
    mov     w0, w9
    ldr     x1, [sp]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // back home: speed randint(4, 6) / 100
    mov     x0, #4
    mov     x1, #6
    bl      rng_randint
    scvtf   d0, x9
    LDD     d1, bh_hundred
    fdiv    d0, d0, d1
    mov     w0, w21
    mov     w1, #BH_EASE_IN_CUBIC
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    str     x9, [sp, #48]               // input path
    mov     w0, w9
    ldr     x1, [sp, #40]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     x0, #6
    bl      rng_below
    ADRG    x3, bh_explode_colors
    ldr     x9, [x3, x9, lsl #3]
    str     x9, [sp, #8]
    // explode scene: the input symbol in the star color
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     x9, [sp, #16]
    mov     w0, w9
    ldr     x1, [sp, #32]
    mov     w2, #1
    ldr     x3, [sp, #8]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     x9, [sp, #24]
    // cooling scene
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    bh_explode.final
    LDB     w9, bh_preexisting
    cbz     w9, bh_explode.final
    bl      bh_cool_dynamic
    b       bh_explode.events
bh_explode.final:
    // star color -> final gradient color in 10 steps, 20 ticks each
    ldr     x9, [sp, #8]
    STX     x9, bh_pair
    ldr     x9, [sp, #40]
    asr     x2, x9, #32
    LDX     x10, text_bottom
    sub     x2, x2, x10
    LDX     x10, bh_final_map_width
    mul     x2, x2, x10
    sxtw    x9, w9
    add     x9, x9, x2
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, bh_final_map
    ldr     x9, [x3, x9, lsl #3]
    ADRG    x0, bh_pair
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, bh_ten_steps
    mov     w3, #1
    ADRG    x4, bh_pair_spectrum
    bl      gradient_new
    str     x9, [sp, #56]
    mov     w22, #0
bh_explode.cool_frame:
    ldr     x9, [sp, #56]
    cmp     w22, w9
    b.hs    bh_explode.cooled
    ldr     w0, [sp, #24]
    ldr     x1, [sp, #32]
    mov     w2, #20
    ADRG    x9, bh_pair_spectrum
    ldr     x3, [x9, x22, lsl #3]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    add     w22, w22, #1
    b       bh_explode.cool_frame
bh_explode.cooled:
bh_explode.events:
    // nearby complete -> the input path and the cooling scene
    mov     w9, w20
    bl      bh_path_name
    mov     w22, w9                     // nearby path name
    ldr     w9, [sp, #48]
    bl      bh_path_name
    mov     w5, w9
    mov     w0, w21
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w3, w22
    mov     w4, #ACT_ACTIVATE_PATH
    mov     x6, #0
    bl      event_register
    ldr     w9, [sp, #24]
    bl      bh_scene_name
    mov     w5, w9
    mov     w0, w21
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w3, w22
    mov     w4, #ACT_ACTIVATE_SCENE
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    ldr     w1, [sp, #16]
    bl      scene_activate
    mov     w0, w21
    mov     w1, w20
    bl      path_activate
    mov     w0, w21
    bl      active_insert
    add     x19, x19, #1
    b       bh_explode.char
bh_explode.done:
    ldr     x30, [sp, #112]
    ldp     x23, x24, [sp, #96]
    ldp     x21, x22, [sp, #80]
    ldp     x19, x20, [sp, #64]
    add     sp, sp, #128
    ret
bh_explode.short_ring:
    ADRG    x0, msg_bh_ring
    mov     w1, #msg_bh_ring_len
    b       fatal

// bh_cool_dynamic(w21=slot; bh_explode's frame at sp on entry): the cooling
// scene under --existing-color-handling dynamic with input colors present -
// the input colors as is, or gradients from the star color to them.
bh_cool_dynamic:
    PUSH2   x19, x20
    PUSH1   x30
    // bh_explode's locals are now at [sp + 32]
    LDX     x9, ch_fg
    ldr     x19, [x9, x21, lsl #3]      // input fg
    LDX     x9, ch_bg
    ldr     x20, [x9, x21, lsl #3]      // input bg
    cmn     x19, #1                     // NONE
    b.ne    bh_cool_dynamic.gradients
    cmn     x20, #1
    b.ne    bh_cool_dynamic.gradients
    ldr     w0, [sp, #(32 + 24)]
    ldr     x1, [sp, #(32 + 32)]
    mov     w2, #1
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       bh_cool_dynamic.out
bh_cool_dynamic.gradients:
    STX     xzr, bh_fg_len
    STX     xzr, bh_bg_len
    cmn     x19, #1
    b.eq    bh_cool_dynamic.bg
    ldr     x9, [sp, #(32 + 8)]
    ADRG    x0, bh_pair
    stp     x9, x19, [x0]
    mov     w1, #2
    ADRG    x2, bh_ten_steps
    mov     w3, #1
    ADRG    x4, bh_pair_spectrum
    bl      gradient_new
    STX     x9, bh_fg_len
bh_cool_dynamic.bg:
    cmn     x20, #1
    b.eq    bh_cool_dynamic.apply
    ldr     x9, [sp, #(32 + 8)]
    ADRG    x0, bh_pair
    stp     x9, x20, [x0]
    mov     w1, #2
    ADRG    x2, bh_ten_steps
    mov     w3, #1
    ADRG    x4, bh_bg_spectrum
    bl      gradient_new
    STX     x9, bh_bg_len
bh_cool_dynamic.apply:
    ldr     w0, [sp, #(32 + 24)]
    add     x1, sp, #(32 + 32)          // [input symbol]
    mov     w2, #1
    mov     w3, #20
    mov     x4, #0
    mov     w5, #0
    cmn     x19, #1
    b.eq    bh_cool_dynamic.no_fg
    ADRG    x4, bh_pair_spectrum
    LDW     w5, bh_fg_len
bh_cool_dynamic.no_fg:
    mov     x6, #0
    mov     w7, #0
    cmn     x20, #1
    b.eq    bh_cool_dynamic.no_bg
    ADRG    x6, bh_bg_spectrum
    LDW     w7, bh_bg_len
bh_cool_dynamic.no_bg:
    bl      scene_apply_gradient
bh_cool_dynamic.out:
    POP1    x30
    POP2    x19, x20
    ret

    .section .rodata
    .balign 8
bh_point3:          .double 0.3
bh_point2:          .double 0.2
bh_speed_form:      .double 0.7
bh_speed_rotate:    .double 0.45
bh_speed_min:       .double 0.17
bh_speed_max:       .double 0.30
bh_speed_expand:    .double 0.2
bh_speed_collapse:  .double 0.3
bh_ten:             .double 10.0
bh_hundred:         .double 100.0
bh_six:             .quad 6
bh_ten_steps:       .quad 10
bh_explode_colors:  .quad 0xffcc0d, 0xff7326, 0xff194d, 0xbf2669, 0x702a8c, 0x049dbf
// * ' ` ¤ • ° ·
bh_star_codes:      .4byte 0x2a, 0x27, 0x60, 0xa4, 0x2022, 0xb0, 0xb7
// ◦ ◎ ◉ ● ◉ ◎ ◦
bh_unstable_codes:  .4byte 0x25e6, 0x25ce, 0x25c9, 0x25cf, 0x25c9, 0x25ce, 0x25e6
STRING msg_bh_ring, "ttfx: asm engine: blackhole ring has too few positions\n"

    TSTATE
    .balign 8
bh_radius:          .skip 8
bh_chars:           .skip 8         // u32 slots, in selection order
bh_count:           .skip 8
bh_bits:            .skip 8         // membership bitmap over slots
bh_consume:         .skip 8         // u32 slots awaiting consumption
bh_consume_count:   .skip 8
bh_formation_delay: .skip 8
bh_f_delay:         .skip 8
bh_form_pos:        .skip 8
bh_phase:           .skip 8
bh_final_spectrum:  .skip 8
bh_final_map:       .skip 8
bh_final_map_width: .skip 8
bh_symbols:         .skip 8 * BH_STAR_SYMBOLS
bh_unstable:        .skip 8 * BH_STAR_SYMBOLS
bh_space:           .skip 8
bh_starfield:       .skip 8 * 8
bh_fades:           .skip 8 * BH_STARFIELD_COLORS * 16
bh_offsets:         .skip 8 * 5
bh_pair:            .skip 8 * 2
bh_pair_spectrum:   .skip 8 * 16
bh_bg_spectrum:     .skip 8 * 16
bh_fg_len:          .skip 8
bh_bg_len:          .skip 8
bh_star_vis:        .skip 4 * BH_STAR_SYMBOLS * BH_STARFIELD_COLORS
bh_fade_vis:        .skip 4 * BH_STAR_SYMBOLS * BH_STARFIELD_COLORS * BH_FADE_FRAMES
bh_preexisting:     .skip 1

    .text
