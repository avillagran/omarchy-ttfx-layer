// effects/randomsequence.s - "Prints the input data in a random sequence"
// (src/effects/random_sequence.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Randomsequence):

.equ HAVE_randomsequence, 1

.equ RANDOMSEQUENCE.speed,              0       // f64
.equ RANDOMSEQUENCE.final_stops,        8       // *const u64
.equ RANDOMSEQUENCE.final_stop_count,   16
.equ RANDOMSEQUENCE.final_steps,        24      // *const i64
.equ RANDOMSEQUENCE.final_step_count,   32
.equ RANDOMSEQUENCE.final_frames,       40      // fits an i32 (marshal checks)
.equ RANDOMSEQUENCE.final_direction,    48
.equ RANDOMSEQUENCE_size,               56

.equ RS_FADE_LEN,           8           // Gradient::with_steps(2 stops, 7)
.equ RS_SCENE,              NAME_LITERAL + 0    // new_scene(.., "")

    .text

// randomsequence_build: RandomSequence::build.
// Locals: [sp, #56] the character's input fg.
randomsequence_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    // characters_per_tick = max(int(speed * len(input_characters)), 1)
    LDX     x9, input_count
    scvtf   d0, x9
    ldr     d1, [x19, #RANDOMSEQUENCE.speed]
    fmul    d0, d0, d1
    bl      f64_to_i64
    mov     w3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    STX     x9, rs_per_tick
    LDX     x9, request
    ldr     x9, [x9, #RQ_BACKGROUND]
    STX     x9, rs_pair                 // every fade starts at the background
    bl      rs_final_color_map
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    STX     x9, rs_pending              // a fresh array: it becomes pending
    STX     x2, rs_pending_count
    mov     x21, x9
    mov     x22, x2
    mov     x23, #0
randomsequence_build.char:
    cmp     x23, x22
    b.hs    randomsequence_build.shuffle
    ldr     w20, [x21, x23, lsl #2]     // slot
    mov     w0, w20
    mov     w1, #0
    bl      set_visibility
    mov     w0, w20
    MOV64   w1, RS_SCENE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9                     // scene
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    randomsequence_build.dynamic
    // the final gradient's color at the input coordinate, faded in from
    // the background
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x20, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, rs_final_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x20, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, rs_final_map
    ldr     x19, [x3, x9, lsl #3]
    // the fade's frames: visuals shared by symbol and final color
    LDX     x0, ch_sym
    ldr     x0, [x0, x20, lsl #3]
    mov     x1, x19
    mov     x2, #NONE
    bl      visual_run_find
    cbnz    x9, randomsequence_build.fade_frames
    mov     x0, x19
    bl      rs_fade
    LDX     x0, ch_sym
    ldr     x0, [x0, x20, lsl #3]
    mov     x1, x9
    mov     w2, #RS_FADE_LEN
    mov     x3, #NONE
    mov     x4, x19
    bl      visual_run
randomsequence_build.fade_frames:
    mov     w0, w24
    mov     x1, x9
    LDX     x3, effect_config
    ldr     w3, [x3, #RANDOMSEQUENCE.final_frames]
    bl      visual_frames
randomsequence_build.activate:
    mov     w0, w20
    mov     w1, w24
    bl      scene_activate
    add     x23, x23, #1
    b       randomsequence_build.char
randomsequence_build.dynamic:
    // ExistingColorHandling::Dynamic: fade to the input colors
    LDX     x9, ch_fg
    ldr     x9, [x9, x20, lsl #3]
    str     x9, [sp, #56]
    LDX     x9, ch_bg
    ldr     x0, [x9, x20, lsl #3]
    cmn     x0, #1                      // NONE
    b.ne    randomsequence_build.dyn_bg
    ldr     x9, [sp, #56]
    cmn     x9, #1                      // NONE
    b.eq    randomsequence_build.neutral
    mov     x2, #0
    b       randomsequence_build.dyn_fg
randomsequence_build.dyn_bg:
    bl      rs_fade
    ADRG    x0, rs_bg_fade
    mov     x3, #RS_FADE_LEN
    mov     x1, x9
    REP_MOVSQ
    ADRG    x2, rs_bg_fade
randomsequence_build.dyn_fg:
    mov     x9, #0
    ldr     x0, [sp, #56]
    cmn     x0, #1                      // NONE
    b.eq    randomsequence_build.dyn_apply
    PUSH1   x2
    bl      rs_fade
    POP1    x2
randomsequence_build.dyn_apply:
    mov     x3, x2                      // bg fade or 0
    mov     x2, x9                      // fg fade or 0
    mov     w0, w24
    mov     w1, w20
    bl      rs_add_fade
    b       randomsequence_build.activate
randomsequence_build.neutral:
    MOV64   x0, 0x808080                // DYNAMIC_NEUTRAL_GRAY
    bl      rs_fade
    mov     w0, w24
    mov     w1, w20
    mov     x2, x9
    mov     x3, #0
    bl      rs_add_fade
    mov     w0, w24
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    LDX     x2, effect_config
    ldr     w2, [x2, #RANDOMSEQUENCE.final_frames]
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       randomsequence_build.activate
randomsequence_build.shuffle:
    mov     x0, x21
    mov     x1, x22
    bl      rng_shuffle32
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// rs_fade(x0=color) -> x9 = Gradient::with_steps([background, color], 7)
// spectrum (RS_FADE_LEN colors, in rs_fade_spectrum; reused by each call).
rs_fade:
    PUSH1   x30
    ADRG    x9, rs_pair
    str     x0, [x9, #8]
    mov     x0, x9
    mov     w1, #2
    ADRG    x2, rs_seven
    mov     w3, #1
    ADRG    x4, rs_fade_spectrum
    bl      gradient_new
    ADRG    x9, rs_fade_spectrum
    POP1    x30
    ret

// rs_add_fade(w0=scene, w1=slot, x2=fg spectrum or 0, x3=bg spectrum or
// 0): apply_gradient_to_symbols([input symbol], frames, fg, bg). Both
// spectra have RS_FADE_LEN colors, so the cyclic distribution pairs them
// index by index and gives each pair one frame of the single symbol.
rs_add_fade:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w19, w0
    LDX     x9, ch_sym
    ldr     x21, [x9, w1, uxtw #3]
    mov     x22, x2
    mov     x23, x3
    LDX     x9, effect_config
    ldr     w24, [x9, #RANDOMSEQUENCE.final_frames]
    mov     w20, #0
rs_add_fade.frame:
    mov     x3, #NONE
    cbz     x22, rs_add_fade.bg
    ldr     x3, [x22, x20, lsl #3]
rs_add_fade.bg:
    mov     x4, #NONE
    cbz     x23, rs_add_fade.add
    ldr     x4, [x23, x20, lsl #3]
rs_add_fade.add:
    mov     w0, w19
    mov     x1, x21
    mov     w2, w24
    mov     w5, #0
    bl      scene_add_frame
    add     w20, w20, #1
    cmp     w20, #RS_FADE_LEN
    b.lo    rs_add_fade.frame
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// rs_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
rs_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #RANDOMSEQUENCE.final_steps]
    ldr     x3, [x19, #RANDOMSEQUENCE.final_step_count]
    ldr     x1, [x19, #RANDOMSEQUENCE.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, rs_final_spectrum
    ldr     x0, [x19, #RANDOMSEQUENCE.final_stops]
    ldr     x1, [x19, #RANDOMSEQUENCE.final_stop_count]
    ldr     x2, [x19, #RANDOMSEQUENCE.final_steps]
    ldr     x3, [x19, #RANDOMSEQUENCE.final_step_count]
    LDX     x4, rs_final_spectrum
    bl      gradient_new
    LDX     x0, rs_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, rs_final_map_width
    ldr     x6, [x19, #RANDOMSEQUENCE.final_direction]
    bl      gradient_map
    STX     x9, rs_final_map
    POP2    x19, x30
    ret

// randomsequence_next_frame -> w9 = 1 for a frame, 0 when done: reveal
// characters_per_tick characters from the end of the shuffled list.
randomsequence_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x9, rs_pending_count
    cbnz    x9, randomsequence_next_frame.tick
    bl      active_empty
    cbnz    w9, randomsequence_next_frame.finished
randomsequence_next_frame.tick:
    LDX     x21, rs_per_tick
    LDX     x22, rs_pending
randomsequence_next_frame.reveal:
    cbz     x21, randomsequence_next_frame.update
    LDX     x19, rs_pending_count
    cbz     x19, randomsequence_next_frame.update   // the remaining pops are no-ops
    sub     x19, x19, #1
    STX     x19, rs_pending_count
    ldr     w0, [x22, x19, lsl #2]
    PUSH1   x0
    bl      set_visible
    POP1    x0
    bl      active_insert
    sub     x21, x21, #1
    b       randomsequence_next_frame.reveal
randomsequence_next_frame.update:
    bl      update
    mov     w9, #1
    POP2    x22, x30
    POP2    x19, x21
    ret
randomsequence_next_frame.finished:
    mov     w9, #0
    POP2    x22, x30
    POP2    x19, x21
    ret

    .section .rodata
    .balign 8
rs_seven:           .quad 7

    TSTATE
    .balign 8
rs_per_tick:        .skip 8
rs_pending:         .skip 8         // u32 slots, popped from the end
rs_pending_count:   .skip 8
rs_final_spectrum:  .skip 8
rs_final_map:       .skip 8
rs_final_map_width: .skip 8
rs_pair:            .skip 8 * 2
rs_fade_spectrum:   .skip 8 * 16
rs_bg_fade:         .skip 8 * RS_FADE_LEN

    .text
