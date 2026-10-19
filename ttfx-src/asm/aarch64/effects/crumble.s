// effects/crumble.s - "Characters lose color and crumble into dust,
// vacuumed up, and reformed" (src/effects/crumble.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Crumble):
//
// Every character gets, in Rust's order: an initial weak scene (auto), the
// fall path (auto), "weaken", the "top" and "input" paths, the strengthen
// flash and strengthen scenes (auto) and the distance-synced dust scene
// (auto, five rng.choice draws), then its five events. Characters sharing
// their (fg, bg) pair share the derived colors and gradients.

.equ HAVE_crumble, 1

.equ CRUMBLE.final_stops,       0       // *const u64
.equ CRUMBLE.final_stop_count,  8
.equ CRUMBLE.final_steps,       16      // *const i64
.equ CRUMBLE.final_step_count,  24
.equ CRUMBLE.final_direction,   32
.equ CRUMBLE_size,              40

// scene names
.equ CR_S_WEAKEN,               NAME_LITERAL + 0
// path names
.equ CR_P_TOP,                  NAME_LITERAL + 0
.equ CR_P_INPUT,                NAME_LITERAL + 1

.equ CR_EASE_OUT_QUINT,         14
.equ CR_EASE_OUT_BOUNCE,        29

// stages
.equ CR_FALLING,                0
.equ CR_VACUUMING,              1
.equ CR_RESETTING,              2
.equ CR_COMPLETE,               3

.equ CR_NEUTRAL_GRAY,           0x808080
.equ CR_WHITE,                  0xffffff

    .text

// crumble_build: Crumble::build.
crumble_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    bl      cr_final_color_map
    // dust symbols: * . ,
    mov     w0, #'*'
    bl      utf8_pack
    ADRG    x3, cr_dust_symbols
    str     x9, [x3]
    mov     w0, #'.'
    bl      utf8_pack
    ADRG    x3, cr_dust_symbols
    str     x9, [x3, #8]
    mov     w0, #','
    bl      utf8_pack
    ADRG    x3, cr_dust_symbols
    str     x9, [x3, #16]
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x22, x9
    mov     x23, x2
    STX     x9, cr_pending              // pending_chars: the same list
    STX     x2, cr_pending_count
    mov     x19, #0
crumble_build.char:
    cmp     x19, x23
    b.hs    crumble_build.built
    ldr     w21, [x22, x19, lsl #2]
    // (fg, bg): the input colors under dynamic, else (final color, none)
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    crumble_build.mapped
    LDX     x9, ch_fg
    ldr     x0, [x9, x21, lsl #3]
    LDX     x9, ch_bg
    ldr     x1, [x9, x21, lsl #3]
    b       crumble_build.colors
crumble_build.mapped:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x21, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, cr_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x21, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, cr_map
    ldr     x0, [x3, x9, lsl #3]
    mov     x1, #NONE
crumble_build.colors:
    LDB     w9, cr_key_valid
    cbz     w9, crumble_build.derive
    LDX     x9, cr_key_fg
    cmp     x0, x9
    b.ne    crumble_build.derive
    LDX     x9, cr_key_bg
    cmp     x1, x9
    b.eq    crumble_build.visible
crumble_build.derive:
    bl      cr_derive
crumble_build.visible:
    LDX     x9, ch_sym
    ldr     x9, [x9, x21, lsl #3]
    STX     x9, cr_symbol
    mov     w0, w21
    bl      set_visible
    // initial scene: the input symbol in the weak colors
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    mov     w0, w9
    LDX     x1, cr_symbol
    mov     w2, #1
    LDX     x3, cr_weak_fg
    LDX     x4, cr_weak_bg
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w21
    mov     w1, w20
    bl      scene_activate
    // fall path: to (column, canvas.bottom)
    mov     w0, w21
    LDD     d0, cr_fall_speed
    mov     w1, #CR_EASE_OUT_BOUNCE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w20, w9
    mov     x1, #(1 << 32)              // canvas.bottom = 1
    LDX     x9, ch_icol
    ldr     w9, [x9, x21, lsl #2]
    orr     x1, x1, x9
    mov     w0, w20
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    PATH_PTR x9, x20
    ldr     w9, [x9, #PA_NAME]
    STW     w9, cr_fall_name
    // weaken
    mov     w0, w21
    MOV64   w1, CR_S_WEAKEN
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w0, w9
    ADRG    x3, cr_weaken_fg
    ADRG    x2, cr_weaken_bg
    bl      cr_apply
    // top path: to (column, canvas.top) through the canvas center
    mov     w0, w21
    LDD     d0, cr_one
    mov     w1, #CR_EASE_OUT_QUINT
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, CR_P_TOP
    bl      path_new
    mov     w20, w9
    LDX     x9, center_row
    LDW     w3, center_col
    orr     x9, x3, x9, lsl #32
    STX     x9, cr_control
    LDX     x1, canvas_top
    LDX     x9, ch_icol
    ldr     w9, [x9, x21, lsl #2]
    orr     x1, x9, x1, lsl #32
    mov     w0, w20
    ADRG    x2, cr_control
    mov     w3, #1
    mov     w4, #AUTO
    bl      path_new_waypoint
    // input path: back to the input coordinate
    mov     w0, w21
    LDD     d0, cr_one
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, CR_P_INPUT
    bl      path_new
    mov     w20, w9
    LDX     x1, ch_irow
    ldr     w1, [x1, x21, lsl #2]
    LDX     x9, ch_icol
    ldr     w9, [x9, x21, lsl #2]
    orr     x1, x9, x1, lsl #32
    mov     w0, w20
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // strengthen flash
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    mov     w0, w9
    ADRG    x3, cr_flash_fg
    ADRG    x2, cr_flash_bg
    bl      cr_apply
    SCENE_PTR x9, x20
    MOV64   x12, SC_NAME
    ldr     w9, [x9, x12]
    STW     w9, cr_flash_name
    // strengthen: no colors at all (dynamic only) is one plain frame
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    mov     w0, w9
    LDX     x9, cr_key_fg
    cmn     x9, #1                      // NONE
    b.ne    crumble_build.strengthen_gradient
    LDX     x9, cr_key_bg
    cmn     x9, #1
    b.ne    crumble_build.strengthen_gradient
    LDX     x1, cr_symbol
    mov     w2, #4
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       crumble_build.strengthen_named
crumble_build.strengthen_gradient:
    ADRG    x3, cr_strengthen_fg
    ADRG    x2, cr_strengthen_bg
    bl      cr_apply
crumble_build.strengthen_named:
    SCENE_PTR x9, x20
    MOV64   x12, SC_NAME
    ldr     w9, [x9, x12]
    STW     w9, cr_strengthen_name
    // dust: five random dust symbols, synced to the fall's distance
    mov     w0, w21
    mov     w1, #AUTO
    mov     w2, #SCF_SYNC_DISTANCE
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    mov     w24, #0
crumble_build.dust:
    mov     x0, #3
    bl      rng_below
    ADRG    x3, cr_dust_symbols
    ldr     x1, [x3, x9, lsl #3]
    mov     w0, w20
    mov     w2, #1
    LDX     x3, cr_dust_fg
    LDX     x4, cr_dust_bg
    mov     w5, #0
    bl      scene_add_frame
    add     w24, w24, #1
    cmp     w24, #5
    b.lo    crumble_build.dust
    SCENE_PTR x9, x20
    MOV64   x12, SC_NAME
    ldr     w20, [x9, x12]              // dust name
    // events
    mov     w0, w21
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, CR_S_WEAKEN
    mov     w4, #ACT_ACTIVATE_PATH
    LDW     w5, cr_fall_name
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, CR_S_WEAKEN
    mov     w4, #ACT_SET_LAYER
    mov     w5, #1
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, CR_S_WEAKEN
    mov     w4, #ACT_ACTIVATE_SCENE
    mov     w5, w20
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    MOV64   w3, CR_P_INPUT
    mov     w4, #ACT_ACTIVATE_SCENE
    LDW     w5, cr_flash_name
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    LDW     w3, cr_flash_name
    mov     w4, #ACT_ACTIVATE_SCENE
    LDW     w5, cr_strengthen_name
    mov     x6, #0
    bl      event_register
    add     x19, x19, #1
    b       crumble_build.char
crumble_build.built:
    LDX     x0, cr_pending
    LDX     x1, cr_pending_count
    bl      rng_shuffle32
    mov     x9, #12
    STX     x9, cr_fall_delay
    STX     x9, cr_max_fall_delay
    mov     x9, #9
    STX     x9, cr_min_fall_delay
    STB     wzr, cr_reset
    mov     x9, #1
    STX     x9, cr_group_maxsize
    mov     x9, #CR_FALLING
    STX     x9, cr_stage
    // unvacuumed_chars: input_characters, shuffled
    LDX     x0, input_count
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, cr_unvacuumed
    LDX     x3, input_count
    STX     x3, cr_unvacuumed_count
    LDX     x1, input_chars
    mov     x2, #0
crumble_build.copy:
    cmp     x2, x3
    b.hs    crumble_build.shuffle
    ldr     w0, [x1, x2, lsl #2]
    str     w0, [x9, x2, lsl #2]
    add     x2, x2, #1
    b       crumble_build.copy
crumble_build.shuffle:
    mov     x0, x9
    mov     x1, x3
    bl      rng_shuffle32
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// cr_apply(w0=scene, x3=fg gradient record, x2=bg gradient record):
// apply_gradient_to_symbols([input symbol], 4, fg, bg). A record is
// (length, spectrum[16]); length 0 is None. Clobbers C.
cr_apply:
    mov     x6, #0
    ldr     x7, [x2]
    cbz     x7, cr_apply.bg_none
    add     x6, x2, #8
cr_apply.bg_none:
    mov     x4, #0
    ldr     x5, [x3]
    cbz     x5, cr_apply.fg_none
    add     x4, x3, #8
cr_apply.fg_none:
    ADRG    x1, cr_symbol
    mov     x2, #1
    mov     x3, #4
    b       scene_apply_gradient        // tail call

// cr_derive(x0=fg or NONE, x1=bg or NONE): the weak and dust colors and
// the weaken, strengthen flash and strengthen gradients of Crumble::build.
// Without dynamic handling fg is the final color and bg is NONE; with it and
// no input colors at all, the neutral gray stands in for fg where Rust uses
// it. Records the pair as the cache key. Clobbers C.
cr_derive:
    PUSH2   x19, x20
    PUSH2   x21, x30
    STX     x0, cr_key_fg
    STX     x1, cr_key_bg
    mov     w9, #1
    STB     w9, cr_key_valid
    mov     x19, x0                     // g
    mov     x20, x1                     // bg
    cmn     x19, #1                     // NONE
    b.ne    cr_derive.g
    cmn     x20, #1
    b.ne    cr_derive.g
    MOV64   w19, CR_NEUTRAL_GRAY
cr_derive.g:
    // weak / dust colors
    mov     x9, #NONE
    STX     x9, cr_weak_fg
    STX     x9, cr_weak_bg
    STX     x9, cr_dust_fg
    STX     x9, cr_dust_bg
    cmn     x19, #1
    b.eq    cr_derive.bg_colors
    mov     x0, x19
    LDD     d0, cr_weak_brightness
    bl      adjust_color_brightness
    STX     x9, cr_weak_fg
    mov     x0, x19
    LDD     d0, cr_dust_brightness
    bl      adjust_color_brightness
    STX     x9, cr_dust_fg
cr_derive.bg_colors:
    cmn     x20, #1
    b.eq    cr_derive.gradients
    mov     x0, x20
    LDD     d0, cr_weak_brightness
    bl      adjust_color_brightness
    STX     x9, cr_weak_bg
    mov     x0, x20
    LDD     d0, cr_dust_brightness
    bl      adjust_color_brightness
    STX     x9, cr_dust_bg
cr_derive.gradients:
    // weaken: weak -> dust in 9 steps, per channel present
    LDX     x0, cr_weak_fg
    LDX     x1, cr_dust_fg
    ADRG    x4, cr_weaken_fg
    mov     w21, #9
    bl      cr_pair
    LDX     x0, cr_weak_bg
    LDX     x1, cr_dust_bg
    ADRG    x4, cr_weaken_bg
    bl      cr_pair
    // strengthen flash: color -> white in 6 steps
    mov     x0, x19
    mov     x1, #CR_WHITE
    ADRG    x4, cr_flash_fg
    mov     w21, #6
    bl      cr_pair
    mov     x0, x20
    mov     x1, #CR_WHITE
    ADRG    x4, cr_flash_bg
    bl      cr_pair
    // strengthen: white -> color in 9 steps (the real fg, not the gray)
    LDX     x1, cr_key_fg
    mov     x0, #CR_WHITE
    cmn     x1, #1
    b.ne    cr_derive.strengthen_fg
    mov     x0, x1
cr_derive.strengthen_fg:
    ADRG    x4, cr_strengthen_fg
    mov     w21, #9
    bl      cr_pair
    mov     x1, x20
    mov     x0, #CR_WHITE
    cmn     x1, #1
    b.ne    cr_derive.strengthen_bg
    mov     x0, x1
cr_derive.strengthen_bg:
    ADRG    x4, cr_strengthen_bg
    bl      cr_pair
    POP2    x21, x30
    POP2    x19, x20
    ret

// cr_pair(x0=from or NONE, x1=to or NONE, x21=steps, x4=record):
// Gradient::with_steps([from, to], steps) into the record, or length 0
// (None) when either color is absent. Clobbers C except x21.
cr_pair:
    str     xzr, [x4]
    cmn     x0, #1                      // NONE
    b.eq    cr_pair.none
    cmn     x1, #1
    b.eq    cr_pair.none
    PUSH2   x4, x30
    ADRG    x9, cr_stops
    stp     x0, x1, [x9]
    ADRG    x2, cr_steps
    str     x21, [x2]
    mov     x0, x9
    mov     w1, #2
    mov     w3, #1
    add     x4, x4, #8
    bl      gradient_new
    POP2    x4, x30
    mov     w9, w9
    str     x9, [x4]
cr_pair.none:
    ret

// cr_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
cr_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #CRUMBLE.final_steps]
    ldr     x3, [x19, #CRUMBLE.final_step_count]
    ldr     x1, [x19, #CRUMBLE.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, cr_spectrum
    ldr     x0, [x19, #CRUMBLE.final_stops]
    ldr     x1, [x19, #CRUMBLE.final_stop_count]
    ldr     x2, [x19, #CRUMBLE.final_steps]
    ldr     x3, [x19, #CRUMBLE.final_step_count]
    LDX     x4, cr_spectrum
    bl      gradient_new
    LDX     x0, cr_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, cr_map_width
    ldr     x6, [x19, #CRUMBLE.final_direction]
    bl      gradient_map
    STX     x9, cr_map
    POP2    x19, x30
    ret

// crumble_next_frame -> w9 = 1 for a frame, 0 when done. Crumble::next_frame.
crumble_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x9, cr_stage
    cmp     x9, #CR_COMPLETE
    b.eq    crumble_next_frame.finished
    cmp     x9, #CR_VACUUMING
    b.eq    crumble_next_frame.vacuuming
    cmp     x9, #CR_RESETTING
    b.eq    crumble_next_frame.resetting
    // --- falling
    LDX     x9, cr_pending_pos
    LDX     x10, cr_pending_count
    cmp     x9, x10
    b.hs    crumble_next_frame.fall_check
    LDX     x9, cr_fall_delay
    cbz     x9, crumble_next_frame.fall_group
    sub     x9, x9, #1
    STX     x9, cr_fall_delay
    b       crumble_next_frame.fall_check
crumble_next_frame.fall_group:
    mov     x0, #1
    LDX     x1, cr_group_maxsize
    bl      rng_randint
    mov     x19, x9
crumble_next_frame.fall_one:
    cmp     x19, #0
    b.le    crumble_next_frame.fall_delay
    sub     x19, x19, #1
    LDX     x9, cr_pending_pos
    LDX     x10, cr_pending_count
    cmp     x9, x10
    b.hs    crumble_next_frame.fall_one
    add     x10, x9, #1
    STX     x10, cr_pending_pos
    LDX     x3, cr_pending
    ldr     w21, [x3, x9, lsl #2]
    mov     w0, w21
    MOV64   w1, CR_S_WEAKEN
    bl      scene_activate_name
    mov     w0, w21
    bl      active_insert
    b       crumble_next_frame.fall_one
crumble_next_frame.fall_delay:
    LDX     x0, cr_min_fall_delay
    LDX     x1, cr_max_fall_delay
    bl      rng_randint
    STX     x9, cr_fall_delay
    mov     x0, #1
    mov     x1, #10
    bl      rng_randint
    cmp     x9, #4
    b.le    crumble_next_frame.fall_check
    LDX     x9, cr_group_maxsize
    add     x9, x9, #1
    STX     x9, cr_group_maxsize
    LDX     x9, cr_min_fall_delay
    subs    x9, x9, #1
    csel    x9, xzr, x9, lt
    STX     x9, cr_min_fall_delay
    LDX     x9, cr_max_fall_delay
    subs    x9, x9, #1
    csel    x9, xzr, x9, lt
    STX     x9, cr_max_fall_delay
crumble_next_frame.fall_check:
    LDX     x9, cr_pending_pos
    LDX     x10, cr_pending_count
    cmp     x9, x10
    b.lo    crumble_next_frame.update
    bl      active_empty
    cbz     w9, crumble_next_frame.update
    mov     x9, #CR_VACUUMING
    STX     x9, cr_stage
    b       crumble_next_frame.update
crumble_next_frame.vacuuming:
    LDX     x9, cr_unvacuumed_pos
    LDX     x10, cr_unvacuumed_count
    cmp     x9, x10
    b.hs    crumble_next_frame.vacuum_check
    mov     x0, #3
    mov     x1, #10
    bl      rng_randint
    mov     x19, x9
crumble_next_frame.vacuum_one:
    cmp     x19, #0
    b.le    crumble_next_frame.vacuum_check
    sub     x19, x19, #1
    LDX     x9, cr_unvacuumed_pos
    LDX     x10, cr_unvacuumed_count
    cmp     x9, x10
    b.hs    crumble_next_frame.vacuum_one
    add     x10, x9, #1
    STX     x10, cr_unvacuumed_pos
    LDX     x3, cr_unvacuumed
    ldr     w21, [x3, x9, lsl #2]
    mov     w0, w21
    MOV64   w1, CR_P_TOP
    bl      path_activate_name
    mov     w0, w21
    bl      active_insert
    b       crumble_next_frame.vacuum_one
crumble_next_frame.vacuum_check:
    bl      active_empty
    cbz     w9, crumble_next_frame.update
    mov     x9, #CR_RESETTING
    STX     x9, cr_stage
    b       crumble_next_frame.update
crumble_next_frame.resetting:
    LDB     w9, cr_reset
    cbnz    w9, crumble_next_frame.reset_check
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    mov     x19, #0
crumble_next_frame.reset_one:
    cmp     x19, x22
    b.hs    crumble_next_frame.reset_done
    ldr     w0, [x21, x19, lsl #2]
    MOV64   w1, CR_P_INPUT
    bl      path_activate_name
    ldr     w0, [x21, x19, lsl #2]
    bl      active_insert
    add     x19, x19, #1
    b       crumble_next_frame.reset_one
crumble_next_frame.reset_done:
    mov     w9, #1
    STB     w9, cr_reset
crumble_next_frame.reset_check:
    bl      active_empty
    cbz     w9, crumble_next_frame.update
    mov     x9, #CR_COMPLETE
    STX     x9, cr_stage
crumble_next_frame.update:
    bl      update
    mov     w9, #1
    POP2    x22, x30
    POP2    x19, x21
    ret
crumble_next_frame.finished:
    mov     w9, #0
    POP2    x22, x30
    POP2    x19, x21
    ret

    .section .rodata
    .balign 8
cr_fall_speed:      .double 0.65
cr_one:             .double 1.0
cr_weak_brightness: .double 0.65
cr_dust_brightness: .double 0.55

    TSTATE
    .balign 8
cr_spectrum:        .skip 8
cr_map:             .skip 8
cr_map_width:       .skip 8
cr_dust_symbols:    .skip 8 * 3
cr_symbol:          .skip 8
cr_control:         .skip 8
cr_stops:           .skip 8 * 2
cr_steps:           .skip 8
cr_key_fg:          .skip 8
cr_key_bg:          .skip 8
cr_weak_fg:         .skip 8
cr_weak_bg:         .skip 8
cr_dust_fg:         .skip 8
cr_dust_bg:         .skip 8
// gradient records: (length, spectrum[16])
cr_weaken_fg:       .skip 8 * 17
cr_weaken_bg:       .skip 8 * 17
cr_flash_fg:        .skip 8 * 17
cr_flash_bg:        .skip 8 * 17
cr_strengthen_fg:   .skip 8 * 17
cr_strengthen_bg:   .skip 8 * 17
cr_fall_name:       .skip 4
cr_flash_name:      .skip 4
cr_strengthen_name: .skip 4
    .balign 8
cr_pending:         .skip 8
cr_pending_pos:     .skip 8
cr_pending_count:   .skip 8
cr_unvacuumed:      .skip 8
cr_unvacuumed_pos:  .skip 8
cr_unvacuumed_count: .skip 8
cr_fall_delay:      .skip 8
cr_max_fall_delay:  .skip 8
cr_min_fall_delay:  .skip 8
cr_group_maxsize:   .skip 8
cr_stage:           .skip 8
cr_reset:           .skip 1
cr_key_valid:       .skip 1

    .text
