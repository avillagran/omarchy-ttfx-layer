// effects/wipe.s - "Performs a wipe across the terminal to reveal
// characters" (src/effects/wipe.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Wipe).
//
// The groups from get_characters_grouped are 16-byte records, but the
// SequenceEaser works over u64 elements. It is handed the group array with
// the group count as its length: only its pointer arithmetic is used, so an
// element offset e maps to the group record at groups + 2 * e.
//
// ch_user0 holds each character's "wipe" scene index.

.equ HAVE_wipe, 1

.equ WIPE.direction,        0           // CharacterGroup (GROUP_*)
.equ WIPE.delay,            8
.equ WIPE.ease,             16          // easing id
.equ WIPE.final_stops,      24          // *const u64
.equ WIPE.final_stop_count, 32
.equ WIPE.final_steps,      40          // *const i64
.equ WIPE.final_step_count, 48
.equ WIPE.final_frames,     56          // fits i32 (marshal checks)
.equ WIPE.final_direction,  64
.equ WIPE_size,             72

.equ WIPE_SCENE,            NAME_LITERAL + 0

    .text

// wipe_build: Wipe::build.
wipe_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    mov     w0, #FILTER_INPUT
    ldr     w1, [x19, #WIPE.direction]
    bl      get_characters_grouped
    STX     x9, wipe_groups
    ADRG    x0, wipe_easer
    mov     x1, x9
    ldr     w3, [x19, #WIPE.ease]
    mov     w4, #100
    bl      sequence_easer_new
    bl      wipe_final_map
    // the per-character wipe gradient: spectrum[0] -> the final color
    ldr     x0, [x19, #WIPE.final_steps]
    ldr     x3, [x19, #WIPE.final_step_count]
    mov     w1, #2
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, wipe_pair_spectrum
    LDX     x9, wipe_spectrum
    ldr     x9, [x9]
    STX     x9, wipe_pair_stops
    mov     x9, #NONE
    STX     x9, wipe_last_fg
    // dynamic handling: sum(final steps) + 1 frames of the input colors
    ldr     x3, [x19, #WIPE.final_steps]
    ldr     x2, [x19, #WIPE.final_step_count]
    mov     w9, #1
wipe_build.sum:
    cbz     x2, wipe_build.summed
    sub     x2, x2, #1
    ldr     x10, [x3, x2, lsl #3]
    add     x9, x9, x10
    b       wipe_build.sum
wipe_build.summed:
    STX     x9, wipe_dynamic_frames
    ldr     x9, [x19, #WIPE.delay]
    STX     x9, wipe_delay_left
    // one "wipe" scene per input character (no RNG, so order is free)
    LDX     x21, input_chars
    LDX     x22, input_count
    mov     w20, #0
wipe_build.char:
    cmp     x20, x22
    b.hs    wipe_build.built
    ldr     w24, [x21, x20, lsl #2]     // slot
    mov     w0, w24
    mov     w1, #WIPE_SCENE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w23, w9
    LDX     x3, ch_user0
    str     x9, [x3, x24, lsl #3]
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    wipe_build.dynamic
    // final fg from the coordinate map
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x24, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, wipe_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x24, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, wipe_map
    ldr     x9, [x3, x9, lsl #3]
    LDX     x10, wipe_last_fg
    cmp     x9, x10
    b.eq    wipe_build.gradient
    STX     x9, wipe_last_fg
    ADRG    x0, wipe_pair_stops
    str     x9, [x0, #8]
    mov     w1, #2
    ldr     x2, [x19, #WIPE.final_steps]
    ldr     x3, [x19, #WIPE.final_step_count]
    LDX     x4, wipe_pair_spectrum
    bl      gradient_new
    STW     w9, wipe_pair_len
wipe_build.gradient:
    // apply_gradient_to_symbols with one symbol and an fg gradient: one
    // frame per spectrum color (the visuals shared by symbol and final color)
    LDX     x0, ch_sym
    ldr     x0, [x0, x24, lsl #3]
    LDX     x1, wipe_pair_spectrum
    LDW     w2, wipe_pair_len
    mov     x3, #NONE
    LDX     x4, wipe_last_fg
    bl      visual_run
    mov     w0, w23
    mov     x1, x9
    ldr     w3, [x19, #WIPE.final_frames]
    bl      visual_frames
    add     x20, x20, #1
    b       wipe_build.char
wipe_build.dynamic:
    PUSH2   x20, x22
    LDX     x22, wipe_dynamic_frames
wipe_build.frame:
    cmp     x22, #0
    b.le    wipe_build.framed
    LDX     x3, ch_fg
    ldr     x3, [x3, x24, lsl #3]
    LDX     x4, ch_bg
    ldr     x4, [x4, x24, lsl #3]
    LDX     x1, ch_sym
    ldr     x1, [x1, x24, lsl #3]
    mov     w0, w23
    ldr     w2, [x19, #WIPE.final_frames]
    mov     w5, #0
    bl      scene_add_frame
    sub     x22, x22, #1
    b       wipe_build.frame
wipe_build.framed:
    POP2    x20, x22
    add     x20, x20, #1
    b       wipe_build.char
wipe_build.built:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// wipe_final_map (x19 = config): Gradient::new(final stops, final steps) and
// its coordinate mapping over the text rectangle.
wipe_final_map:
    PUSH1   x30
    ldr     x0, [x19, #WIPE.final_steps]
    ldr     x3, [x19, #WIPE.final_step_count]
    ldr     x1, [x19, #WIPE.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, wipe_spectrum
    ldr     x0, [x19, #WIPE.final_stops]
    ldr     x1, [x19, #WIPE.final_stop_count]
    ldr     x2, [x19, #WIPE.final_steps]
    ldr     x3, [x19, #WIPE.final_step_count]
    LDX     x4, wipe_spectrum
    bl      gradient_new
    LDX     x0, wipe_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, wipe_map_width
    ldr     x6, [x19, #WIPE.final_direction]
    bl      gradient_map
    STX     x9, wipe_map
    POP1    x30
    ret

// wipe_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
wipe_next_frame:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    bl      active_empty
    cbz     w9, wipe_next_frame.run
    ADRG    x0, wipe_easer
    bl      sequence_easer_is_complete
    cbnz    w9, wipe_next_frame.finished
wipe_next_frame.run:
    LDX     x9, wipe_delay_left
    cbz     x9, wipe_next_frame.step
    sub     x9, x9, #1
    STX     x9, wipe_delay_left
    b       wipe_next_frame.update
wipe_next_frame.step:
    ADRG    x0, wipe_easer
    bl      sequence_easer_step
    // added groups: activate, reveal, track
    ldr     x19, [x9, #SequenceStep.added]
    ldr     x21, [x9, #SequenceStep.added_count]
    bl      wipe_group_record
wipe_next_frame.add_group:
    cbz     x21, wipe_next_frame.added
    ldr     x22, [x19]                  // slots
    ldr     x23, [x19, #8]              // count
wipe_next_frame.add_char:
    cbz     x23, wipe_next_frame.add_next
    ldr     w24, [x22]
    mov     w0, w24
    LDX     x9, ch_user0
    add     x9, x9, x24, lsl #3
    ldr     w1, [x9]
    bl      scene_activate
    mov     w0, w24
    bl      set_visible
    mov     w0, w24
    bl      active_insert
    add     x22, x22, #4
    sub     x23, x23, #1
    b       wipe_next_frame.add_char
wipe_next_frame.add_next:
    add     x19, x19, #16
    sub     x21, x21, #1
    b       wipe_next_frame.add_group
wipe_next_frame.added:
    // removed groups: deactivate, rewind the scene, hide
    ADRG    x9, wipe_easer + SequenceEaser.result
    ldr     x19, [x9, #SequenceStep.removed]
    ldr     x21, [x9, #SequenceStep.removed_count]
    bl      wipe_group_record
wipe_next_frame.remove_group:
    cbz     x21, wipe_next_frame.removed
    ldr     x22, [x19]
    ldr     x23, [x19, #8]
wipe_next_frame.remove_char:
    cbz     x23, wipe_next_frame.remove_next
    ldr     w24, [x22]
    mov     w0, w24
    mov     w1, #NONE
    bl      scene_deactivate
    LDX     x9, ch_user0
    add     x9, x9, x24, lsl #3
    ldr     w0, [x9]
    bl      scene_reset
    mov     w0, w24
    mov     w1, #0
    bl      set_visibility
    add     x22, x22, #4
    sub     x23, x23, #1
    b       wipe_next_frame.remove_char
wipe_next_frame.remove_next:
    add     x19, x19, #16
    sub     x21, x21, #1
    b       wipe_next_frame.remove_group
wipe_next_frame.removed:
    LDX     x9, effect_config
    ldr     x9, [x9, #WIPE.delay]
    STX     x9, wipe_delay_left
wipe_next_frame.update:
    bl      update
    mov     w9, #1
    b       wipe_next_frame.out
wipe_next_frame.finished:
    mov     w9, #0
wipe_next_frame.out:
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// wipe_group_record: x19 = an easer element pointer -> the group record it
// stands for (groups + 2 * (element - groups)). Clobbers x9.
wipe_group_record:
    LDX     x9, wipe_groups
    sub     x19, x19, x9
    add     x19, x9, x19, lsl #1
    ret

    TSTATE
    .balign 8
wipe_easer:             .skip SequenceEaser_size
    .balign 8
wipe_groups:            .skip 8
wipe_spectrum:          .skip 8
wipe_map:               .skip 8
wipe_map_width:         .skip 8
wipe_pair_stops:        .skip 16
wipe_pair_spectrum:     .skip 8
wipe_pair_len:          .skip 8
wipe_last_fg:           .skip 8
wipe_dynamic_frames:    .skip 8
wipe_delay_left:        .skip 8
