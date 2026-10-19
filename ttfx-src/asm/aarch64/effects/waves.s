// effects/waves.s - "Waves travel across the terminal leaving behind the
// characters" (src/effects/waves.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Waves).
//
// Every character gets an eased "wave" scene (auto id 0) and a "final" scene
// (auto id 1), activated by the wave's SCENE_COMPLETE. The wave scene is the
// same frame list for every character: the first character whose scene has no
// preexisting colors builds it with apply_gradient_to_symbols, exactly as Rust,
// and the others copy its frames (visual handle, duration) straight from it.
// No RNG is drawn, so the character order is free.

.equ HAVE_waves, 1

.equ WAVES.symbols,             0       // *const u64 packed symbols
.equ WAVES.symbol_count,        8
.equ WAVES.wave_stops,          16      // *const u64
.equ WAVES.wave_stop_count,     24
.equ WAVES.wave_steps,          32      // *const i64
.equ WAVES.wave_step_count,     40
.equ WAVES.wave_count,          48      // >= 1
.equ WAVES.wave_length,         56      // frame duration, fits i32 (marshal checks)
.equ WAVES.direction,           64      // CharacterGroup (GROUP_*)
.equ WAVES.ease,                72      // easing id
.equ WAVES.final_stops,         80      // *const u64
.equ WAVES.final_stop_count,    88
.equ WAVES.final_steps,         96      // *const i64
.equ WAVES.final_step_count,    104
.equ WAVES.final_direction,     112
.equ WAVES_size,                120

.equ WV_WAVE,                   0       // auto scene ids: new_scene("") twice
.equ WV_FINAL,                  1
.equ WV_FINAL_DURATION,         10

    .text

// waves_build: Waves::build.
waves_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    bl      wv_final_map
    // the wave gradient
    ldr     x0, [x19, #WAVES.wave_steps]
    ldr     x3, [x19, #WAVES.wave_step_count]
    ldr     x1, [x19, #WAVES.wave_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, wv_wave_spectrum
    ldr     x0, [x19, #WAVES.wave_stops]
    ldr     x1, [x19, #WAVES.wave_stop_count]
    ldr     x2, [x19, #WAVES.wave_steps]
    ldr     x3, [x19, #WAVES.wave_step_count]
    LDX     x4, wv_wave_spectrum
    bl      gradient_new
    mov     w9, w9
    STX     x9, wv_wave_len
    // the final scene gradients start at the wave spectrum's last color
    LDX     x3, wv_wave_spectrum
    add     x3, x3, x9, lsl #3
    ldur    x9, [x3, #-8]
    STX     x9, wv_pair_stops
    ldr     x0, [x19, #WAVES.final_steps]
    ldr     x3, [x19, #WAVES.final_step_count]
    mov     w1, #2
    bl      gradient_capacity
    lsl     x9, x9, #3
    STX     x9, wv_pair_bytes
    mov     x0, x9
    bl      alloc
    STX     x9, wv_pair_spectrum
    LDX     x0, wv_pair_bytes
    bl      alloc
    STX     x9, wv_bg_spectrum
    mov     x9, #NONE
    STX     x9, wv_last_fg
    LDX     x21, input_chars
    LDX     x22, input_count
    mov     x20, #0
waves_build.char:
    cmp     x20, x22
    b.hs    waves_build.grouped
    ldr     w24, [x21, x20, lsl #2]     // slot
    // --- the eased wave scene: the same for every character whose input
    // colors don't enter it, so later ones clone the first one's
    LDX     x9, wv_template_scene
    cbz     x9, waves_build.wave_fresh
    LDX     x3, cfg_existing_colors
    cbnz    x3, waves_build.wave_clone
    LDX     x3, ch_flags
    ldrh    w3, [x3, x24, lsl #1]
    tst     w3, #CF_PREEXISTING
    b.ne    waves_build.wave_fresh
waves_build.wave_clone:
    mov     w0, w24
    sub     w1, w9, #1
    mov     w2, #WV_WAVE
    bl      scene_copy
    mov     w23, w9
    b       waves_build.final
waves_build.wave_fresh:
    mov     w0, w24
    mov     w1, #WV_WAVE
    mov     w2, #0
    ldr     w3, [x19, #WAVES.ease]
    bl      scene_new
    mov     w23, w9
    mov     w0, w9
    bl      wv_wave_frames
    LDX     x9, wv_template_scene
    cbnz    x9, waves_build.final
    SCENE_PTR x9, x23
    ldr     w3, [x9, #SC_FLAGS]
    tst     w3, #(SCF_PREEXISTING | SCF_PRE_BOLD)
    b.ne    waves_build.final
    add     w9, w23, #1
    STX     x9, wv_template_scene
waves_build.final:
    // --- the final scene
    mov     w0, w24
    mov     w1, #WV_FINAL
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    LDX     x3, cfg_existing_colors
    cmp     x3, #1
    b.eq    waves_build.dynamic
    mov     w0, w9
    bl      wv_final_frames
    b       waves_build.events
waves_build.dynamic:
    mov     w0, w9
    bl      wv_dynamic_frames
waves_build.events:
    mov     w0, w24
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    mov     w3, #WV_WAVE
    mov     w4, #ACT_ACTIVATE_SCENE
    mov     w5, #WV_FINAL
    mov     x6, #0
    bl      event_register
    mov     w0, w24
    mov     w1, w23
    bl      scene_activate
    LDX     x3, cfg_existing_colors
    cmp     x3, #1
    b.ne    waves_build.next
    // dynamic: show the input colors until the wave arrives
    mov     w0, w24
    mov     x1, #0
    LDX     x2, ch_fg
    ldr     x2, [x2, x24, lsl #3]
    LDX     x3, ch_bg
    ldr     x3, [x3, x24, lsl #3]
    bl      set_appearance
waves_build.next:
    add     x20, x20, #1
    b       waves_build.char
waves_build.grouped:
    mov     w0, #FILTER_INPUT
    ldr     w1, [x19, #WAVES.direction]
    bl      get_characters_grouped
    STX     x9, wv_groups
    STX     x2, wv_group_count
    STX     xzr, wv_group_pos
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// wv_wave_frames(w0=scene), x19 = config: wave_count times
// apply_gradient_to_symbols(wave symbols, wave_length, wave gradient).
wv_wave_frames:
    PUSH2   x20, x21
    PUSH2   x22, x30
    mov     w20, w0
    ldr     x21, [x19, #WAVES.wave_count]
wv_wave_frames.wave:
    cbz     x21, wv_wave_frames.done
    mov     x6, #0                      // no bg gradient
    mov     x7, #0
    mov     w0, w20
    ldr     x1, [x19, #WAVES.symbols]
    ldr     x2, [x19, #WAVES.symbol_count]
    ldr     w3, [x19, #WAVES.wave_length]
    LDX     x4, wv_wave_spectrum
    LDX     x5, wv_wave_len
    bl      scene_apply_gradient
    sub     x21, x21, #1
    b       wv_wave_frames.wave
wv_wave_frames.done:
    POP2    x22, x30
    POP2    x20, x21
    ret

// wv_final_frames(w0=scene) with x24 = slot, x19 = config: one frame of
// the input symbol per color of Gradient([wave last, final color], final
// steps), duration 10. The gradient is rebuilt only when the final color
// changes.
wv_final_frames:
    PUSH2   x20, x21
    PUSH2   x22, x30
    mov     w20, w0
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x24, lsl #2]
    LDX     x3, text_bottom
    sub     x9, x9, x3
    LDX     x3, wv_map_width
    mul     x9, x9, x3
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x24, lsl #2]
    add     x9, x9, x3
    LDX     x3, text_left
    sub     x9, x9, x3
    LDX     x3, wv_map
    ldr     x9, [x3, x9, lsl #3]
    LDX     x3, wv_last_fg
    cmp     x9, x3
    b.eq    wv_final_frames.frames
    STX     x9, wv_last_fg
    ADRG    x0, wv_pair_stops
    str     x9, [x0, #8]
    mov     w1, #2
    ldr     x2, [x19, #WAVES.final_steps]
    ldr     x3, [x19, #WAVES.final_step_count]
    LDX     x4, wv_pair_spectrum
    bl      gradient_new
    mov     w9, w9
    STX     x9, wv_pair_len
wv_final_frames.frames:
    mov     x21, #0
wv_final_frames.color:
    LDX     x9, wv_pair_len
    cmp     x21, x9
    b.hs    wv_final_frames.done
    LDX     x3, wv_pair_spectrum
    ldr     x3, [x3, x21, lsl #3]
    mov     x4, #NONE
    LDX     x1, ch_sym
    ldr     x1, [x1, x24, lsl #3]
    mov     w0, w20
    mov     w2, #WV_FINAL_DURATION
    mov     w5, #0
    bl      scene_add_frame
    add     x21, x21, #1
    b       wv_final_frames.color
wv_final_frames.done:
    POP2    x22, x30
    POP2    x20, x21
    ret

// wv_dynamic_frames(w0=scene) with x24 = slot, x19 = config: the final
// scene under --existing-color-handling dynamic, from the input colors.
wv_dynamic_frames:
    PUSH2   x20, x21
    PUSH2   x22, x23
    PUSH1   x30
    mov     w20, w0
    LDX     x21, ch_fg
    ldr     x21, [x21, x24, lsl #3]     // fg or NONE
    LDX     x22, ch_bg
    ldr     x22, [x22, x24, lsl #3]     // bg or NONE
    LDX     x9, ch_sym
    ldr     x9, [x9, x24, lsl #3]
    STX     x9, wv_symbol
    cmn     x21, #1                     // NONE
    b.ne    wv_dynamic_frames.gradients
    cmn     x22, #1
    b.ne    wv_dynamic_frames.gradients
    // no colors: one plain frame
    mov     w0, w20
    mov     x1, x9
    mov     w2, #WV_FINAL_DURATION
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       wv_dynamic_frames.done
wv_dynamic_frames.gradients:
    mov     w23, #0                     // fg spectrum length (0 = None)
    cmn     x21, #1
    b.eq    wv_dynamic_frames.bg
    ADRG    x0, wv_pair_stops
    str     x21, [x0, #8]
    mov     w1, #2
    ldr     x2, [x19, #WAVES.final_steps]
    ldr     x3, [x19, #WAVES.final_step_count]
    LDX     x4, wv_pair_spectrum
    bl      gradient_new
    mov     w23, w9
    mov     x9, #NONE                   // the pair spectrum no longer holds it
    STX     x9, wv_last_fg
wv_dynamic_frames.bg:
    mov     w9, #0
    cmn     x22, #1
    b.eq    wv_dynamic_frames.apply
    ADRG    x0, wv_pair_stops
    str     x22, [x0, #8]
    mov     w1, #2
    ldr     x2, [x19, #WAVES.final_steps]
    ldr     x3, [x19, #WAVES.final_step_count]
    LDX     x4, wv_bg_spectrum
    bl      gradient_new
wv_dynamic_frames.apply:
    // apply_gradient_to_symbols([symbol], 10, fg gradient, bg gradient)
    mov     w5, #0
    mov     x4, #0
    cbz     w23, wv_dynamic_frames.bg_args
    mov     w5, w23
    LDX     x4, wv_pair_spectrum
wv_dynamic_frames.bg_args:
    mov     x6, #0
    cbz     w9, wv_dynamic_frames.call
    LDX     x6, wv_bg_spectrum
wv_dynamic_frames.call:
    mov     w7, w9                      // bg count
    mov     w0, w20
    ADRG    x1, wv_symbol
    mov     w2, #1
    mov     w3, #WV_FINAL_DURATION
    bl      scene_apply_gradient
    cmn     x21, #1
    b.ne    wv_dynamic_frames.done
    // no fg: a last frame of the bg alone
    mov     w0, w20
    LDX     x1, wv_symbol
    mov     w2, #WV_FINAL_DURATION
    mov     x3, #NONE
    mov     x4, x22
    mov     w5, #0
    bl      scene_add_frame
wv_dynamic_frames.done:
    POP1    x30
    POP2    x22, x23
    POP2    x20, x21
    ret

// wv_final_map (x19 = config): Gradient::new(final stops, final steps) and
// its coordinate mapping over the text rectangle.
wv_final_map:
    PUSH1   x30
    ldr     x0, [x19, #WAVES.final_steps]
    ldr     x3, [x19, #WAVES.final_step_count]
    ldr     x1, [x19, #WAVES.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, wv_final_spectrum
    ldr     x0, [x19, #WAVES.final_stops]
    ldr     x1, [x19, #WAVES.final_stop_count]
    ldr     x2, [x19, #WAVES.final_steps]
    ldr     x3, [x19, #WAVES.final_step_count]
    LDX     x4, wv_final_spectrum
    bl      gradient_new
    LDX     x0, wv_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, wv_map_width
    ldr     x6, [x19, #WAVES.final_direction]
    bl      gradient_map
    STX     x9, wv_map
    POP1    x30
    ret

// waves_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
waves_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x9, wv_group_pos
    LDX     x3, wv_group_count
    cmp     x9, x3
    b.lo    waves_next_frame.reveal
    bl      active_empty
    cbnz    w9, waves_next_frame.finished
    b       waves_next_frame.update
waves_next_frame.reveal:
    // the next group: visible and active
    add     x3, x9, #1
    STX     x3, wv_group_pos
    LDX     x3, wv_groups
    add     x9, x3, x9, lsl #4
    ldr     x19, [x9]                   // slots
    ldr     x21, [x9, #8]               // count
waves_next_frame.char:
    cbz     x21, waves_next_frame.update
    ldr     w22, [x19]
    mov     w0, w22
    bl      set_visible
    mov     w0, w22
    bl      active_insert
    add     x19, x19, #4
    sub     x21, x21, #1
    b       waves_next_frame.char
waves_next_frame.update:
    bl      update
    mov     w9, #1
    b       waves_next_frame.out
waves_next_frame.finished:
    mov     w9, #0
waves_next_frame.out:
    POP2    x22, x30
    POP2    x19, x21
    ret

    TSTATE
    .balign 8
wv_groups:              .skip 8
wv_group_count:         .skip 8
wv_group_pos:           .skip 8
wv_final_spectrum:      .skip 8
wv_map:                 .skip 8
wv_map_width:           .skip 8
wv_wave_spectrum:       .skip 8
wv_wave_len:            .skip 8
wv_template_scene:      .skip 8         // the wave scene to clone, + 1 (0 = none yet)
wv_pair_stops:          .skip 8 * 2
wv_pair_bytes:          .skip 8
wv_pair_spectrum:       .skip 8
wv_pair_len:            .skip 8
wv_bg_spectrum:         .skip 8
wv_last_fg:             .skip 8
wv_symbol:              .skip 8

    .text
