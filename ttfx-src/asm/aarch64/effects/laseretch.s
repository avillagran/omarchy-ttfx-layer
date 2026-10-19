// effects/laseretch.s - "A laser etches characters onto the terminal"
// (src/effects/laseretch.rs).
//
// build() makes every character's spawn scene, then (for the "algorithm"
// pattern) runs RecursiveBacktracker from a random text coordinate; its link
// order is the etch order. A character-group pattern etches nothing (the
// upstream dead branch Rust reproduces). The laser follows: the spark pool
// (2000 particles, each reclaimed when its spark scene completes; particles
// the pool creates later never are, as in Rust) and one beam character per
// canvas row up the diagonal from (0, 0).

.equ HAVE_laseretch, 1

.equ LASERETCH.group_pattern,     0     // 1 = a CharacterGroup, 0 = algorithm
.equ LASERETCH.etch_speed,        8
.equ LASERETCH.etch_delay,        16
.equ LASERETCH.cool_stops,        24    // *const u64
.equ LASERETCH.cool_stop_count,   32
.equ LASERETCH.laser_stops,       40
.equ LASERETCH.laser_stop_count,  48
.equ LASERETCH.spark_stops,       56
.equ LASERETCH.spark_stop_count,  64
.equ LASERETCH.spark_cooling,     72
.equ LASERETCH.final_stops,       80
.equ LASERETCH.final_stop_count,  88
.equ LASERETCH.final_steps,       96    // *const i64
.equ LASERETCH.final_step_count,  104
.equ LASERETCH.final_direction,   112
.equ LASERETCH_size,              120

// scene names
.equ LE_SPAWN,              NAME_LITERAL + 0
.equ LE_SPARK,              NAME_LITERAL + 1
.equ LE_LASER,              NAME_LITERAL + 2

.equ LE_OUT_SINE,           2
.equ LE_COOL_MAX,           64          // cool/cooldown spectrum entries

    .text

// laseretch_build: LaserEtchIterator.build + __init__'s tail.
laseretch_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    bl      le_final_map
    // cool gradient stops: the cool stops, then the mapped color
    ldr     x9, [x19, #LASERETCH.cool_stop_count]
    add     x9, x9, #1
    STX     x9, le_cool_stop_count
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, le_cool_stops
    mov     x0, x9
    ldr     x1, [x19, #LASERETCH.cool_stops]
    ldr     x3, [x19, #LASERETCH.cool_stop_count]
    REP_MOVSQ
    ADRG    x0, le_eight
    mov     w3, #1
    LDX     x1, le_cool_stop_count
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, le_cool
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x23, x9
    mov     x24, x2
    mov     x22, #0
laseretch_build.character:
    cmp     x22, x24
    b.hs    laseretch_build.etch_order
    ldr     w0, [x23, x22, lsl #2]
    bl      le_character
    add     x22, x22, #1
    b       laseretch_build.character
laseretch_build.etch_order:
    ldr     x9, [x19, #LASERETCH.group_pattern]
    cbnz    x9, laseretch_build.laser
    mov     w0, #1
    bl      rb_new
    bl      rb_run
    STX     x9, le_pending
    STX     x2, le_pending_count
laseretch_build.laser:
    STX     xzr, le_pending_head
    STX     xzr, le_delay
    bl      le_make_laser
    mov     x21, #0
laseretch_build.beam:
    LDX     x9, le_beam_count
    cmp     x21, x9
    b.hs    laseretch_build.built
    LDX     x9, le_beam
    ldr     w0, [x9, x21, lsl #2]
    bl      active_insert
    add     x21, x21, #1
    b       laseretch_build.beam
laseretch_build.built:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// le_character(w0=slot): the spawn scene ("^", the cool gradient, and the
// dynamic tail), activated.
le_character:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w19, w0
    LDX     x20, effect_config
    // final colors: x21 fg, x22 bg
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    le_character.mapped
    LDX     x9, ch_fg
    ldr     x21, [x9, x19, lsl #3]
    LDX     x9, ch_bg
    ldr     x22, [x9, x19, lsl #3]
    // cool = Gradient(cool stops, steps=8)
    ldr     x0, [x20, #LASERETCH.cool_stops]
    ldr     x1, [x20, #LASERETCH.cool_stop_count]
    b       le_character.cool
le_character.mapped:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x19, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, le_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x19, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, le_map
    ldr     x21, [x3, x9, lsl #3]
    mov     x22, #NONE
    LDX     x0, le_cool_stops
    LDX     x1, le_cool_stop_count
    add     x9, x0, x1, lsl #3
    stur    x21, [x9, #-8]
le_character.cool:
    ADRG    x2, le_eight
    mov     w3, #1
    LDX     x4, le_cool
    bl      gradient_new
    mov     w23, w9                     // cool length
    mov     w0, w19
    MOV64   w1, LE_SPAWN
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9
    mov     w0, w24
    MOV64   x1, ((1 << 32) | '^')
    mov     w2, #3
    MOV64   x3, 0xffe680
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    mov     w20, #0
le_character.cool_frame:
    cmp     w20, w23
    b.hs    le_character.tail
    mov     w0, w24
    LDX     x1, ch_sym
    ldr     x1, [x1, x19, lsl #3]
    mov     w2, #3
    LDX     x3, le_cool
    ldr     x3, [x3, x20, lsl #3]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    add     w20, w20, #1
    b       le_character.cool_frame
le_character.tail:
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    le_character.activate
    LDX     x9, le_cool
    add     x9, x9, x23, lsl #3
    ldur    x9, [x9, #-8]
    STX     x9, le_pair_stops           // the cool gradient's last color
    and     x9, x21, x22
    cmn     x9, #1                      // NONE
    b.eq    le_character.white
    // fg / bg gradients from the cool end to the input colors
    mov     w23, #0                     // fg length
    mov     w20, #0                     // bg length
    cmn     x21, #1                     // NONE
    b.eq    le_character.tail_bg
    ADRG    x0, le_pair_stops
    str     x21, [x0, #8]
    mov     w1, #2
    ADRG    x2, le_eight
    mov     w3, #1
    ADRG    x4, le_fg_spectrum
    bl      gradient_new
    mov     w23, w9
le_character.tail_bg:
    cmn     x22, #1                     // NONE
    b.eq    le_character.tail_apply
    ADRG    x0, le_pair_stops
    str     x22, [x0, #8]
    mov     w1, #2
    ADRG    x2, le_eight
    mov     w3, #1
    ADRG    x4, le_bg_spectrum
    bl      gradient_new
    mov     w20, w9
le_character.tail_apply:
    mov     x4, #0
    cbz     w23, le_character.no_fg
    ADRG    x4, le_fg_spectrum
le_character.no_fg:
    mov     x6, #0
    cbz     w20, le_character.no_bg
    ADRG    x6, le_bg_spectrum
le_character.no_bg:
    mov     x7, x20
    mov     w0, w24
    LDX     x1, ch_sym
    add     x1, x1, x19, lsl #3
    mov     w2, #1
    mov     w3, #3
    mov     w5, w23
    bl      scene_apply_gradient
    b       le_character.activate
le_character.white:
    // no input colors: cool end -> white, then a colorless frame
    ADRG    x0, le_pair_stops
    mov     x9, #0xffffff
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, le_eight
    mov     w3, #1
    ADRG    x4, le_fg_spectrum
    bl      gradient_new
    mov     x6, #0
    mov     x7, #0
    mov     w0, w24
    LDX     x1, ch_sym
    add     x1, x1, x19, lsl #3
    mov     w2, #1
    mov     w3, #3
    ADRG    x4, le_fg_spectrum
    mov     w5, w9
    bl      scene_apply_gradient
    mov     w0, w24
    LDX     x1, ch_sym
    ldr     x1, [x1, x19, lsl #3]
    mov     w2, #3
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
le_character.activate:
    mov     w0, w19
    mov     w1, w24
    bl      scene_activate
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// le_final_map: Gradient::new(final stops, final steps) mapped over the text
// rectangle.
le_final_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #LASERETCH.final_steps]
    ldr     x3, [x19, #LASERETCH.final_step_count]
    ldr     x1, [x19, #LASERETCH.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, le_final_spectrum
    ldr     x0, [x19, #LASERETCH.final_stops]
    ldr     x1, [x19, #LASERETCH.final_stop_count]
    ldr     x2, [x19, #LASERETCH.final_steps]
    ldr     x3, [x19, #LASERETCH.final_step_count]
    LDX     x4, le_final_spectrum
    bl      gradient_new
    LDX     x0, le_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, le_map_width
    ldr     x6, [x19, #LASERETCH.final_direction]
    bl      gradient_map
    STX     x9, le_map
    POP2    x19, x30
    ret

// ------------------------------------------------------------ the laser

// le_make_laser: Laser.__init__ + _make_sparks_pool.
le_make_laser:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    // the looped laser gradient: the stops plus the first again, 6 steps
    ldr     x9, [x19, #LASERETCH.laser_stop_count]
    lsl     x0, x9, #3
    add     x0, x0, #8
    bl      alloc
    mov     x21, x9
    mov     x0, x9
    ldr     x1, [x19, #LASERETCH.laser_stops]
    ldr     x3, [x19, #LASERETCH.laser_stop_count]
    REP_MOVSQ
    ldr     x9, [x19, #LASERETCH.laser_stops]
    ldr     x9, [x9]
    str     x9, [x0]
    ldr     x22, [x19, #LASERETCH.laser_stop_count]
    cmp     x22, #1
    b.eq    le_make_laser.one_stop      // a single stop is not looped
    add     x22, x22, #1
le_make_laser.one_stop:
    ADRG    x0, le_six
    mov     w3, #1
    mov     x1, x22
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, le_laser
    mov     x0, x21
    mov     x1, x22
    ADRG    x2, le_six
    mov     w3, #1
    LDX     x4, le_laser
    bl      gradient_new
    STX     x9, le_laser_len
    // the spark gradient, steps (3, 8)
    ADRG    x0, le_three_eight
    mov     w3, #2
    ldr     x1, [x19, #LASERETCH.spark_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, le_spark
    ldr     x0, [x19, #LASERETCH.spark_stops]
    ldr     x1, [x19, #LASERETCH.spark_stop_count]
    ADRG    x2, le_three_eight
    mov     w3, #2
    LDX     x4, le_spark
    bl      gradient_new
    STX     x9, le_spark_len
    // the spark pool: unbounded, 2000 preallocated, reclaimed on "spark"
    ADRG    x9, le_spark_symbols
    MOV64   x3, ((1 << 32) | '.')
    str     x3, [x9]
    MOV64   x3, ((1 << 32) | ',')
    str     x3, [x9, #8]
    MOV64   x3, ((1 << 32) | '*')
    str     x3, [x9, #16]
    ADRG    x0, le_pool
    ADRG    x1, le_spark_symbols
    mov     w2, #3
    mov     x3, #-1
    mov     x4, #0
    bl      pool_init
    ADRG    x9, le_init_spark
    ADRG    x3, le_pool
    str     x9, [x3, #POOL.initializer]
    ADRG    x0, le_pool
    mov     w1, #2000
    bl      pool_preallocate
    mov     x21, #0
le_make_laser.reclaim:
    ADRG    x9, le_pool
    ldr     x10, [x9, #POOL.particle_count]
    cmp     x21, x10
    b.hs    le_make_laser.beam
    ldr     x9, [x9, #POOL.particles]
    ldr     w0, [x9, x21, lsl #2]
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, LE_SPARK
    mov     w4, #ACT_CALLBACK
    ADRG    x5, le_reclaim
    mov     x6, #0
    bl      event_register
    add     x21, x21, #1
    b       le_make_laser.reclaim
le_make_laser.beam:
    // one beam character per row 0..=canvas.top up the diagonal
    LDX     x0, canvas_top
    lsl     x0, x0, #2
    add     x0, x0, #68
    bl      alloc
    STX     x9, le_beam
    mov     x21, #0                     // row = column
le_make_laser.beam_char:
    LDX     x9, canvas_top
    cmp     x21, x9
    b.gt    le_make_laser.done
    MOV64   x0, ((1 << 32) | '/')
    MOV64   x9, ((1 << 32) | '*')
    cmp     x21, #0
    csel    x0, x9, x0, eq
    lsl     x1, x21, #32
    mov     w9, w21
    orr     x1, x1, x9
    bl      add_character
    mov     w19, w9
    LDX     x3, le_beam
    str     w9, [x3, x21, lsl #2]
    LDX     x9, le_beam_count
    add     x9, x9, #1
    STX     x9, le_beam_count
    mov     w0, w19
    mov     x1, #2
    bl      set_layer
    mov     w0, w19
    bl      set_visible
    // the looping laser scene, its gradient rotated left once per character
    mov     w0, w19
    MOV64   w1, LE_LASER
    mov     w2, #SCF_LOOPING
    mov     w3, #NONE
    bl      scene_new
    mov     w22, w9
    mov     x23, #0
le_make_laser.laser_frame:
    LDX     x10, le_laser_len
    cmp     x23, x10
    b.hs    le_make_laser.laser_done
    add     x9, x21, x23
    udiv    x2, x9, x10
    msub    x2, x2, x10, x9
    LDX     x3, le_laser
    ldr     x3, [x3, x2, lsl #3]
    mov     w0, w22
    LDX     x1, ch_sym
    ldr     x1, [x1, x19, lsl #3]
    mov     w2, #3
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    add     x23, x23, #1
    b       le_make_laser.laser_frame
le_make_laser.laser_done:
    mov     w0, w19
    mov     w1, w22
    bl      scene_activate
    add     x21, x21, #1
    b       le_make_laser.beam_char
le_make_laser.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// le_init_spark(w0=slot): initialize_spark - layer 2 and the "spark"
// cooling scene.
le_init_spark:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     w19, w0
    mov     x1, #2
    bl      set_layer
    mov     w0, w19
    MOV64   w1, LE_SPARK
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w21, w9
    // the cooling gradient over the symbol: three symbols, one spectrum
    LDX     x0, ch_sym
    ldr     x0, [x0, x19, lsl #3]
    LDX     x1, le_spark
    LDX     x2, le_spark_len
    mov     x3, #NONE
    mov     x4, #0
    bl      visual_run
    mov     w0, w21
    mov     x1, x9
    LDX     x3, effect_config
    ldr     x3, [x3, #LASERETCH.spark_cooling]
    bl      visual_frames
le_init_spark.done:
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// le_reclaim(w0=spark): sparks_pool.reclaim(spark, hide=True,
// deactivate=True).
le_reclaim:
    mov     w1, w0
    ADRG    x0, le_pool
    mov     w2, #1
    mov     w3, #1
    b       pool_reclaim

// le_reposition(x0=target coord): Laser.reposition - the beam up the
// diagonal from the target, then one spark.
le_reposition:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    STX     x0, le_position
    mov     x21, x0
    mov     x19, #0
le_reposition.move:
    LDX     x9, le_beam_count
    cmp     x19, x9
    b.hs    le_reposition.spark
    LDX     x9, le_beam
    ldr     w0, [x9, x19, lsl #2]
    mov     x1, x21
    bl      set_coordinate
    MOV64   x9, ((1 << 32) | 1)
    add     x21, x21, x9                // row + 1, column + 1 (the column
    add     x19, x19, #1                // is positive: no carry)
    b       le_reposition.move
le_reposition.spark:
    // emit_sparks(1)
    ADRG    x0, le_pool
    LDX     x1, le_position
    mov     x2, #0
    mov     w3, #1
    ADRG    x4, le_setup_spark
    mov     x5, #0
    bl      pool_emit
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// le_setup_spark(w0=spark): setup_spark_path - an out_sine bezier fall to
// the canvas bottom from the laser position, and the spark scene.
le_setup_spark:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x30, [sp, #16]         // [sp, #32]: the control coordinate
    mov     w19, w0
    LDX     x1, le_position
    bl      set_coordinate
    LDD     d0, le_spark_speed
    mov     w0, w19
    mov     w1, #LE_OUT_SINE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w22, w9
    LDSW    x0, le_position
    add     x1, x0, #20
    sub     x0, x0, #20
    bl      rng_randint
    mov     w21, w9                     // fall column
    mov     x0, #-10
    mov     x1, #20
    bl      rng_randint
    LDX     x3, le_position
    asr     x3, x3, #32
    add     x9, x9, x3
    lsl     x9, x9, #32
    orr     x9, x9, x21
    str     x9, [sp, #32]               // control (fall column, row + offset)
    mov     x1, #(1 << 32)              // canvas.bottom
    orr     x1, x1, x21
    mov     w0, w22
    add     x2, sp, #32
    mov     w3, #1
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w19
    mov     w1, w22
    bl      path_activate
    mov     w0, w19
    MOV64   w1, LE_SPARK
    bl      scene_activate_name
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// le_pop -> w9 = the next pending character, or Z set when none.
le_pop:
    LDX     x9, le_pending_head
    LDX     x3, le_pending_count
    cmp     x9, x3
    b.hs    le_pop.none
    add     x3, x9, #1
    STX     x3, le_pending_head
    LDX     x3, le_pending
    ldr     w9, [x3, x9, lsl #2]
    cmp     x3, #0                      // Z clear (a non-null pointer)
    ret
le_pop.none:
    cmp     x3, x3                      // Z set
    ret

// laseretch_next_frame -> w9 = 1 for a frame, 0 when done.
laseretch_next_frame:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    LDX     x9, le_pending_head
    LDX     x10, le_pending_count
    cmp     x9, x10
    b.lo    laseretch_next_frame.frame
    bl      active_empty
    cbnz    w9, laseretch_next_frame.finished
laseretch_next_frame.frame:
    LDX     x9, le_delay
    cbnz    x9, laseretch_next_frame.wait
    LDX     x9, effect_config
    ldr     x21, [x9, #LASERETCH.etch_speed]
laseretch_next_frame.etch:
    cbz     x21, laseretch_next_frame.etched
    sub     x21, x21, #1
    bl      le_pop
    b.eq    laseretch_next_frame.etched
    mov     w19, w9
laseretch_next_frame.skip_blank:
    // spaces without input colors are passed over
    LDX     x9, ch_sym
    ldr     x9, [x9, x19, lsl #3]
    MOV64   x3, ((1 << 32) | ' ')
    cmp     x9, x3
    b.ne    laseretch_next_frame.etch_char
    LDX     x9, ch_fg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.ne    laseretch_next_frame.etch_char
    LDX     x9, ch_bg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.ne    laseretch_next_frame.etch_char
    bl      le_pop
    b.eq    laseretch_next_frame.etch_char
    mov     w19, w9
    b       laseretch_next_frame.skip_blank
laseretch_next_frame.etch_char:
    mov     w0, w19
    bl      set_visible
    mov     w0, w19
    bl      active_insert
    mov     w0, w19
    bl      char_input_coord
    mov     x0, x9
    bl      le_reposition
    b       laseretch_next_frame.etch
laseretch_next_frame.etched:
    LDX     x9, effect_config
    ldr     x9, [x9, #LASERETCH.etch_delay]
    STX     x9, le_delay
    b       laseretch_next_frame.beam
laseretch_next_frame.wait:
    sub     x9, x9, #1
    STX     x9, le_delay
laseretch_next_frame.beam:
    mov     x19, #0
    LDX     x9, le_pending_head
    LDX     x10, le_pending_count
    cmp     x9, x10
    b.hs    laseretch_next_frame.disable
laseretch_next_frame.keep:
    LDX     x9, le_beam_count
    cmp     x19, x9
    b.hs    laseretch_next_frame.tick
    LDX     x9, le_beam
    ldr     w0, [x9, x19, lsl #2]
    bl      active_insert
    add     x19, x19, #1
    b       laseretch_next_frame.keep
laseretch_next_frame.disable:
    LDX     x9, le_beam_count
    cmp     x19, x9
    b.hs    laseretch_next_frame.tick
    LDX     x9, le_beam
    ldr     w0, [x9, x19, lsl #2]
    mov     w1, #0
    bl      set_visibility
    add     x19, x19, #1
    b       laseretch_next_frame.disable
laseretch_next_frame.tick:
    bl      update
    mov     w9, #1
    b       laseretch_next_frame.out
laseretch_next_frame.finished:
    mov     w9, #0
laseretch_next_frame.out:
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

    .section .rodata
    .balign 8
le_spark_speed:     .double 0.3
le_six:             .quad 6
le_eight:           .quad 8
le_three_eight:     .quad 3, 8

    TSTATE
    .balign 8
le_pool:            .skip POOL_size
    .balign 8
le_spark_symbols:   .skip 8 * 3
le_pair_stops:      .skip 8 * 2
le_fg_spectrum:     .skip 8 * 16
le_bg_spectrum:     .skip 8 * 16
le_cool_stops:      .skip 8
le_cool_stop_count: .skip 8
le_cool:            .skip 8
le_final_spectrum:  .skip 8
le_map:             .skip 8
le_map_width:       .skip 8
le_laser:           .skip 8
le_laser_len:       .skip 8
le_spark:           .skip 8
le_spark_len:       .skip 8
le_beam:            .skip 8
le_beam_count:      .skip 8
le_pending:         .skip 8
le_pending_count:   .skip 8
le_pending_head:    .skip 8
le_delay:           .skip 8
le_position:        .skip 8

    .text
