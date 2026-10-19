// effects/beams.s - src/effects/beams.rs.
// Groups keep a cursor and remaining count; pending/active arrays hold pointers.

.equ HAVE_beams, 1

.equ BEAMS.final_stops,         0
.equ BEAMS.final_stop_count,    8
.equ BEAMS.final_steps,         16
.equ BEAMS.final_step_count,    24
.equ BEAMS.final_direction,     32
.equ BEAMS.row_symbols,         40
.equ BEAMS.row_count,           48
.equ BEAMS.column_symbols,      56
.equ BEAMS.column_count,        64
.equ BEAMS.delay,               72
.equ BEAMS.row_min,             80
.equ BEAMS.row_max,             88
.equ BEAMS.column_min,          96
.equ BEAMS.column_max,          104
.equ BEAMS.stops,               112
.equ BEAMS.stop_count,          120
.equ BEAMS.steps,               128
.equ BEAMS.step_count,          136
.equ BEAMS.frames,              144
.equ BEAMS.final_frames,        152
.equ BEAMS.wipe_speed,          160
.equ BEAMS_size,                168

.equ BEAM_GROUP.chars,          0
.equ BEAM_GROUP.count,          8
.equ BEAM_GROUP.speed,          16
.equ BEAM_GROUP.counter,        24
.equ BEAM_GROUP.direction,      32
.equ BEAM_GROUP_size,           40

.equ BEAM_ROW,      (NAME_LITERAL + 0)
.equ BEAM_COLUMN,   (NAME_LITERAL + 1)
.equ BEAM_BRIGHTEN, (NAME_LITERAL + 2)
.equ BEAM_ALL,      (FILTER_INPUT | FILTER_INNER_FILL | FILTER_OUTER_FILL)

    .text
// Beams::build. Clobbers C.
beams_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w0, #FILTER_INPUT
    mov     w1, #GROUP_DIAG_TL_TO_BR
    bl      get_characters_grouped
    STX     x9, beams_wipe
    STX     x2, beams_wipe_count
    bl      beams_color_map
    LDX     x9, cfg_existing_colors
    cbz     x9, beams_build.no_cache    // input colors enter the frames
    bl      beams_cache_init
beams_build.no_cache:
    LDX     x19, effect_config
    ldr     x0, [x19, #BEAMS.steps]
    ldr     x3, [x19, #BEAMS.step_count]
    ldr     x1, [x19, #BEAMS.stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, beams_spectrum
    mov     x4, x9
    ldr     x0, [x19, #BEAMS.stops]
    ldr     x1, [x19, #BEAMS.stop_count]
    ldr     x2, [x19, #BEAMS.steps]
    ldr     x3, [x19, #BEAMS.step_count]
    bl      gradient_new
    mov     w9, w9
    STX     x9, beams_spectrum_count
    // At most canvas rows + columns groups.
    LDX     x0, canvas_top
    LDX     x9, canvas_right
    add     x0, x0, x9
    lsl     x0, x0, #3
    bl      alloc
    STX     x9, beams_pending
    LDX     x0, canvas_top
    LDX     x9, canvas_right
    add     x0, x0, x9
    lsl     x0, x0, #3
    bl      alloc
    STX     x9, beams_active
    mov     w20, #0
beams_build.direction:
    mov     w0, #BEAM_ALL
    mov     w1, #GROUP_ROW_TOP_TO_BOTTOM
    cbz     w20, beams_build.group
    mov     w1, #GROUP_COLUMN_L2R
beams_build.group:
    bl      get_characters_grouped
    mov     x21, x9
    mov     x22, x2
    mov     x23, #0
beams_build.make:
    cmp     x23, x22
    b.hs    beams_build.next_direction
    mov     x0, #BEAM_GROUP_size
    bl      alloc
    mov     x24, x9
    add     x9, x21, x23, lsl #4
    ldr     x3, [x9]
    ldr     x2, [x9, #8]
    str     x3, [x24, #BEAM_GROUP.chars]
    str     x2, [x24, #BEAM_GROUP.count]
    str     x20, [x24, #BEAM_GROUP.direction]
    ldr     x0, [x19, #BEAMS.row_min]
    ldr     x1, [x19, #BEAMS.row_max]
    cbz     w20, beams_build.speed
    ldr     x0, [x19, #BEAMS.column_min]
    ldr     x1, [x19, #BEAMS.column_max]
beams_build.speed:
    bl      rng_randint
    scvtf   d0, x9
    LDD     d1, beams_tenth
    fmul    d0, d0, d1
    str     d0, [x24, #BEAM_GROUP.speed]
    mov     x0, #2
    bl      rng_below
    cbnz    w9, beams_build.append
    ldr     x3, [x24, #BEAM_GROUP.chars]
    ldr     x2, [x24, #BEAM_GROUP.count]
    add     x2, x3, x2, lsl #2
    sub     x2, x2, #4
beams_build.reverse:
    cmp     x3, x2
    b.hs    beams_build.append
    ldr     w9, [x3]
    ldr     w1, [x2]
    str     w1, [x3]
    str     w9, [x2]
    add     x3, x3, #4
    sub     x2, x2, #4
    b       beams_build.reverse
beams_build.append:
    LDX     x9, beams_pending
    LDX     x3, beams_pending_count
    str     x24, [x9, x3, lsl #3]
    add     x3, x3, #1
    STX     x3, beams_pending_count
    add     x23, x23, #1
    b       beams_build.make
beams_build.next_direction:
    add     w20, w20, #1
    cmp     w20, #2
    b.lo    beams_build.direction
    // Scene construction consumes no RNG. Each character appears once here.
    mov     w0, #BEAM_ALL
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    mov     x23, #0
beams_build.scenes:
    cmp     x23, x22
    b.hs    beams_build.shuffle
    ldr     w0, [x21, x23, lsl #2]
    bl      beams_scenes
    add     x23, x23, #1
    b       beams_build.scenes
beams_build.shuffle:
    LDX     x0, beams_pending
    LDX     x1, beams_pending_count
    bl      rng_shuffle64
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// Beams::build: one character's beam and brighten scenes. Clobbers C.
beams_scenes:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w21, w0
    LDX     x19, effect_config
    // the character's symbol and colors, which with the (shared) beam
    // gradients determine all three scenes
    LDX     x9, ch_sym
    ldr     x9, [x9, x21, lsl #3]
    STX     x9, beams_symbol
    mov     x22, #0
    mov     x23, #NONE
    LDX     x9, ch_flags
    ldrh    w9, [x9, x21, lsl #1]
    tst     w9, #CF_FILL
    b.ne    beams_scenes.keyed
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    beams_scenes.mapped
    LDX     x9, ch_fg
    ldr     x22, [x9, x21, lsl #3]
    LDX     x9, ch_bg
    ldr     x23, [x9, x21, lsl #3]
    b       beams_scenes.keyed
beams_scenes.mapped:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x21, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, beams_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x21, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, beams_map
    ldr     x22, [x3, x9, lsl #3]
beams_scenes.keyed:
    // a character with the same (symbol, fg, bg) as an earlier one gets
    // clones of that one's scenes
    mov     x24, #0                     // cache entry to fill, 0 = none
    LDX     x1, beams_cache
    cbz     x1, beams_scenes.new_scenes
    LDX     x0, beams_symbol
    mov     x9, x22
    MOV64   x3, 0x9E3779B97F4A7C15
    mul     x9, x9, x3
    eor     x9, x9, x0
    MOV64   x3, 0xBF58476D1CE4E5B9
    mul     x9, x9, x3
    eor     x9, x9, x23
    mul     x9, x9, x3
    LDX     x3, beams_cache_shift
    lsr     x9, x9, x3
beams_scenes.probe:
    add     x2, x1, x9, lsl #5
    ldr     x10, [x2]
    cbz     x10, beams_scenes.miss
    cmp     x10, x0
    b.ne    beams_scenes.probe_next
    ldr     x10, [x2, #8]
    cmp     x10, x22
    b.ne    beams_scenes.probe_next
    ldr     x10, [x2, #16]
    cmp     x10, x23
    b.eq    beams_scenes.hit
beams_scenes.probe_next:
    add     x9, x9, #1
    LDX     x10, beams_cache_mask
    and     x9, x9, x10
    b       beams_scenes.probe
beams_scenes.miss:
    str     x0, [x2]
    str     x22, [x2, #8]
    str     x23, [x2, #16]
    mov     x24, x2
    b       beams_scenes.new_scenes
beams_scenes.hit:
    ldr     w24, [x2, #24]              // the earlier character
    mov     w20, #0
beams_scenes.copy:
    mov     w0, w24
    mov     w1, #BEAM_ROW
    add     w1, w1, w20
    bl      scene_find
    mov     w1, w9
    mov     w0, w21
    mov     w2, #BEAM_ROW
    add     w2, w2, w20
    bl      scene_copy
    add     w20, w20, #1
    cmp     w20, #3
    b.lo    beams_scenes.copy
    b       beams_scenes.out
beams_scenes.new_scenes:
    cbz     x24, beams_scenes.fresh
    str     w21, [x24, #24]
beams_scenes.fresh:
    mov     w20, #0
beams_scenes.new:
    mov     w0, w21
    mov     w1, #BEAM_ROW
    add     w1, w1, w20
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    ADRG    x3, beams_scene_ids
    str     w9, [x3, x20, lsl #2]
    add     w20, w20, #1
    cmp     w20, #3
    b.lo    beams_scenes.new
    mov     w20, #0
beams_scenes.beam:
    ADRG    x9, beams_scene_ids
    ldr     w0, [x9, x20, lsl #2]
    add     x9, x19, x20, lsl #4
    ldr     x1, [x9, #BEAMS.row_symbols]
    ldr     x2, [x9, #BEAMS.row_count]
    ldr     x3, [x19, #BEAMS.frames]
    LDX     x4, beams_spectrum
    LDX     x5, beams_spectrum_count
    mov     x6, #0
    mov     x7, #0
    bl      scene_apply_gradient
    add     w20, w20, #1
    cmp     w20, #2
    b.lo    beams_scenes.beam
beams_scenes.colors:
    STX     xzr, beams_fg_count
    STX     xzr, beams_bg_count
    cmn     x22, #1                     // NONE
    b.eq    beams_scenes.bg
    mov     x0, x22
    ADRG    x1, beams_fg_fade
    ADRG    x2, beams_fg_bright
    bl      beams_fades
    mov     x9, #11
    STX     x9, beams_fg_count
beams_scenes.bg:
    cmn     x23, #1                     // NONE
    b.eq    beams_scenes.fades
    mov     x0, x23
    ADRG    x1, beams_bg_fade
    ADRG    x2, beams_bg_bright
    bl      beams_fades
    mov     x9, #11
    STX     x9, beams_bg_count
beams_scenes.fades:
    mov     w20, #0
beams_scenes.fade:
    ADRG    x9, beams_scene_ids
    ldr     w0, [x9, x20, lsl #2]
    ADRG    x1, beams_symbol
    mov     w2, #1
    mov     w3, #2
    ADRG    x4, beams_fg_fade
    ADRG    x9, beams_bg_fade
    cmp     w20, #2
    b.ne    beams_scenes.apply
    ldr     x3, [x19, #BEAMS.final_frames]
    ADRG    x4, beams_fg_bright
    ADRG    x9, beams_bg_bright
beams_scenes.apply:
    LDX     x5, beams_fg_count
    LDX     x10, beams_bg_count
    cbnz    x10, beams_scenes.has_bg
    mov     x9, #0
beams_scenes.has_bg:
    cbnz    x5, beams_scenes.gradient
    mov     x4, #0
    cbnz    x9, beams_scenes.gradient
    mov     w2, w3
    LDX     x1, beams_symbol
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       beams_scenes.added
beams_scenes.gradient:
    LDX     x7, beams_bg_count
    mov     x6, x9
    bl      scene_apply_gradient
beams_scenes.added:
    add     w20, w20, #1
    cmp     w20, #3
    b.lo    beams_scenes.fade
beams_scenes.out:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// Beams::build: fg/bg fade and brighten gradients (not a reversed spectrum:
// integer interpolation rounds down in each direction). Clobbers C.
// x0=color, x1=fade output, x2=brighten output.
beams_fades:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     x19, x0
    mov     x21, x1
    mov     x22, x2
    LDD     d0, beams_dim
    bl      adjust_color_brightness
    ADRG    x0, beams_pair
    str     x19, [x0]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, beams_ten
    mov     w3, #1
    mov     x4, x21
    bl      gradient_new
    ADRG    x0, beams_pair
    ldr     x9, [x0, #8]
    str     x9, [x0]
    str     x19, [x0, #8]
    mov     w1, #2
    ADRG    x2, beams_ten
    mov     w3, #1
    mov     x4, x22
    bl      gradient_new
    POP2    x22, x30
    POP2    x19, x21
    ret

// Beams::next_frame + Group::get_next_character. Clobbers C.
beams_next_frame:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x9, beams_phase
    cmp     x9, #2
    b.ne    beams_next_frame.phase
    bl      active_empty
    cbnz    w9, beams_next_frame.done
beams_next_frame.phase:
    LDX     x9, beams_phase
    cbnz    x9, beams_next_frame.wipe
    LDX     x9, beams_delay
    cbnz    x9, beams_next_frame.delay
    LDX     x9, beams_pending_count
    cbz     x9, beams_next_frame.reset_delay
    mov     x0, #1
    mov     x1, #5
    bl      rng_randint
    LDX     x3, beams_pending_count
    cmp     x9, x3
    csel    x9, x3, x9, hi
    sub     x3, x3, x9
    STX     x3, beams_pending_count
    LDX     x2, beams_pending
    LDX     x1, beams_active
    LDX     x3, beams_active_count
beams_next_frame.activate:
    ldr     x0, [x2]
    str     x0, [x1, x3, lsl #3]
    add     x3, x3, #1
    add     x2, x2, #8
    subs    x9, x9, #1
    b.ne    beams_next_frame.activate
    STX     x2, beams_pending
    STX     x3, beams_active_count
beams_next_frame.reset_delay:
    LDX     x9, effect_config
    ldr     x9, [x9, #BEAMS.delay]
    STX     x9, beams_delay
    b       beams_next_frame.groups
beams_next_frame.delay:
    LDX     x9, beams_delay
    sub     x9, x9, #1
    STX     x9, beams_delay
beams_next_frame.groups:
    mov     x21, #0
    mov     x22, #0
    LDX     x23, beams_active
beams_next_frame.group:
    LDX     x9, beams_active_count
    cmp     x21, x9
    b.hs    beams_next_frame.pruned
    ldr     x19, [x23, x21, lsl #3]
    ldr     d0, [x19, #BEAM_GROUP.counter]
    ldr     d1, [x19, #BEAM_GROUP.speed]
    fadd    d0, d0, d1
    str     d0, [x19, #BEAM_GROUP.counter]
    bl      f64_to_i64
    mov     x20, x9
    cmp     x20, #1
    b.le    beams_next_frame.keep
    // Empty-group iterations do nothing in Rust.
    ldr     x9, [x19, #BEAM_GROUP.count]
    cmp     x20, x9
    csel    x20, x9, x20, hi
beams_next_frame.character:
    cbz     x20, beams_next_frame.keep
    ldr     d0, [x19, #BEAM_GROUP.counter]
    LDD     d1, beams_one
    fsub    d0, d0, d1
    str     d0, [x19, #BEAM_GROUP.counter]
    ldr     x9, [x19, #BEAM_GROUP.chars]
    ldr     w24, [x9]
    add     x9, x9, #4
    str     x9, [x19, #BEAM_GROUP.chars]
    ldr     x9, [x19, #BEAM_GROUP.count]
    sub     x9, x9, #1
    str     x9, [x19, #BEAM_GROUP.count]
    LDX     x9, ch_scene
    ldr     w0, [x9, x24, lsl #2]
    cmn     w0, #1                      // NONE
    b.eq    beams_next_frame.visible
    bl      scene_reset
    b       beams_next_frame.start_scene
beams_next_frame.visible:
    mov     w0, w24
    bl      set_visible
    mov     w0, w24
    bl      active_insert
beams_next_frame.start_scene:
    mov     w0, w24
    ldr     w1, [x19, #BEAM_GROUP.direction]
    mov     w3, #BEAM_ROW
    add     w1, w1, w3
    bl      scene_activate_name
    sub     x20, x20, #1
    b       beams_next_frame.character
beams_next_frame.keep:
    ldr     x9, [x19, #BEAM_GROUP.count]
    cbz     x9, beams_next_frame.next
    str     x19, [x23, x22, lsl #3]
    add     x22, x22, #1
beams_next_frame.next:
    add     x21, x21, #1
    b       beams_next_frame.group
beams_next_frame.pruned:
    STX     x22, beams_active_count
    LDX     x9, beams_pending_count
    orr     x22, x22, x9
    cbnz    x22, beams_next_frame.tick
    bl      active_empty
    cbz     w9, beams_next_frame.tick
    mov     x9, #1
    STX     x9, beams_phase
    b       beams_next_frame.tick
beams_next_frame.wipe:
    LDX     x9, beams_phase
    cmp     x9, #1
    b.ne    beams_next_frame.tick
    LDX     x9, beams_wipe_count
    cbnz    x9, beams_next_frame.wipe_groups
    mov     x9, #2
    STX     x9, beams_phase
    b       beams_next_frame.tick
beams_next_frame.wipe_groups:
    LDX     x9, effect_config
    ldr     x20, [x9, #BEAMS.wipe_speed]
beams_next_frame.wipe_group:
    cbz     x20, beams_next_frame.tick
    LDX     x9, beams_wipe_count
    cbz     x9, beams_next_frame.tick
    LDX     x9, beams_wipe
    ldr     x21, [x9]
    ldr     x22, [x9, #8]
    add     x9, x9, #16
    STX     x9, beams_wipe
    LDX     x9, beams_wipe_count
    sub     x9, x9, #1
    STX     x9, beams_wipe_count
beams_next_frame.wipe_character:
    ldr     w19, [x21]
    mov     w0, w19
    MOV64   w1, BEAM_BRIGHTEN
    bl      scene_activate_name
    mov     w0, w19
    bl      set_visible
    mov     w0, w19
    bl      active_insert
    add     x21, x21, #4
    subs    x22, x22, #1
    b.ne    beams_next_frame.wipe_character
    sub     x20, x20, #1
    b       beams_next_frame.wipe_group
beams_next_frame.tick:
    bl      update
    mov     w9, #1
    b       beams_next_frame.return
beams_next_frame.done:
    mov     w9, #0
beams_next_frame.return:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// beams_cache_init: the (symbol, fg, bg) -> character table of beams_scenes,
// 32-byte entries (symbol 0 = empty), at least twice the character count.
beams_cache_init:
    PUSH1   x30
    LDW     w9, char_count
    add     x9, x9, x9
    mov     x3, #16
    mov     w2, #0
beams_cache_init.size:
    cmp     x3, x9
    b.hs    beams_cache_init.sized
    add     x3, x3, x3
    add     w2, w2, #1
    b       beams_cache_init.size
beams_cache_init.sized:
    sub     x9, x3, #1
    STX     x9, beams_cache_mask
    add     w2, w2, #4
    mov     w9, #64
    sub     w9, w9, w2
    STX     x9, beams_cache_shift
    lsl     x3, x3, #5
    add     x0, x3, #64
    bl      alloc
    STX     x9, beams_cache
    POP1    x30
    ret

// beams_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
beams_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    // spectrum capacity: sum over pairs of the step counts, plus one per pair
    ldr     x0, [x19, #BEAMS.final_steps]
    ldr     x3, [x19, #BEAMS.final_step_count]
    ldr     x1, [x19, #BEAMS.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, beams_final_spectrum
    ldr     x0, [x19, #BEAMS.final_stops]
    ldr     x1, [x19, #BEAMS.final_stop_count]
    ldr     x2, [x19, #BEAMS.final_steps]
    ldr     x3, [x19, #BEAMS.final_step_count]
    LDX     x4, beams_final_spectrum
    bl      gradient_new
    LDX     x0, beams_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, beams_map_width
    ldr     x6, [x19, #BEAMS.final_direction]
    bl      gradient_map
    STX     x9, beams_map
    POP2    x19, x30
    ret


    .section .rodata
    .balign 8
beams_tenth:    .double 0.1
beams_dim:      .double 0.3
beams_one:      .double 1.0
beams_ten:      .quad 10

    TSTATE
    .balign 8
beams_wipe:             .skip 8
beams_wipe_count:       .skip 8
beams_pending:          .skip 8
beams_pending_count:    .skip 8
beams_active:           .skip 8
beams_active_count:     .skip 8
beams_delay:            .skip 8
beams_phase:            .skip 8
beams_spectrum:         .skip 8
beams_spectrum_count:   .skip 8
beams_final_spectrum:   .skip 8
beams_map:              .skip 8
beams_map_width:        .skip 8
beams_scene_ids:        .skip 4 * 3
    .balign 8
beams_cache:            .skip 8
beams_cache_mask:       .skip 8
beams_cache_shift:      .skip 8
beams_symbol:           .skip 8
beams_pair:             .skip 8 * 2
beams_fg_count:         .skip 8
beams_bg_count:         .skip 8
beams_fg_fade:          .skip 8 * 11
beams_fg_bright:        .skip 8 * 11
beams_bg_fade:          .skip 8 * 11
beams_bg_bright:        .skip 8 * 11

    .text
