// effects/thunderstorm.s - "Create a thunderstorm in the terminal"
// (src/effects/thunderstorm.rs).
//
// Two particle pools (rain, sparks) and a hand-managed stack of strike
// characters (available / pending / active lists). The storm budget reads
// clock_monotonic at exactly Rust's points: build, fade_complete and every
// storm frame. Characters are created, scenes built and RNG draws taken in
// Rust's order, so slots, ids and the random stream line up.
//
// Per-character words:
//   text characters:   ch_user0 = glow | fade << 32, ch_user1 = unfade | flash << 32
//   sparks:            ch_user0 = glow scene
//   strike characters: ch_user0 = flash scene, ch_user1 = strike symbol index

.equ HAVE_thunderstorm, 1

.equ THUNDERSTORM.lightning_color,      0
.equ THUNDERSTORM.glowing_color,        8
.equ THUNDERSTORM.text_glow_time,       16
.equ THUNDERSTORM.rain_symbols,         24      // *const u64 packed symbols
.equ THUNDERSTORM.rain_symbol_count,    32
.equ THUNDERSTORM.spark_symbols,        40
.equ THUNDERSTORM.spark_symbol_count,   48
.equ THUNDERSTORM.spark_glow_color,     56
.equ THUNDERSTORM.spark_glow_time,      64
.equ THUNDERSTORM.storm_time,           72
.equ THUNDERSTORM.final_stops,          80      // *const u64
.equ THUNDERSTORM.final_stop_count,     88
.equ THUNDERSTORM.final_steps,          96      // *const i64
.equ THUNDERSTORM.final_step_count,     104
.equ THUNDERSTORM.final_direction,      112
.equ THUNDERSTORM_size,                 120

// scene names
.equ TS_GLOW,               NAME_LITERAL + 0
.equ TS_FADE,               NAME_LITERAL + 1
.equ TS_UNFADE,             NAME_LITERAL + 2
.equ TS_FLASH,              NAME_LITERAL + 3

.equ TS_PRE_STORM,          0
.equ TS_WAITING,            1
.equ TS_STORM,              2
.equ TS_COMPLETE,           3

.equ TS_EASE_OUT_QUINT,     14
.equ TS_EASE_IN_CIRC,       19

.equ TS_LIST_LIMIT,         (1 << 24)   // entries per strike / glow list

// Gradient::with_steps lengths: two stops and 7 steps give 8 colors, the
// looped flash 15, the strike fade (6 steps) 7.
.equ TS_GRAD,               8
.equ TS_FLASH_LEN,          15
.equ TS_STRIKE_FADE_LEN,    7

// a scene record's cold fields, from its cold half (engine/scene.s)
.equ TS_SCC_EASE,           SC_EASE - SCENE_COLD
.equ TS_SCC_OWNER,          SC_OWNER - SCENE_COLD

    .text

// thunderstorm_build: Thunderstorm::new + build().
thunderstorm_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     x9, #-1
    STX     x9, ts_audio_branch_budget  // uncapped outside on_audio strikes
    // the strike and glow lists share one reservation
    mov     x0, #(TS_LIST_LIMIT * 4 * 4)
    bl      reserve
    mov     x10, #(TS_LIST_LIMIT * 4)
    STX     x9, ts_pending
    add     x9, x9, x10
    STX     x9, ts_available
    add     x9, x9, x10
    STX     x9, ts_active
    add     x9, x9, x10
    STX     x9, ts_glow
    LDX     x19, effect_config
    // ParticlePool::new for rain (unbounded) and sparks (at most 2000); both
    // emit with ParticleReset { clear_events, ..default }
    ADRG    x0, ts_rain_pool
    ldr     x1, [x19, #THUNDERSTORM.rain_symbols]
    ldr     x2, [x19, #THUNDERSTORM.rain_symbol_count]
    mov     x3, #-1
    mov     x4, #0
    bl      pool_init
    ADRG    x0, ts_rain_pool
    mov     x9, #(RESET_DEFAULT | RESET_CLEAR_EVENTS)
    str     x9, [x0, #POOL.reset]
    ADRG    x9, ts_init_raindrop
    str     x9, [x0, #POOL.initializer]
    ADRG    x0, ts_spark_pool
    ldr     x1, [x19, #THUNDERSTORM.spark_symbols]
    ldr     x2, [x19, #THUNDERSTORM.spark_symbol_count]
    mov     w3, #2000
    mov     x4, #0
    bl      pool_init
    ADRG    x0, ts_spark_pool
    mov     x9, #(RESET_DEFAULT | RESET_CLEAR_EVENTS)
    str     x9, [x0, #POOL.reset]
    ADRG    x9, ts_init_spark
    str     x9, [x0, #POOL.initializer]
    // __init__ preamble: rain, the spark gradient, sparks, the storm clock
    ADRG    x0, ts_rain_pool
    mov     w1, #50
    bl      pool_preallocate
    ldr     x0, [x19, #THUNDERSTORM.spark_glow_color]
    bl      ts_background
    mov     x1, x9
    ADRG    x2, ts_spark_spectrum
    bl      ts_gradient2
    STW     w9, ts_spark_len
    ADRG    x0, ts_spark_pool
    mov     w1, #200
    bl      pool_preallocate
    bl      clock_monotonic
    STD     d0, ts_storm_start
    // build(): the final gradient mapping, 200 strike characters
    bl      ts_final_color_map
    mov     w0, #200
    bl      ts_build_strike_characters
    bl      ts_strike_visuals
    // scenes on the text characters
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    STX     x9, ts_text
    STX     x2, ts_text_count
    mov     x21, #0
thunderstorm_build.text:
    LDX     x9, ts_text_count
    cmp     x21, x9
    b.hs    thunderstorm_build.reference
    LDX     x9, ts_text
    ldr     w0, [x9, x21, lsl #2]
    bl      ts_text_scenes
    add     x21, x21, #1
    b       thunderstorm_build.text
thunderstorm_build.reference:
    // the first character's fade completing starts the storm
    LDX     x9, ts_text_count
    cbz     x9, thunderstorm_build.built
    LDX     x9, ts_text
    ldr     w0, [x9]
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, TS_FADE
    mov     w4, #ACT_CALLBACK
    ADRG    x5, ts_cb_fade_complete
    mov     x6, #0
    bl      event_register
thunderstorm_build.built:
    LDX     x9, ts_branch_default
    STX     x9, ts_branch_chance
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// ts_background -> x9: TerminalConfig.terminal_background_color. Keeps x0.
ts_background:
    LDX     x9, request
    ldr     x9, [x9, #RQ_BACKGROUND]
    ret

// ts_gradient2(x0=start, x1=end, x2=out) -> w9 = length:
// Gradient::with_steps([start, end], 7, false).
ts_gradient2:
    ADRG    x9, ts_stops
    stp     x0, x1, [x9]
    mov     x0, x9
    mov     w1, #2
    mov     x4, x2
    ADRG    x2, ts_seven
    mov     w3, #1
    b       gradient_new

// ts_gradient_loop(x0=start, x1=end, x2=out) -> w9 = length:
// Gradient::with_steps([start, end], 7, true) - the stops go round.
ts_gradient_loop:
    ADRG    x9, ts_stops
    stp     x0, x1, [x9]
    str     x0, [x9, #16]
    mov     x0, x9
    mov     w1, #3
    mov     x4, x2
    ADRG    x2, ts_seven
    mov     w3, #1
    b       gradient_new

// ts_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
ts_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #THUNDERSTORM.final_steps]
    ldr     x3, [x19, #THUNDERSTORM.final_step_count]
    ldr     x1, [x19, #THUNDERSTORM.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, ts_final_spectrum
    ldr     x0, [x19, #THUNDERSTORM.final_stops]
    ldr     x1, [x19, #THUNDERSTORM.final_stop_count]
    ldr     x2, [x19, #THUNDERSTORM.final_steps]
    ldr     x3, [x19, #THUNDERSTORM.final_step_count]
    LDX     x4, ts_final_spectrum
    bl      gradient_new
    LDX     x0, ts_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, ts_final_width
    ldr     x6, [x19, #THUNDERSTORM.final_direction]
    bl      gradient_map
    STX     x9, ts_final_map
    POP2    x19, x30
    ret

// ts_build_strike_characters(w0=count): build_strike_characters - "|"
// characters at (1, 1) pushed onto the available stack.
ts_build_strike_characters:
    PUSH2   x19, x30
    mov     w19, w0
ts_build_strike_characters.next:
    cbz     w19, ts_build_strike_characters.done
    MOV64   x0, (0x7c | 1 << 32)        // "|"
    MOV64   x1, (1 << 32 | 1)
    bl      add_character
    LDX     x3, ts_available_count
    LDX     x2, ts_available
    str     w9, [x2, x3, lsl #2]
    add     x3, x3, #1
    STX     x3, ts_available_count
    sub     w19, w19, #1
    b       ts_build_strike_characters.next
ts_build_strike_characters.done:
    POP2    x19, x30
    ret

// ts_strike_visuals: lightning_strike's flash and fade frames for each
// strike symbol. Their colors depend only on the config, so every strike
// shares them.
ts_strike_visuals:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #THUNDERSTORM.lightning_color]
    LDD     d0, ts_bright
    bl      adjust_color_brightness
    mov     x1, x9
    ldr     x0, [x19, #THUNDERSTORM.lightning_color]
    ADRG    x2, ts_strike_flash_colors
    bl      ts_gradient_loop
    // Gradient::with_steps([lightning, background], 6, false)
    ldr     x9, [x19, #THUNDERSTORM.lightning_color]
    STX     x9, ts_stops
    bl      ts_background
    ADRG    x0, ts_stops
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, ts_six
    mov     w3, #1
    ADRG    x4, ts_strike_fade_colors
    bl      gradient_new
    mov     w21, #0                     // symbol index
ts_strike_visuals.symbol:
    cmp     w21, #3
    b.hs    ts_strike_visuals.done
    mov     w22, #0
ts_strike_visuals.flash:
    ADRG    x9, ts_strike_flash_colors
    ldr     x0, [x9, x22, lsl #3]
    mov     x1, #NONE
    ADRG    x9, ts_strike_symbols
    ldr     x2, [x9, x21, lsl #3]
    mov     w3, #0
    bl      visual_make
    mov     w3, #TS_FLASH_LEN
    mul     w3, w21, w3
    add     w3, w3, w22
    ADRG    x2, ts_strike_flash_handles
    str     w9, [x2, x3, lsl #2]
    add     w22, w22, #1
    cmp     w22, #TS_FLASH_LEN
    b.lo    ts_strike_visuals.flash
    mov     w22, #0
ts_strike_visuals.fade:
    ADRG    x9, ts_strike_fade_colors
    ldr     x0, [x9, x22, lsl #3]
    mov     x1, #NONE
    ADRG    x9, ts_strike_symbols
    ldr     x2, [x9, x21, lsl #3]
    mov     w3, #0
    bl      visual_make
    mov     w3, #TS_STRIKE_FADE_LEN
    mul     w3, w21, w3
    add     w3, w3, w22
    ADRG    x2, ts_strike_fade_handles
    str     w9, [x2, x3, lsl #2]
    add     w22, w22, #1
    cmp     w22, #TS_STRIKE_FADE_LEN
    b.lo    ts_strike_visuals.fade
    add     w21, w21, #1
    b       ts_strike_visuals.symbol
ts_strike_visuals.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// ts_text_colors(x0=visible fg, x1=visible bg or NONE): the colors build()
// derives from a character's visible pair. Neighbors mostly share one, so
// the last result is kept.
ts_text_colors:
    LDB     w9, ts_tc_valid
    cbz     w9, ts_text_colors.compute
    LDX     x9, ts_tc_fg
    cmp     x0, x9
    b.ne    ts_text_colors.compute
    LDX     x9, ts_tc_bg
    cmp     x1, x9
    b.ne    ts_text_colors.compute
    ret
ts_text_colors.compute:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w9, #1
    STB     w9, ts_tc_valid
    STX     x0, ts_tc_fg
    STX     x1, ts_tc_bg
    mov     x19, x0
    mov     x21, x1
    // storm colors: _adjust_color_pair_brightness(visible, 0.5)
    LDD     d0, ts_half
    bl      adjust_color_brightness
    STX     x9, ts_storm_fg
    mov     x9, #NONE
    cmn     x21, #1                     // NONE
    b.eq    ts_text_colors.storm_bg
    mov     x0, x21
    LDD     d0, ts_half
    bl      adjust_color_brightness
ts_text_colors.storm_bg:
    STX     x9, ts_storm_bg
    ADRG    x0, ts_bg_storm
    mov     x3, #16
    REP_STOSQ
    // glow: glowing text color -> storm fg
    LDX     x0, effect_config
    ldr     x0, [x0, #THUNDERSTORM.glowing_color]
    LDX     x1, ts_storm_fg
    ADRG    x2, ts_glow_colors
    bl      ts_gradient2
    // fade: visible fg -> storm fg; unfade plays it backwards
    mov     x0, x19
    LDX     x1, ts_storm_fg
    ADRG    x2, ts_fade_colors
    bl      ts_gradient2
    ADRG    x1, ts_fade_colors
    ADRG    x0, ts_unfade_colors
    mov     w3, #TS_GRAD
ts_text_colors.reverse:
    sub     w3, w3, #1
    ldr     x9, [x1, x3, lsl #3]
    str     x9, [x0], #8
    cbnz    w3, ts_text_colors.reverse
    // flash: storm fg -> visible fg at 1.7 brightness, looped
    mov     x0, x19
    LDD     d0, ts_bright
    bl      adjust_color_brightness
    mov     x1, x9
    LDX     x0, ts_storm_fg
    ADRG    x2, ts_flash_colors
    bl      ts_gradient_loop
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    ts_text_colors.done
    // dynamic: the pair gradients of _add_color_pair_gradient_frames
    LDX     x0, ts_storm_fg
    mov     x1, x19
    ADRG    x2, ts_unfade_colors
    bl      ts_gradient2
    mov     x0, x21
    LDX     x1, ts_storm_bg
    ADRG    x2, ts_fade_bg
    bl      ts_pair_steps
    LDX     x0, ts_storm_bg
    mov     x1, x21
    ADRG    x2, ts_unfade_bg
    bl      ts_pair_steps
ts_text_colors.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// ts_pair_steps(x0=start or NONE, x1=end or NONE, x2=out): one channel of
// _add_color_pair_gradient_frames - the gradient when both colors exist,
// else the end color (or the start) repeated.
ts_pair_steps:
    cmn     x0, #1                      // NONE
    b.eq    ts_pair_steps.fill
    cmn     x1, #1
    b.ne    ts_gradient2
ts_pair_steps.fill:
    mov     x9, x1
    cmn     x9, #1
    csel    x9, x0, x9, eq
    mov     x0, x2
    mov     x3, #TS_GRAD
    REP_STOSQ
    ret

// ts_add_frames(w0=scene, x1=fg colors, x2=bg colors, w3=count,
//               w4=duration): add_frame per color pair, with the symbol in
// [ts_symbol].
ts_add_frames:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    stp     x23, x30, [sp, #32]
    mov     w19, w0
    mov     x21, x1
    mov     x22, x2
    mov     w20, w3
    mov     w23, w4
ts_add_frames.next:
    cbz     w20, ts_add_frames.done
    mov     w0, w19
    LDX     x1, ts_symbol
    mov     w2, w23
    ldr     x3, [x21], #8
    ldr     x4, [x22], #8
    mov     w5, #0
    bl      scene_add_frame
    sub     w20, w20, #1
    b       ts_add_frames.next
ts_add_frames.done:
    ldp     x23, x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// ts_add_frame(w0=scene, x1=fg, x2=bg, w3=duration): one add_frame.
ts_add_frame:
    mov     x4, x2
    mov     w2, w3
    mov     x3, x1
    LDX     x1, ts_symbol
    mov     w5, #0
    b       scene_add_frame

// ts_new_scene(w0=slot, w1=name) -> w9: a plain named scene.
ts_new_scene:
    mov     w2, #0
    mov     w3, #NONE
    b       scene_new

// ts_text_scenes(w0=slot): build()'s glow, fade, unfade and flash scenes
// for one text character, then its visibility.
ts_text_scenes:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w21, w0
    LDX     x9, ch_sym
    ldr     x9, [x9, x21, lsl #3]
    STX     x9, ts_symbol
    LDX     x24, effect_config
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    ts_text_scenes.dynamic_colors
    // visible = (final gradient color at the input coordinate, none)
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x21, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, ts_final_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x21, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, ts_final_map
    ldr     x22, [x3, x9, lsl #3]
    mov     x23, #NONE
    b       ts_text_scenes.colors
ts_text_scenes.dynamic_colors:
    // visible = (input fg or neutral gray, input bg)
    LDX     x9, ch_fg
    ldr     x22, [x9, x21, lsl #3]
    cmn     x22, #1                     // NONE
    b.ne    ts_text_scenes.input_bg
    MOV64   w22, 0x808080
ts_text_scenes.input_bg:
    LDX     x9, ch_bg
    ldr     x23, [x9, x21, lsl #3]
ts_text_scenes.colors:
    mov     x0, x22
    mov     x1, x23
    bl      ts_text_colors
    // glow: glowing -> storm over 8 frames of text_glow_time
    mov     w0, w21
    MOV64   w1, TS_GLOW
    bl      ts_new_scene
    mov     w19, w9
    LDX     x3, ch_user0
    add     x3, x3, x21, lsl #3
    str     w9, [x3]
    mov     w0, w19
    ADRG    x1, ts_glow_colors
    ADRG    x2, ts_bg_storm
    mov     w3, #TS_GRAD
    ldr     x4, [x24, #THUNDERSTORM.text_glow_time]
    bl      ts_add_frames
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    ts_text_scenes.fade
    mov     w0, w19
    LDX     x1, ts_storm_fg
    LDX     x2, ts_storm_bg
    ldr     x3, [x24, #THUNDERSTORM.text_glow_time]
    bl      ts_add_frame
ts_text_scenes.fade:
    mov     w0, w21
    MOV64   w1, TS_FADE
    bl      ts_new_scene
    mov     w19, w9
    LDX     x3, ch_user0
    add     x3, x3, x21, lsl #3
    str     w9, [x3, #4]
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    ts_text_scenes.dynamic_fade
    mov     w0, w19
    ADRG    x1, ts_fade_colors
    ADRG    x2, ts_bg_none
    mov     w3, #TS_GRAD
    mov     w4, #12
    bl      ts_add_frames
    b       ts_text_scenes.unfade
ts_text_scenes.dynamic_fade:
    // range(7) of the pair gradient, then the storm colors
    mov     w0, w19
    ADRG    x1, ts_fade_colors
    ADRG    x2, ts_fade_bg
    mov     w3, #7
    mov     w4, #12
    bl      ts_add_frames
    mov     w0, w19
    LDX     x1, ts_storm_fg
    LDX     x2, ts_storm_bg
    mov     w3, #12
    bl      ts_add_frame
ts_text_scenes.unfade:
    mov     w0, w21
    MOV64   w1, TS_UNFADE
    bl      ts_new_scene
    mov     w19, w9
    LDX     x3, ch_user1
    add     x3, x3, x21, lsl #3
    str     w9, [x3]
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    ts_text_scenes.dynamic_unfade
    mov     w0, w19
    ADRG    x1, ts_unfade_colors
    ADRG    x2, ts_bg_none
    mov     w3, #TS_GRAD
    mov     w4, #12
    bl      ts_add_frames
    b       ts_text_scenes.flash
ts_text_scenes.dynamic_unfade:
    mov     w0, w19
    ADRG    x1, ts_unfade_colors
    ADRG    x2, ts_unfade_bg
    mov     w3, #7
    mov     w4, #12
    bl      ts_add_frames
    mov     w0, w19
    mov     x1, x22
    mov     x2, x23
    mov     w3, #12
    bl      ts_add_frame
    // the restore colors (the input pair) when they differ from visible
    LDX     x9, ch_fg
    ldr     x1, [x9, x21, lsl #3]
    LDX     x9, ch_bg
    ldr     x2, [x9, x21, lsl #3]
    cmp     x1, x22
    b.ne    ts_text_scenes.restore
    cmp     x2, x23
    b.eq    ts_text_scenes.flash
ts_text_scenes.restore:
    mov     w0, w19
    mov     w3, #12
    bl      ts_add_frame
ts_text_scenes.flash:
    mov     w0, w21
    MOV64   w1, TS_FLASH
    bl      ts_new_scene
    mov     w19, w9
    LDX     x3, ch_user1
    add     x3, x3, x21, lsl #3
    str     w9, [x3, #4]
    mov     w0, w19
    ADRG    x1, ts_flash_colors
    ADRG    x2, ts_bg_storm
    mov     w3, #TS_FLASH_LEN
    mov     w4, #6
    bl      ts_add_frames
    mov     w0, w21
    bl      set_visible
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// ------------------------------------------------------------ particles

// ts_init_raindrop(w0=slot): initialize_raindrop - layer 1, the input
// symbol in aaaaff.
ts_init_raindrop:
    PUSH2   x19, x30
    mov     w19, w0
    mov     x1, #1
    bl      set_layer
    mov     w0, w19
    mov     x1, #0
    MOV64   x2, 0xaaaaff
    mov     x3, #NONE
    bl      set_appearance
    POP2    x19, x30
    ret

// ts_init_spark(w0=slot): _build_spark_characters - layer 2 and an
// in_circ "glow" scene down the spark gradient.
ts_init_spark:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w19, w0
    mov     x1, #2
    bl      set_layer
    mov     w0, w19
    MOV64   w1, TS_GLOW
    mov     w2, #0
    mov     w3, #TS_EASE_IN_CIRC
    bl      scene_new
    mov     w21, w9
    LDX     x3, ch_user0
    add     x3, x3, x19, lsl #3
    str     w9, [x3]
    mov     w22, #0
ts_init_spark.frame:
    LDW     w9, ts_spark_len
    cmp     w22, w9
    b.hs    ts_init_spark.done
    mov     w0, w21
    LDX     x1, ch_sym
    ldr     x1, [x1, x19, lsl #3]
    LDX     x2, effect_config
    ldr     x2, [x2, #THUNDERSTORM.spark_glow_time]
    ADRG    x3, ts_spark_spectrum
    ldr     x3, [x3, x22, lsl #3]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    add     w22, w22, #1
    b       ts_init_spark.frame
ts_init_spark.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// ts_setup_raindrop(w0=slot): _setup_raindrop - a straight fall to row 0,
// reclaimed when the path completes.
ts_setup_raindrop:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w19, w0
    bl      char_coord
    mov     x21, x9                     // origin
    LDD     d0, ts_half
    LDD     d1, ts_one_half
    bl      rng_uniform
    mov     w0, w19
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w22, w9
    // (origin.column + canvas.top + 1, canvas.bottom - 1)
    sxtw    x1, w21
    LDX     x9, canvas_top
    add     x1, x1, x9
    add     x1, x1, #1
    mov     w1, w1
    mov     w0, w22
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    PATH_PTR x3, x22
    ldr     w3, [x3, #PA_NAME]
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w4, #ACT_CALLBACK
    ADRG    x5, ts_reclaim
    ADRG    x6, ts_rain_pool
    bl      event_register
    mov     w0, w19
    mov     w1, w22
    bl      path_activate
    POP2    x22, x30
    POP2    x19, x21
    ret

// ts_setup_sparks(w0=slot): _setup_sparks_for_impact - an out_quint bezier
// arc to the canvas bottom, the glow scene, reclaimed when the glow ends.
// [sp, #48]: the control point.
ts_setup_sparks:
    stp     x19, x21, [sp, #-64]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    mov     w19, w0
    bl      char_coord
    mov     x21, x9                     // impact
    LDD     d0, ts_spark_speed_low
    LDD     d1, ts_spark_speed_high
    bl      rng_uniform
    mov     w0, w19
    mov     w1, #TS_EASE_OUT_QUINT
    MOV64   x2, NONE_I64
    mov     x3, #30
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w22, w9
    // offset = randint(4, 20) * choice([1, -1])
    mov     x0, #4
    mov     x1, #20
    bl      rng_randint
    mov     x23, x9
    mov     x0, #2
    bl      rng_below
    cbz     x9, ts_setup_sparks.positive
    neg     x23, x23
ts_setup_sparks.positive:
    sxtw    x9, w21
    add     x23, x9, x23                // target column
    // bezier column: impact - floor_div(impact - target, 2)
    sub     x3, x9, x23
    asr     x3, x3, #1
    sub     x9, x9, x3
    mov     x24, x9
    mov     x0, #1
    LDX     x1, canvas_top
    bl      rng_randint
    lsl     x9, x9, #32
    mov     w3, w24
    orr     x9, x9, x3
    str     x9, [sp, #48]               // the control point
    mov     w1, w23
    mov     x9, #(1 << 32)              // canvas.bottom
    orr     x1, x1, x9
    mov     w0, w22
    add     x2, sp, #48
    mov     w3, #1
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, TS_GLOW
    mov     w4, #ACT_CALLBACK
    ADRG    x5, ts_reclaim
    ADRG    x6, ts_spark_pool
    bl      event_register
    mov     w0, w19
    LDX     x9, ch_user0
    add     x9, x9, x19, lsl #3
    ldr     w1, [x9]
    bl      scene_activate
    mov     w0, w19
    mov     w1, w22
    bl      path_activate
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #64
    ret

// ts_reclaim(w0=slot, x1=pool): the pools' reclaim_on_event callback.
ts_reclaim:
    mov     w9, w0
    mov     x0, x1
    mov     w1, w9
    mov     w2, #1
    mov     w3, #1
    b       pool_reclaim

// ------------------------------------------------------------ callbacks

// fade_complete: the storm begins and its clock restarts.
ts_cb_fade_complete:
    PUSH1   x30
    mov     w9, #TS_STORM
    STB     w9, ts_phase
    bl      clock_monotonic
    STD     d0, ts_storm_start
    POP1    x30
    ret

// hide_character
ts_cb_hide:
    mov     w1, #0
    b       set_visibility

// make_char_glow: the visible text character under the strike character
// starts glowing; it joins the active set on the next storm frame.
ts_cb_glow:
    PUSH2   x19, x30
    bl      char_coord
    mov     x1, x9
    bl      char_at_input_coord
    cmn     w9, #1                      // NONE
    b.eq    ts_cb_glow.done
    mov     w19, w9
    LDX     x3, ch_flags
    ldrh    w3, [x3, x19, lsl #1]
    tst     w3, #CF_VISIBLE
    b.eq    ts_cb_glow.done
    mov     w0, w19
    LDX     x9, ch_user0
    add     x9, x9, x19, lsl #3
    ldr     w1, [x9]       // glow
    bl      scene_activate
    LDX     x9, ts_glow_count
    LDX     x3, ts_glow
    str     w19, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, ts_glow_count
ts_cb_glow.done:
    POP2    x19, x30
    ret

// return_strike_to_pool
ts_cb_return_strike:
    LDX     x9, ts_available_count
    LDX     x3, ts_available
    str     w0, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, ts_available_count
    ret

// set_strike_in_progress_false
ts_cb_strike_done:
    STB     wzr, ts_strike_in_progress
    ret

// ------------------------------------------------------------ the storm

// ts_setup_strike(w0=branch neighbor or NONE): setup_lightning_strike, with
// its recursive branching. Strike characters are all created with input
// symbol "|", so a branch's first step always takes the random-delta arm
// (the "/" and "\\" arms are unreachable upstream too).
ts_setup_strike:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    stp     x23, x30, [sp, #32]
    mov     w23, w0                     // branch neighbor
    cmn     w23, #1                     // NONE
    b.eq    ts_setup_strike.fresh
    bl      char_coord
    sxtw    x21, w9                     // column
    asr     x22, x9, #32                // row
    b       ts_setup_strike.row
ts_setup_strike.fresh:
    mov     x0, #1
    LDX     x1, canvas_right
    bl      rng_randint
    mov     x21, x9
    LDX     x22, canvas_top
ts_setup_strike.row:
    cmp     x22, #1                     // canvas.bottom
    b.lt    ts_setup_strike.done
    LDX     x9, ts_available_count
    cbnz    x9, ts_setup_strike.symbol
    mov     w0, #20
    bl      ts_build_strike_characters
ts_setup_strike.symbol:
    cmn     w23, #1
    b.eq    ts_setup_strike.choose
    // delta = choice([-1, 1]); "\\" when it is 1, else "/"
    mov     x0, #2
    bl      rng_below
    lsl     x3, x9, #1
    sub     x3, x3, #1
    add     x21, x21, x3
    mov     w20, #0                     // "\\"
    cbnz    x9, ts_setup_strike.place
    mov     w20, #1                     // "/"
    b       ts_setup_strike.place
ts_setup_strike.choose:
    mov     x0, #3
    bl      rng_below                   // choice(["\\", "/", "|"])
    mov     w20, w9
ts_setup_strike.place:
    // get_next_strike_char: pop, clear its scenes and events
    LDX     x9, ts_available_count
    sub     x9, x9, #1
    STX     x9, ts_available_count
    LDX     x3, ts_available
    ldr     w19, [x3, x9, lsl #2]
    mov     w0, w19
    bl      scenes_release
    mov     w0, w19
    bl      events_release
    mov     w1, w21
    lsl     x9, x22, #32
    orr     x1, x1, x9
    mov     w0, w19
    bl      set_coordinate
    LDX     x9, ch_user1
    str     x20, [x9, x19, lsl #3]
    ADRG    x9, ts_strike_symbols
    ldr     x1, [x9, x20, lsl #3]
    LDX     x2, effect_config
    ldr     x2, [x2, #THUNDERSTORM.lightning_color]
    mov     x3, #NONE
    mov     w0, w19
    bl      set_appearance
    sub     x22, x22, #1
    cbnz    w20, ts_setup_strike.slash
    add     x21, x21, #1
    b       ts_setup_strike.pending
ts_setup_strike.slash:
    cmp     w20, #1
    b.ne    ts_setup_strike.pending
    sub     x21, x21, #1
ts_setup_strike.pending:
    LDX     x9, ts_pending_count
    LDX     x3, ts_pending
    str     w19, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, ts_pending_count
    // random() is always drawn; a branch never branches at its first step.
    // An audio strike caps its branches at the volume's budget (fx
    // thunderstorm.rs's audio_branch_budget): -1 is uncapped, 0 blocks.
    bl      rng_random
    LDD     d1, ts_branch_chance
    fcmp    d1, d0
    b.le    ts_setup_strike.next        // !(random() < chance)
    cmn     w23, #1
    b.ne    ts_setup_strike.next
    LDX     x9, ts_audio_branch_budget
    cbz     x9, ts_setup_strike.next
    b.mi    ts_setup_strike.branch      // -1: uncapped, no decrement
    sub     x9, x9, #1
    STX     x9, ts_audio_branch_budget
ts_setup_strike.branch:
    LDD     d2, ts_branch_step
    fsub    d1, d1, d2
    STD     d1, ts_branch_chance
    mov     w0, w19
    bl      ts_setup_strike
ts_setup_strike.next:
    mov     w23, #NONE
    b       ts_setup_strike.row
ts_setup_strike.done:
    LDX     x9, ts_branch_default
    STX     x9, ts_branch_chance
    ldp     x23, x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// ts_lightning_strike: lightning_strike - one ambient ray from the top, then
// the shared scene setup.
ts_lightning_strike:
    str     x30, [sp, #-16]!
    STX     xzr, ts_pending_head
    STX     xzr, ts_pending_count
    mov     w0, #NONE
    bl      ts_setup_strike
    ldr     x30, [sp], #16
    b       ts_finish_strike

// ts_audio_strike(w0=rays, w1=branch budget): on_audio's strike - rays rays
// from the top, each with the volume's branch budget, then the shared scene
// setup. The budget is per ray and uncapped (-1) again afterwards.
ts_audio_strike:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w19, w0                     // rays remaining
    mov     w21, w1                     // branch budget per ray
    STX     xzr, ts_pending_head
    STX     xzr, ts_pending_count
ts_audio_strike.ray:
    sxtw    x9, w21
    STX     x9, ts_audio_branch_budget
    mov     w0, #NONE
    bl      ts_setup_strike
    sub     w19, w19, #1
    cbnz    w19, ts_audio_strike.ray
    mov     x9, #-1
    STX     x9, ts_audio_branch_budget
    POP2    x22, x30
    POP2    x19, x21
    b       ts_finish_strike

// ts_finish_strike: finish_lightning_strike - lay out the bolt, give each of
// its characters flash (eased by a fresh random curve) and fade scenes, and
// ease the text's flash scenes by the same curve.
ts_finish_strike:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    stp     x23, x30, [sp, #32]
    LDD     d0, ts_ease_y2_low
    LDD     d1, ts_ease_y2_high
    bl      rng_uniform
    fmov    d3, d0
    fmov    d0, xzr
    LDD     d1, ts_ease_y1
    LDD     d2, ts_one
    bl      bezier_new
    STW     w9, ts_flash_ease
    LDX     x21, ts_pending_head
ts_finish_strike.strike:
    LDX     x9, ts_pending_count
    cmp     x21, x9
    b.hs    ts_finish_strike.text
    LDX     x9, ts_pending
    ldr     w19, [x9, x21, lsl #2]
    LDX     x9, ch_user1
    ldr     x20, [x9, x19, lsl #3]      // symbol index
    mov     w0, w19
    MOV64   w1, TS_FLASH
    mov     w2, #0
    LDW     w3, ts_flash_ease
    bl      scene_new
    mov     w22, w9
    LDX     x3, ch_user0
    add     x3, x3, x19, lsl #3
    str     w9, [x3]
    mov     w9, #TS_FLASH_LEN
    mul     w23, w20, w9
ts_finish_strike.flash_frame:
    mov     w0, w22
    ADRG    x9, ts_strike_flash_handles
    ldr     w1, [x9, x23, lsl #2]
    mov     w2, #6
    bl      scene_add_frame_visual
    add     w23, w23, #1
    mov     w9, #TS_FLASH_LEN
    mul     w9, w20, w9
    add     w9, w9, #TS_FLASH_LEN
    cmp     w23, w9
    b.lo    ts_finish_strike.flash_frame
    mov     w0, w19
    MOV64   w1, TS_FADE
    bl      ts_new_scene
    mov     w22, w9
    mov     w9, #TS_STRIKE_FADE_LEN
    mul     w23, w20, w9
ts_finish_strike.fade_frame:
    mov     w0, w22
    ADRG    x9, ts_strike_fade_handles
    ldr     w1, [x9, x23, lsl #2]
    mov     w2, #2
    bl      scene_add_frame_visual
    add     w23, w23, #1
    mov     w9, #TS_STRIKE_FADE_LEN
    mul     w9, w20, w9
    add     w9, w9, #TS_STRIKE_FADE_LEN
    cmp     w23, w9
    b.lo    ts_finish_strike.fade_frame
    mov     w0, w19
    mov     x1, #1
    bl      set_layer
    // flash -> fade; fade -> hide, glow, return to the pool
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, TS_FLASH
    mov     w4, #ACT_ACTIVATE_SCENE
    MOV64   w5, TS_FADE
    mov     x6, #0
    bl      event_register
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, TS_FADE
    mov     w4, #ACT_CALLBACK
    ADRG    x5, ts_cb_hide
    mov     x6, #0
    bl      event_register
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, TS_FADE
    mov     w4, #ACT_CALLBACK
    ADRG    x5, ts_cb_glow
    mov     x6, #0
    bl      event_register
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, TS_FADE
    mov     w4, #ACT_CALLBACK
    ADRG    x5, ts_cb_return_strike
    mov     x6, #0
    bl      event_register
    add     x21, x21, #1
    b       ts_finish_strike.strike
ts_finish_strike.text:
    // every text flash scene takes the new curve
    LDW     w2, ts_flash_ease
    mov     x3, #0
ts_finish_strike.ease:
    LDX     x9, ts_text_count
    cmp     x3, x9
    b.hs    ts_finish_strike.done
    LDX     x9, ts_text
    ldr     w9, [x9, x3, lsl #2]
    LDX     x1, ch_user1
    add     x1, x1, x9, lsl #3
    ldr     w9, [x1, #4]                // flash
    SCENE_PTR x4, x9
    MOV64   x5, SCENE_COLD
    add     x5, x4, x5                  // its cold half
    ldr     w0, [x5, #TS_SCC_OWNER]
    bl      doze_wake                   // update.s: its playback changes
    str     w2, [x5, #TS_SCC_EASE]
    ldr     w9, [x4, #SC_FLAGS]
    orr     w9, w9, #SCF_EASED
    str     w9, [x4, #SC_FLAGS]
    add     x3, x3, #1
    b       ts_finish_strike.ease
ts_finish_strike.done:
    ldp     x23, x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// ts_step_strike: step_lightning_strike - reveal 1-3 bolt characters every
// other frame; after the last, sparks fly and everything flashes.
ts_step_strike:
    LDX     x9, ts_progression_delay
    cbz     x9, ts_step_strike.go
    sub     x9, x9, #1
    STX     x9, ts_progression_delay
    ret
ts_step_strike.go:
    LDX     x9, ts_pending_head
    LDX     x3, ts_pending_count
    cmp     x9, x3
    b.lo    ts_step_strike.reveal
    ret
ts_step_strike.reveal:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    str     x30, [sp, #32]
    // step_lightning_strike's audio-aware batch on the bass channel: 3-5 on
    // a loud frame or a beat (bass byte >= 154, the reference's > 0.6), 2-4
    // when moderate (> 0.32, byte >= 82), else 1-3.
    LDB     w9, ts_audio_bass
    cmp     w9, #154
    b.hs    ts_step_strike.loud
    LDB     w10, ts_audio_beat
    cbnz    w10, ts_step_strike.loud
    cmp     w9, #82
    b.hs    ts_step_strike.moderate
    mov     x0, #1
    mov     x1, #3
    b       ts_step_strike.batch_size
ts_step_strike.moderate:
    mov     x0, #2
    mov     x1, #4
    b       ts_step_strike.batch_size
ts_step_strike.loud:
    mov     x0, #3
    mov     x1, #5
ts_step_strike.batch_size:
    bl      rng_randint
    mov     x20, x9                     // batch
ts_step_strike.batch:
    cbz     x20, ts_step_strike.done
    sub     x20, x20, #1
    LDX     x9, ts_pending_head
    LDX     x3, ts_pending_count
    cmp     x9, x3
    b.hs    ts_step_strike.done
    LDX     x3, ts_pending
    ldr     w19, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, ts_pending_head
    LDX     x9, ts_active_count
    LDX     x3, ts_active
    str     w19, [x3, x9, lsl #2]
    add     x9, x9, #1
    STX     x9, ts_active_count
    mov     w0, w19
    bl      set_visible
    // the progression delay follows the tempo: >= 135 BPM steps every frame,
    // 75-134 every other frame, slower every third frame; a beat always
    // steps. No audio: every other frame (fx step_lightning_strike).
    mov     x9, #1
    STX     x9, ts_progression_delay
    LDB     w9, ts_audio_beat
    cbnz    w9, ts_step_strike.no_delay
    LDB     w9, ts_audio_bpm
    cbz     w9, ts_step_strike.delayed   // no audio: reference delay 1
    cmp     w9, #135
    b.hs    ts_step_strike.no_delay
    cmp     w9, #75
    b.hs    ts_step_strike.delayed       // 75..134 BPM: delay 1, already set
    mov     x9, #2
    STX     x9, ts_progression_delay
    b       ts_step_strike.delayed
ts_step_strike.no_delay:
    STX     xzr, ts_progression_delay
ts_step_strike.delayed:
    LDX     x9, ts_pending_head
    LDX     x3, ts_pending_count
    cmp     x9, x3
    b.lo    ts_step_strike.batch
    // the last strike character: sparks at the impact
    mov     x0, #12
    mov     x1, #18
    bl      rng_randint
    mov     x21, x9
ts_step_strike.spark:
    cbz     x21, ts_step_strike.sparked
    LDX     x9, ts_active_count
    LDX     x3, ts_active
    add     x3, x3, x9, lsl #2
    ldur    w0, [x3, #-4]
    bl      char_coord
    ADRG    x0, ts_spark_pool
    mov     x1, x9
    mov     x2, #0
    mov     w3, #1
    ADRG    x4, ts_setup_sparks
    mov     x5, #0
    bl      pool_emit
    sub     x21, x21, #1
    b       ts_step_strike.spark
ts_step_strike.sparked:
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, TS_FADE
    mov     w4, #ACT_CALLBACK
    ADRG    x5, ts_cb_strike_done
    mov     x6, #0
    bl      event_register
    // flash the bolt, then the text
    mov     x21, #0
ts_step_strike.bolt:
    LDX     x9, ts_active_count
    cmp     x21, x9
    b.hs    ts_step_strike.bolt_done
    LDX     x9, ts_active
    ldr     w22, [x9, x21, lsl #2]
    LDX     x9, ch_user0
    add     x9, x9, x22, lsl #3
    ldr     w1, [x9]       // flash
    mov     w0, w22
    bl      scene_activate
    mov     w0, w22
    bl      active_insert
    add     x21, x21, #1
    b       ts_step_strike.bolt
ts_step_strike.bolt_done:
    STX     xzr, ts_active_count
    STX     xzr, ts_pending_head
    STX     xzr, ts_pending_count
    mov     w1, #(4 + 8)                // ch_user1 high: flash
    bl      ts_activate_text
    b       ts_step_strike.batch
ts_step_strike.done:
    ldr     x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// ts_activate_text(w1=byte offset of the scene in ch_user0/1): activate
// that scene on every text character (top to bottom, left to right) and make
// each active. Offsets: 0 glow, 4 fade, 8 unfade, 12 flash.
ts_activate_text:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    str     x30, [sp, #32]
    mov     w22, w1
    LDX     x19, ch_user0
    cmp     w22, #8
    b.lo    ts_activate_text.words
    LDX     x19, ch_user1
    sub     w22, w22, #8
ts_activate_text.words:
    mov     x21, #0
ts_activate_text.next:
    LDX     x9, ts_text_count
    cmp     x21, x9
    b.hs    ts_activate_text.done
    LDX     x9, ts_text
    ldr     w23, [x9, x21, lsl #2]
    add     x9, x19, x23, lsl #3
    ldr     w1, [x9, x22]
    mov     w0, w23
    bl      scene_activate
    mov     w0, w23
    bl      active_insert
    add     x21, x21, #1
    b       ts_activate_text.next
ts_activate_text.done:
    ldr     x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// ts_rain: rain - every few frames, 1-6 raindrops from above the canvas.
ts_rain:
    LDX     x9, ts_delay
    cbz     x9, ts_rain.spawn
    sub     x9, x9, #1
    STX     x9, ts_delay
    ret
ts_rain.spawn:
    PUSH2   x19, x30
    // treble drives the drops (agudos): silence keeps the reference 1-6
    // draw; with audio the count is deterministic, 1 + treble * 9 / 255.
    LDB     w9, ts_audio_treble
    cbz     w9, ts_rain.classic
    mov     w10, #9
    mul     w9, w9, w10
    mov     w10, #255
    udiv    w9, w9, w10
    add     x19, x9, #1
    b       ts_rain.drop
ts_rain.classic:
    mov     x0, #1
    mov     x1, #6
    bl      rng_randint
    mov     x19, x9
ts_rain.drop:
    cbz     x19, ts_rain.done
    LDX     x9, canvas_top
    mov     x0, #1
    sub     x0, x0, x9
    LDX     x1, canvas_right
    bl      rng_randint
    // origin (spawn_column - 1, canvas.top + 1)
    sub     w9, w9, #1
    mov     w1, w9
    LDX     x9, canvas_top
    add     x9, x9, #1
    lsl     x9, x9, #32
    orr     x1, x1, x9
    ADRG    x0, ts_rain_pool
    mov     x2, #0
    mov     w3, #1
    ADRG    x4, ts_setup_raindrop
    mov     x5, #0
    bl      pool_emit
    sub     x19, x19, #1
    b       ts_rain.drop
ts_rain.done:
    mov     x0, #1
    mov     x1, #7
    bl      rng_randint
    // the tempo scales the spacing between rain bursts: 90 BPM leaves the
    // reference 1-7 draw unchanged, faster tempi squeeze it (min 1), slower
    // stretch it (BPM floored at 45); no audio keeps the draw as is.
    LDB     w10, ts_audio_bpm
    cbz     w10, ts_rain.store_delay
    cmp     w10, #45
    b.hs    ts_rain.bpm_ok
    mov     w10, #45
ts_rain.bpm_ok:
    mov     w11, #90
    mul     x9, x9, x11
    udiv    x9, x9, x10
    cbnz    x9, ts_rain.store_delay
    mov     x9, #1
ts_rain.store_delay:
    STX     x9, ts_delay
    POP2    x19, x30
    ret

// thunderstorm_next_frame -> w9 = 1 for a frame, 0 when done.
thunderstorm_next_frame:
    PUSH2   x19, x30
    // on_audio's store: the frame's poll word, sampled by audio_frame_poll
    // (lib.s): volume byte 0, bass byte 1, beat byte 2, treble byte 3,
    // bpm byte 4; all zero when there is no audio.
    LDX     x9, audio_word
    STB     w9, ts_audio_volume
    ubfx    w10, w9, #8, #8
    STB     w10, ts_audio_bass
    ubfx    x10, x9, #32, #8            // bpm is byte 4, above the w9 half
    STB     w10, ts_audio_bpm
    // The analyzer's beat lasts several chunks. Convert it to an onset so
    // one held flag cannot repeatedly boost strikes and their progression.
    ubfx    w10, w9, #16, #8
    cmp     w10, #0
    cset    w10, ne
    LDB     w11, ts_audio_beat_previous
    STB     w10, ts_audio_beat_previous
    cmp     w11, #0
    cset    w11, eq
    and     w10, w10, w11
    STB     w10, ts_audio_beat
    ubfx    w10, w9, #24, #8
    STB     w10, ts_audio_treble
    LDB     w9, ts_phase
    cmp     w9, #TS_COMPLETE
    b.ne    thunderstorm_next_frame.phase
    bl      active_empty
    cbnz    w9, thunderstorm_next_frame.finished
thunderstorm_next_frame.phase:
    LDB     w9, ts_phase
    cmp     w9, #TS_PRE_STORM
    b.eq    thunderstorm_next_frame.pre_storm
    cmp     w9, #TS_STORM
    b.eq    thunderstorm_next_frame.storm
    b       thunderstorm_next_frame.update
thunderstorm_next_frame.pre_storm:
    // pre_storm_text_fade
    mov     w1, #4                      // fade
    bl      ts_activate_text
    mov     w9, #TS_WAITING
    STB     w9, ts_phase
    b       thunderstorm_next_frame.update
thunderstorm_next_frame.storm:
    bl      ts_rain
    LDB     w9, ts_strike_in_progress
    cbnz    w9, thunderstorm_next_frame.stepping
    // No strike running: a fresh onset with bass >= 57 starts a bolt now.
    // Between onsets, bass >= 108 uses the reference bass * 0.28 chance.
    // The draw is retained on both routes; ray/branch budgets stay bounded.
    LDB     w9, ts_audio_bass
    mov     w10, #108
    LDB     w11, ts_audio_beat
    cbz     w11, thunderstorm_next_frame.threshold
    mov     w10, #57
thunderstorm_next_frame.threshold:
    cmp     w9, w10
    b.lo    thunderstorm_next_frame.ambient
    ucvtf   d2, w9
    LDD     d3, ts_255
    fdiv    d2, d2, d3
    LDD     d3, ts_chance_quiet
    cbz     w11, thunderstorm_next_frame.chance
    LDD     d3, ts_chance_beat
thunderstorm_next_frame.chance:
    fmul    d2, d2, d3
    // A fresh beat with enough bass and no bolt in progress triggers now;
    // it need not wait for a lucky later frame. Keep the existing draw and
    // all ray/branch budgets; silence keeps the reference RNG sequence.
    bl      rng_random
    cbnz    w11, thunderstorm_next_frame.trigger_audio
    fcmp    d2, d0
    b.ls    thunderstorm_next_frame.ambient
thunderstorm_next_frame.trigger_audio:
    // the rays ladder (up to 10) and the branch budget, floor(bass * 5 / 255)
    mov     w9, #1
    STB     w9, ts_strike_in_progress
    LDB     w9, ts_audio_bass
    mov     w10, #5
    mul     w9, w9, w10
    mov     w10, #255
    udiv    w1, w9, w10                 // branch budget per ray
    LDB     w9, ts_audio_bass
    cmp     w9, #217
    b.hs    thunderstorm_next_frame.rays10
    cmp     w9, #199
    b.hs    thunderstorm_next_frame.rays8
    cmp     w9, #179
    b.hs    thunderstorm_next_frame.rays6
    cmp     w9, #154
    b.hs    thunderstorm_next_frame.rays5
    cmp     w9, #123
    b.hs    thunderstorm_next_frame.rays3
    cmp     w9, #92
    b.hs    thunderstorm_next_frame.rays2
    mov     w0, #1
    b       thunderstorm_next_frame.audio_strike
thunderstorm_next_frame.rays10:
    mov     w0, #10
    b       thunderstorm_next_frame.audio_strike
thunderstorm_next_frame.rays8:
    mov     w0, #8
    b       thunderstorm_next_frame.audio_strike
thunderstorm_next_frame.rays6:
    mov     w0, #6
    b       thunderstorm_next_frame.audio_strike
thunderstorm_next_frame.rays5:
    mov     w0, #5
    b       thunderstorm_next_frame.audio_strike
thunderstorm_next_frame.rays3:
    mov     w0, #3
    b       thunderstorm_next_frame.audio_strike
thunderstorm_next_frame.rays2:
    mov     w0, #2
thunderstorm_next_frame.audio_strike:
    bl      ts_audio_strike
    b       thunderstorm_next_frame.stepping
thunderstorm_next_frame.ambient:
    bl      rng_random
    LDD     d1, ts_strike_chance
    fcmp    d1, d0
    b.le    thunderstorm_next_frame.stepping    // !(random() < 0.008)
    mov     w9, #1
    STB     w9, ts_strike_in_progress
    bl      ts_lightning_strike
thunderstorm_next_frame.stepping:
    LDB     w9, ts_strike_in_progress
    cbz     w9, thunderstorm_next_frame.glowing
    bl      ts_step_strike
thunderstorm_next_frame.glowing:
    mov     x19, #0
thunderstorm_next_frame.glow:
    LDX     x9, ts_glow_count
    cmp     x19, x9
    b.hs    thunderstorm_next_frame.glowed
    LDX     x9, ts_glow
    ldr     w0, [x9, x19, lsl #2]
    bl      active_insert
    add     x19, x19, #1
    b       thunderstorm_next_frame.glow
thunderstorm_next_frame.glowed:
    STX     xzr, ts_glow_count
    bl      clock_monotonic
    LDD     d1, ts_storm_start
    fsub    d0, d0, d1
    LDX     x9, effect_config
    ldr     x9, [x9, #THUNDERSTORM.storm_time]
    scvtf   d1, x9
    fcmp    d0, d1
    b.lt    thunderstorm_next_frame.update
    LDB     w9, ts_strike_in_progress
    cbnz    w9, thunderstorm_next_frame.update
    // post_storm_text_fade_in
    mov     w1, #8                      // unfade
    bl      ts_activate_text
    mov     w9, #TS_COMPLETE
    STB     w9, ts_phase
thunderstorm_next_frame.update:
    bl      update
    mov     w9, #1
    POP2    x19, x30
    ret
thunderstorm_next_frame.finished:
    mov     w9, #0
    POP2    x19, x30
    ret

    .section .rodata
    .balign 8
ts_seven:           .quad 7
ts_six:             .quad 6
ts_half:            .double 0.5
ts_one_half:        .double 1.5
ts_one:             .double 1.0
ts_bright:          .double 1.7
ts_spark_speed_low: .double 0.1
ts_spark_speed_high: .double 0.25
ts_ease_y1:         .double 1.6
ts_ease_y2_low:     .double -0.6
ts_ease_y2_high:    .double 0.4
ts_strike_chance:   .double 0.008
ts_branch_default:  .double 0.05
ts_branch_step:     .double 0.01
ts_255:             .double 255.0
ts_chance_quiet:    .double 0.28        // on_audio's non-beat strike chance
ts_chance_beat:     .double 0.65        // legacy chance; onset overrides its draw
// the strike symbols, in setup_lightning_strike's choice order
ts_strike_symbols:  .quad 0x5c | 1 << 32, 0x2f | 1 << 32, 0x7c | 1 << 32  // \ / |
ts_bg_none:
    .rept 16
    .quad NONE
    .endr

    TSTATE
    .balign 8
ts_rain_pool:       .skip POOL_size
    .balign 8
ts_spark_pool:      .skip POOL_size
    .balign 8
ts_pending:         .skip 8         // u32 lists (see thunderstorm_build)
ts_pending_head:    .skip 8
ts_pending_count:   .skip 8
ts_available:       .skip 8
ts_available_count: .skip 8
ts_active:          .skip 8
ts_active_count:    .skip 8
ts_glow:            .skip 8
ts_glow_count:      .skip 8
ts_text:            .skip 8
ts_text_count:      .skip 8
ts_delay:           .skip 8
ts_progression_delay: .skip 8
ts_branch_chance:   .skip 8
ts_storm_start:     .skip 8
ts_final_spectrum:  .skip 8
ts_final_map:       .skip 8
ts_final_width:     .skip 8
ts_symbol:          .skip 8
ts_stops:           .skip 8 * 3
ts_spark_spectrum:  .skip 8 * 16
ts_spark_len:       .skip 4
ts_flash_ease:      .skip 4
ts_strike_flash_colors: .skip 8 * 16
ts_strike_fade_colors:  .skip 8 * 16
ts_strike_flash_handles: .skip 4 * 3 * TS_FLASH_LEN
ts_strike_fade_handles:  .skip 4 * 3 * TS_STRIKE_FADE_LEN
    .balign 8
ts_tc_fg:           .skip 8
ts_tc_bg:           .skip 8
ts_storm_fg:        .skip 8
ts_storm_bg:        .skip 8
ts_bg_storm:        .skip 8 * 16
ts_glow_colors:     .skip 8 * 16
ts_fade_colors:     .skip 8 * 16
ts_unfade_colors:   .skip 8 * 16
ts_flash_colors:    .skip 8 * 16
ts_fade_bg:         .skip 8 * 16
ts_unfade_bg:       .skip 8 * 16
ts_tc_valid:        .skip 1
ts_phase:           .skip 1
ts_strike_in_progress: .skip 1
ts_audio_volume:    .skip 1         // poll byte 0 (color/intensity reference)
ts_audio_bass:      .skip 1         // poll byte 1: strikes (graves)
ts_audio_beat:      .skip 1         // rising edge of poll byte 2
ts_audio_beat_previous: .skip 1
ts_audio_treble:    .skip 1         // poll byte 3: raindrops (agudos)
ts_audio_bpm:       .skip 1         // poll byte 4: cadence
    .balign 8
ts_audio_branch_budget: .skip 8     // -1 uncapped, 0 blocks, else remaining

    .text
