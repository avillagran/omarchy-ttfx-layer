// effects/rain.s - "Rain characters from the top of the canvas"
// (src/effects/rain.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Rain):
//
// Every character gets its scenes, path and event in Rust's order, so every
// RNG draw lines up. group_by_row is the TopToBottomLeftToRight character list
// walked backwards one row at a time: Rust's stable sort by ascending row keeps
// each row's left-to-right order, and the BTreeMap pops the lowest row first.

.equ HAVE_rain, 1

.equ RAIN.colors,               0       // *const u64
.equ RAIN.color_count,          8
.equ RAIN.speed_min,            16      // f64
.equ RAIN.speed_max,            24      // f64
.equ RAIN.symbols,              32      // *const u64 (packed)
.equ RAIN.symbol_count,         40
.equ RAIN.final_stops,          48      // *const u64
.equ RAIN.final_stop_count,     56
.equ RAIN.final_steps,          64      // *const i64
.equ RAIN.final_step_count,     72
.equ RAIN.final_direction,      80
.equ RAIN.easing,               88
.equ RAIN_size,                 96

// A fresh character's auto ids: its first scene is "0", its second "1", its
// first path "0". Naming them explicitly is the same as AUTO here.
.equ RAIN_SCENE,                0
.equ RAIN_FADE,                 1
.equ RAIN_PATH,                 0

    .text

// rain_build: Rain::build.
// Locals: [sp, #56] the rain scene, then the speed.
rain_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    // memo of (rain color, rain symbol) -> handle
    ldr     x0, [x19, #RAIN.color_count]
    ldr     x9, [x19, #RAIN.symbol_count]
    mul     x0, x0, x9
    lsl     x0, x0, #2
    bl      alloc
    STX     x9, rain_memo
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    cset    w9, eq
    STB     w9, rain_dynamic
    cbnz    w9, rain_build.characters   // dynamic ignores the final gradient
    bl      rain_final_color_map
rain_build.characters:
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    STX     x9, rain_chars
    STX     x2, rain_group_end
    mov     x21, x9
    mov     x22, x2
    lsl     x0, x2, #2
    add     x0, x0, #4
    bl      alloc
    STX     x9, rain_pending
    mov     x19, #0
rain_build.char:
    cmp     x19, x22
    b.hs    rain_build.built
    ldr     w23, [x21, x19, lsl #2]     // slot
    // raindrop_color = choice(rain_colors)
    LDX     x9, effect_config
    ldr     x0, [x9, #RAIN.color_count]
    bl      rng_below
    mov     w24, w9
    mov     w0, w23
    mov     w1, #RAIN_SCENE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    // rain_symbol = choice(rain_symbols); one frame of duration 1
    LDX     x9, effect_config
    ldr     x0, [x9, #RAIN.symbol_count]
    bl      rng_below
    mov     w1, w9
    mov     w9, w24
    bl      rain_memo_visual
    mov     w1, w9
    mov     w0, w20
    mov     w2, #1
    bl      scene_add_frame_visual
    str     w20, [sp, #56]              // rain scene
    mov     w0, w23
    mov     w1, #RAIN_FADE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w0, w9
    mov     w1, w23
    mov     w2, w24
    bl      rain_fade_scene
    mov     w0, w23
    ldr     w1, [sp, #56]
    bl      scene_activate
    // speed = uniform(movement_speed); start at the top of the canvas
    LDX     x9, effect_config
    ldr     d0, [x9, #RAIN.speed_min]
    ldr     d1, [x9, #RAIN.speed_max]
    bl      rng_uniform
    str     d0, [sp, #56]
    LDX     x1, canvas_top
    lsl     x1, x1, #32
    LDX     x9, ch_icol
    ldr     w9, [x9, x23, lsl #2]
    orr     x1, x1, x9
    mov     w0, w23
    bl      set_coordinate
    mov     w0, w23
    ldr     d0, [sp, #56]
    LDX     x9, effect_config
    ldr     w1, [x9, #RAIN.easing]
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #RAIN_PATH
    bl      path_new
    mov     w20, w9
    mov     w0, w23
    bl      char_input_coord
    mov     x1, x9
    mov     w0, w20
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // PathComplete(input path) -> ActivateScene(fade)
    mov     w0, w23
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w3, #RAIN_PATH
    mov     w4, #ACT_ACTIVATE_SCENE
    mov     w5, #RAIN_FADE
    mov     x6, #0
    bl      event_register
    mov     w0, w23
    mov     w1, w20
    bl      path_activate
    add     x19, x19, #1
    b       rain_build.char
rain_build.built:
    STX     xzr, rain_pending_len
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// rain_fade_scene(w0=fade scene, w1=slot, w2=raindrop color index): the
// fade from the raindrop color to the final colors over the input symbol, 3
// ticks per color.
rain_fade_scene:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    stp     x23, x30, [sp, #32]
    mov     w21, w0
    mov     w22, w1
    LDX     x9, effect_config
    ldr     x9, [x9, #RAIN.colors]
    ldr     x23, [x9, w2, uxtw #3]      // raindrop color
    LDB     w9, rain_dynamic
    cbnz    w9, rain_fade_scene.dynamic
    // with_steps([raindrop, final_gradient_mapping[input_coord]], 7)
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x22, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, rain_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x22, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, rain_map
    ldr     x19, [x3, x9, lsl #3]
    mov     x20, #NONE
    b       rain_fade_scene.gradients
rain_fade_scene.dynamic:
    LDX     x9, ch_fg
    ldr     x19, [x9, x22, lsl #3]
    LDX     x9, ch_bg
    ldr     x20, [x9, x22, lsl #3]
    and     x9, x19, x20
    cmn     x9, #1                      // NONE
    b.ne    rain_fade_scene.gradients
    // neither input color: the input symbol with no colors
    mov     w0, w21
    LDX     x1, ch_sym
    ldr     x1, [x1, x22, lsl #3]
    mov     w2, #3
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       rain_fade_scene.done
rain_fade_scene.gradients:
    cmn     x19, #1                     // NONE
    b.eq    rain_fade_scene.bg
    ADRG    x0, rain_pair
    stp     x23, x19, [x0]
    mov     w1, #2
    ADRG    x2, rain_seven
    mov     w3, #1
    ADRG    x4, rain_fg_spectrum
    bl      gradient_new
    STX     x9, rain_fg_len
rain_fade_scene.bg:
    cmn     x20, #1                     // NONE
    b.eq    rain_fade_scene.apply
    ADRG    x0, rain_pair
    stp     x23, x20, [x0]
    mov     w1, #2
    ADRG    x2, rain_seven
    mov     w3, #1
    ADRG    x4, rain_bg_spectrum
    bl      gradient_new
    STX     x9, rain_bg_len
rain_fade_scene.apply:
    LDX     x9, ch_sym
    add     x1, x9, x22, lsl #3         // [input symbol]
    mov     w0, w21
    mov     w2, #1
    mov     w3, #3
    mov     x4, #0
    mov     x5, #0
    cmn     x19, #1                     // NONE
    b.eq    rain_fade_scene.no_fg
    ADRG    x4, rain_fg_spectrum
    LDX     x5, rain_fg_len
rain_fade_scene.no_fg:
    mov     x6, #0
    mov     x7, #0
    cmn     x20, #1                     // NONE
    b.eq    rain_fade_scene.no_bg
    ADRG    x6, rain_bg_spectrum
    LDX     x7, rain_bg_len
rain_fade_scene.no_bg:
    bl      scene_apply_gradient
rain_fade_scene.done:
    ldp     x23, x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// rain_memo_visual(w9=color index, w1=symbol index) -> w9 = handle.
rain_memo_visual:
    PUSH2   x19, x30
    LDX     x3, effect_config
    ldr     w19, [x3, #RAIN.symbol_count]
    mul     w19, w19, w9
    add     w19, w19, w1
    LDX     x2, rain_memo
    ldr     w2, [x2, x19, lsl #2]
    cbnz    w2, rain_memo_visual.hit
    ldr     x2, [x3, #RAIN.colors]
    ldr     x0, [x2, w9, uxtw #3]
    ldr     x2, [x3, #RAIN.symbols]
    ldr     x2, [x2, w1, uxtw #3]
    mov     x1, #NONE
    mov     w3, #0
    bl      visual_make
    LDX     x3, rain_memo
    str     w9, [x3, x19, lsl #2]
    mov     w2, w9
rain_memo_visual.hit:
    mov     w9, w2
    POP2    x19, x30
    ret

// rain_final_color_map: Gradient::new(final stops, final steps) and
// build_coordinate_color_mapping over the text rectangle.
rain_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #RAIN.final_steps]
    ldr     x3, [x19, #RAIN.final_step_count]
    ldr     x1, [x19, #RAIN.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, rain_spectrum
    ldr     x0, [x19, #RAIN.final_stops]
    ldr     x1, [x19, #RAIN.final_stop_count]
    ldr     x2, [x19, #RAIN.final_steps]
    ldr     x3, [x19, #RAIN.final_step_count]
    LDX     x4, rain_spectrum
    bl      gradient_new
    LDX     x0, rain_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, rain_map_width
    ldr     x6, [x19, #RAIN.final_direction]
    bl      gradient_map
    STX     x9, rain_map
    POP2    x19, x30
    ret

// rain_next_frame -> w9 = 1 for a frame, 0 when done. Rain::next_frame.
rain_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    // audio channels (audio_frame_poll in lib.s): bass byte 1, beat byte 2,
    // treble byte 3, bpm byte 4; all zero without audio (reference behavior).
    LDX     x9, audio_word
    ubfx    w10, w9, #8, #8
    STB     w10, rain_a_bass
    ubfx    x10, x9, #32, #8            // bpm is byte 4, above the w9 half
    STB     w10, rain_a_bpm
    ubfx    w10, w9, #16, #8
    STB     w10, rain_a_beat
    ubfx    w10, w9, #24, #8
    STB     w10, rain_a_treble
    LDX     x9, rain_pending_len
    cbnz    x9, rain_next_frame.release
    LDX     x9, rain_group_end
    cbnz    x9, rain_next_frame.next_row
    bl      active_empty
    cbnz    w9, rain_next_frame.finished
    b       rain_next_frame.tick
rain_next_frame.next_row:
    // pending_chars.extend(group_by_row.pop_first())
    LDX     x2, rain_chars
    LDX     x3, rain_group_end
    LDX     x4, ch_irow
    sub     x9, x3, #1
    ldr     w9, [x2, x9, lsl #2]
    ldr     w5, [x4, x9, lsl #2]        // the lowest remaining row
    sub     x9, x3, #1
rain_next_frame.row_start:
    cbz     x9, rain_next_frame.copy
    sub     x10, x9, #1
    ldr     w10, [x2, x10, lsl #2]
    ldr     w10, [x4, x10, lsl #2]
    cmp     w10, w5
    b.ne    rain_next_frame.copy
    sub     x9, x9, #1
    b       rain_next_frame.row_start
rain_next_frame.copy:
    STX     x9, rain_group_end
    sub     x3, x3, x9
    STX     x3, rain_pending_len
    add     x1, x2, x9, lsl #2
    LDX     x0, rain_pending
    REP_MOVSD
rain_next_frame.release:
    // bpm cadence: no audio releases every frame (the reference); with a
    // tempo, releases happen every 180/bpm frames (>= 180 BPM every frame,
    // 45 BPM every fourth).
    LDB     w9, rain_a_bpm
    cbz     w9, rain_next_frame.cadence_ok
    cmp     w9, #45
    b.hs    rain_next_frame.bpm_ok
    mov     w9, #45
rain_next_frame.bpm_ok:
    mov     w10, #180
    udiv    w10, w10, w9
    cbnz    w10, rain_next_frame.cadence_n
    mov     w10, #1
rain_next_frame.cadence_n:
    LDX     x9, rain_cadence
    add     x9, x9, #1
    STX     x9, rain_cadence
    cmp     x9, x10
    b.lo    rain_next_frame.tick
    STX     xzr, rain_cadence
rain_next_frame.cadence_ok:
    // treble drives the drop count: silence keeps the reference 1-2 draw;
    // with audio it is deterministic, 1 + treble * 5 / 255 (1-6).
    LDB     w9, rain_a_treble
    cbz     w9, rain_next_frame.classic
    mov     w10, #5
    mul     w9, w9, w10
    mov     w10, #255
    udiv    w9, w9, w10
    add     x21, x9, #1
    b       rain_next_frame.burst
rain_next_frame.classic:
    mov     x0, #1
    mov     x1, #2
    bl      rng_randint
    mov     x21, x9
rain_next_frame.burst:
    // a bass beat adds a 3-drop burst (graves are the impact channel)
    LDB     w9, rain_a_beat
    cbz     w9, rain_next_frame.drop
    LDB     w9, rain_a_bass
    cmp     w9, #57
    b.lo    rain_next_frame.drop
    add     x21, x21, #3
rain_next_frame.drop:
    cbz     x21, rain_next_frame.tick
    sub     x21, x21, #1
    LDX     x22, rain_pending_len
    cbz     x22, rain_next_frame.tick
    mov     x0, #0
    sub     x1, x22, #1
    bl      rng_randint
    // pending_chars.remove(index): a forward copy down one slot
    LDX     x0, rain_pending
    add     x0, x0, x9, lsl #2
    ldr     w19, [x0]
    add     x1, x0, #4
    sub     x3, x22, #1
    sub     x3, x3, x9
    REP_MOVSD
    LDX     x9, rain_pending_len
    sub     x9, x9, #1
    STX     x9, rain_pending_len
    mov     w0, w19
    bl      set_visible
    mov     w0, w19
    bl      active_insert
    b       rain_next_frame.drop
rain_next_frame.tick:
    bl      update
    mov     w9, #1
    b       rain_next_frame.out
rain_next_frame.finished:
    mov     w9, #0
rain_next_frame.out:
    POP2    x22, x30
    POP2    x19, x21
    ret

    .section .rodata
    .balign 8
rain_seven:         .quad 7

    TSTATE
    .balign 8
rain_memo:          .skip 8
rain_spectrum:      .skip 8
rain_map:           .skip 8
rain_map_width:     .skip 8
rain_chars:         .skip 8         // TopToBottomLeftToRight input characters
rain_group_end:     .skip 8         // rows not yet pending: rain_chars[..end]
rain_pending:       .skip 8
rain_pending_len:   .skip 8
rain_fg_len:        .skip 8
rain_bg_len:        .skip 8
rain_pair:          .skip 16
rain_fg_spectrum:   .skip 8*16
rain_bg_spectrum:   .skip 8*16
rain_dynamic:       .skip 1
rain_a_bass:        .skip 1         // poll byte 1: beat bursts (graves)
rain_a_beat:        .skip 1         // poll byte 2: beat flag
rain_a_treble:      .skip 1         // poll byte 3: drops per release (agudos)
rain_a_bpm:         .skip 1         // poll byte 4: release cadence
    .balign 8
rain_cadence:       .skip 8         // frames since the last release
