// effects/sweep.s - "Sweep across the canvas to reveal uncolored text,
// reverse sweep to color the text" (src/effects/sweep.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Sweep).
//
// Every character (input and fill) gets an initial_sweep and a second_sweep
// scene, created in top-to-bottom, left-to-right order so the RNG draws line
// up. A SequenceEaser (in_out_circ, 100 steps) walks the first sweep's groups,
// then the second's.

.equ HAVE_sweep, 1

.equ SWEEP.symbols,             0       // *const u64 packed sweep symbols
.equ SWEEP.symbol_count,        8
.equ SWEEP.first_direction,     16      // GROUP_*
.equ SWEEP.second_direction,    24      // GROUP_*
.equ SWEEP.final_stops,         32      // *const u64
.equ SWEEP.final_stop_count,    40
.equ SWEEP.final_steps,         48      // *const i64
.equ SWEEP.final_step_count,    56
.equ SWEEP.final_direction,     64
.equ SWEEP_size,                72

.equ SW_INITIAL,        NAME_LITERAL + 0
.equ SW_SECOND,         NAME_LITERAL + 1
.equ SW_GRAY_COUNT,     5
.equ SW_IN_OUT_CIRC,    21

// ch_user0: the initial_sweep scene (low half) and second_sweep (high half)

    .text

// sweep_build: Sweep::build.
// Locals: [sp] final fg, [sp, #8] final bg, [sp, #16] character count.
sweep_build:
    sub     sp, sp, #96
    stp     x19, x20, [sp, #32]
    stp     x21, x22, [sp, #48]
    stp     x23, x24, [sp, #64]
    str     x30, [sp, #80]
    LDX     x19, effect_config
    bl      sw_final_color_map
    // the second sweep's palette: the spectrum, or the input colors (dynamic)
    LDX     x9, sw_spectrum
    STX     x9, sw_palette
    LDX     x9, sw_spectrum_len
    STX     x9, sw_palette_len
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    sweep_build.memos
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    lsl     x0, x2, #3
    add     x0, x0, #8
    lsl     x0, x0, #1
    bl      alloc
    mov     x23, x9
    mov     x24, #0                     // palette length
    mov     x20, #0
sweep_build.palette:
    cmp     x20, x22
    b.hs    sweep_build.palette_done
    ldr     w0, [x21, x20, lsl #2]
    LDX     x9, ch_fg
    ldr     x9, [x9, x0, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    sweep_build.palette_bg
    str     x9, [x23, x24, lsl #3]
    add     x24, x24, #1
sweep_build.palette_bg:
    LDX     x9, ch_bg
    ldr     x9, [x9, x0, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    sweep_build.palette_next
    str     x9, [x23, x24, lsl #3]
    add     x24, x24, #1
sweep_build.palette_next:
    add     x20, x20, #1
    b       sweep_build.palette
sweep_build.palette_done:
    cbz     x24, sweep_build.memos
    STX     x23, sw_palette
    STX     x24, sw_palette_len
sweep_build.memos:
    // memos of (symbol, color index) -> handle for both sweeps
    ldr     x0, [x19, #SWEEP.symbol_count]
    mov     x8, #(SW_GRAY_COUNT * 4)
    mul     x0, x0, x8
    bl      alloc
    STX     x9, sw_gray_memo
    ldr     x0, [x19, #SWEEP.symbol_count]
    LDX     x8, sw_palette_len
    mul     x0, x0, x8
    lsl     x0, x0, #2
    bl      alloc
    STX     x9, sw_color_memo
    // the scenes, per character in top-to-bottom, left-to-right order
    mov     w0, #(FILTER_INPUT | FILTER_INNER_FILL | FILTER_OUTER_FILL)
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    str     x2, [sp, #16]
    mov     x21, x9
    mov     x22, #0
sweep_build.char:
    ldr     x9, [sp, #16]
    cmp     x22, x9
    b.hs    sweep_build.groups
    ldr     w20, [x21, x22, lsl #2]     // slot
    // the final colors
    LDX     x9, ch_flags
    ldrh    w9, [x9, x20, lsl #1]
    tst     w9, #CF_FILL
    b.ne    sweep_build.fill
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    sweep_build.mapped
    LDX     x9, ch_fg
    ldr     x9, [x9, x20, lsl #3]
    LDX     x3, ch_bg
    ldr     x3, [x3, x20, lsl #3]
    b       sweep_build.colors
sweep_build.mapped:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x20, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, sw_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x20, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, sw_map
    ldr     x9, [x3, x9, lsl #3]
    mov     x3, #NONE
    b       sweep_build.colors
sweep_build.fill:
    mov     x9, #NONE
    mov     x3, x9
    LDX     x10, cfg_existing_colors
    cmp     x10, #1
    b.eq    sweep_build.colors
    mov     x9, #0                      // 000000
sweep_build.colors:
    str     x9, [sp]
    str     x3, [sp, #8]
    // initial_sweep: the symbols in random grays, then the symbol in 808080
    mov     w0, w20
    MOV64   w1, SW_INITIAL
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w23, w9
    mov     w0, w9
    LDX     x1, sw_gray_memo
    ADRG    x2, sw_grays
    mov     x3, #SW_GRAY_COUNT
    bl      sw_symbol_frames
    mov     w0, w23
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    mov     w2, #1
    MOV64   x3, 0x808080
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    // second_sweep: the symbols in random palette colors, then the final look
    mov     w0, w20
    MOV64   w1, SW_SECOND
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    LDX     x3, ch_user0
    add     x3, x3, x20, lsl #3
    stp     w23, w9, [x3]
    mov     w23, w9
    mov     w0, w9
    LDX     x1, sw_color_memo
    LDX     x2, sw_palette
    LDX     x3, sw_palette_len
    bl      sw_symbol_frames
    mov     w0, w23
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    mov     w2, #1
    ldr     x3, [sp]
    ldr     x4, [sp, #8]
    mov     w5, #0
    bl      scene_add_frame
    add     x22, x22, #1
    b       sweep_build.char
sweep_build.groups:
    ldr     x1, [x19, #SWEEP.first_direction]
    bl      sw_group_sequence
    STX     x9, sw_seq_first
    STX     x2, sw_seq_first_len
    ldr     x1, [x19, #SWEEP.second_direction]
    bl      sw_group_sequence
    STX     x9, sw_seq_second
    STX     x2, sw_seq_second_len
    ADRG    x0, sw_easer
    LDX     x1, sw_seq_first
    LDX     x2, sw_seq_first_len
    mov     w3, #SW_IN_OUT_CIRC
    mov     x4, #100
    bl      sequence_easer_new
    mov     w9, #1
    STB     w9, sw_first_phase
    ldr     x30, [sp, #80]
    ldp     x23, x24, [sp, #64]
    ldp     x21, x22, [sp, #48]
    ldp     x19, x20, [sp, #32]
    add     sp, sp, #96
    ret

// sw_symbol_frames(w0=scene, x1=memo, x2=colors, x3=color count): a
// 5-tick frame per sweep symbol, each in choice(colors) - the draws in
// symbol order, then the (symbol, color) visuals from the memo (made on
// first use; no bg, no attrs).
.equ SW_CHUNK, 64
sw_symbol_frames:
    stp     x19, x20, [sp, #-80]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    stp     x25, x26, [sp, #48]
    str     x30, [sp, #64]
    sub     sp, sp, #(SW_CHUNK * 4)
    mov     w20, w0
    mov     x21, x1
    mov     x22, x2
    mov     x23, x3
    mov     x24, #0                     // symbol index
sw_symbol_frames.chunk:
    LDX     x9, effect_config
    ldr     x19, [x9, #SWEEP.symbol_count]
    subs    x19, x19, x24
    b.ls    sw_symbol_frames.done
    mov     x9, #SW_CHUNK
    cmp     x19, x9
    csel    x19, x9, x19, hi            // this chunk's symbols
    mov     x0, x23
    mov     x1, sp
    mov     x2, x19
    bl      rng_below_fill
    // color index -> visual, in place
    mov     x25, #0
sw_symbol_frames.visual:
    ldr     w9, [sp, x25, lsl #2]       // color index
    add     x26, x24, x25
    mul     x26, x26, x23
    add     x26, x26, x9                // memo index
    ldr     w4, [x21, x26, lsl #2]
    cbnz    w4, sw_symbol_frames.known
    ldr     x0, [x22, x9, lsl #3]
    LDX     x9, effect_config
    ldr     x9, [x9, #SWEEP.symbols]
    add     x2, x24, x25
    ldr     x2, [x9, x2, lsl #3]
    mov     x1, #NONE
    mov     w3, #0
    bl      visual_make
    str     w9, [x21, x26, lsl #2]
    mov     w4, w9
sw_symbol_frames.known:
    str     w4, [sp, x25, lsl #2]
    add     x25, x25, #1
    cmp     x25, x19
    b.lo    sw_symbol_frames.visual
    mov     w0, w20
    mov     x1, sp
    mov     w2, w19
    mov     w3, #5
    bl      visual_frames
    add     x24, x24, x19
    b       sw_symbol_frames.chunk
sw_symbol_frames.done:
    add     sp, sp, #(SW_CHUNK * 4)
    ldr     x30, [sp, #64]
    ldp     x25, x26, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #80
    ret

// sw_group_sequence(x1=GROUP_*) -> x9 = u64 array of group record
// pointers, x2 = count: get_characters_grouped(fills filter, direction) in
// the form SequenceEaser takes.
sw_group_sequence:
    PUSH2   x21, x22
    PUSH2   x19, x30
    mov     w0, #(FILTER_INPUT | FILTER_INNER_FILL | FILTER_OUTER_FILL)
    bl      get_characters_grouped
    mov     x21, x9
    mov     x22, x2
    lsl     x0, x2, #3
    add     x0, x0, #8
    bl      alloc
    mov     x3, #0
sw_group_sequence.group:
    cmp     x3, x22
    b.hs    sw_group_sequence.done
    add     x2, x21, x3, lsl #4
    str     x2, [x9, x3, lsl #3]
    add     x3, x3, #1
    b       sw_group_sequence.group
sw_group_sequence.done:
    mov     x2, x22
    POP2    x19, x30
    POP2    x21, x22
    ret

// sw_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
sw_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #SWEEP.final_steps]
    ldr     x3, [x19, #SWEEP.final_step_count]
    ldr     x1, [x19, #SWEEP.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, sw_spectrum
    ldr     x0, [x19, #SWEEP.final_stops]
    ldr     x1, [x19, #SWEEP.final_stop_count]
    ldr     x2, [x19, #SWEEP.final_steps]
    ldr     x3, [x19, #SWEEP.final_step_count]
    LDX     x4, sw_spectrum
    bl      gradient_new
    mov     w9, w9
    STX     x9, sw_spectrum_len
    LDX     x0, sw_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, sw_map_width
    ldr     x6, [x19, #SWEEP.final_direction]
    bl      gradient_map
    STX     x9, sw_map
    POP2    x19, x30
    ret

// sweep_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
sweep_next_frame:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDB     w9, sw_complete
    cbz     w9, sweep_next_frame.step
    bl      active_empty
    cbnz    w9, sweep_next_frame.finished
sweep_next_frame.step:
    ADRG    x0, sw_easer
    bl      sequence_easer_step
    ldr     x21, [x9, #SequenceStep.added]
    ldr     x22, [x9, #SequenceStep.added_count]
    mov     x23, #0
sweep_next_frame.group:
    cmp     x23, x22
    b.hs    sweep_next_frame.stepped
    ldr     x9, [x21, x23, lsl #3]
    ldr     x19, [x9]                   // slots
    ldr     x24, [x9, #8]               // count
    mov     x20, #0
sweep_next_frame.activate:
    cmp     x20, x24
    b.hs    sweep_next_frame.insert
    ldr     w0, [x19, x20, lsl #2]
    LDB     w9, sw_first_phase
    cbz     w9, sweep_next_frame.second
    bl      set_visible
    ldr     w0, [x19, x20, lsl #2]
    LDX     x9, ch_user0
    add     x9, x9, x0, lsl #3
    ldr     w1, [x9]                    // initial_sweep
    b       sweep_next_frame.scene
sweep_next_frame.second:
    LDX     x9, ch_user0
    add     x9, x9, x0, lsl #3
    ldr     w1, [x9, #4]                // second_sweep
sweep_next_frame.scene:
    bl      scene_activate
    add     x20, x20, #1
    b       sweep_next_frame.activate
sweep_next_frame.insert:
    mov     x20, #0
sweep_next_frame.insert_next:
    cmp     x20, x24
    b.hs    sweep_next_frame.next_group
    ldr     w0, [x19, x20, lsl #2]
    bl      active_insert
    add     x20, x20, #1
    b       sweep_next_frame.insert_next
sweep_next_frame.next_group:
    add     x23, x23, #1
    b       sweep_next_frame.group
sweep_next_frame.stepped:
    ADRG    x0, sw_easer
    bl      sequence_easer_is_complete
    cbz     w9, sweep_next_frame.tick
    LDB     w9, sw_first_phase
    cbz     w9, sweep_next_frame.done_sweeping
    // the second sweep takes over the easer
    ADRG    x0, sw_easer
    LDX     x9, sw_seq_second
    str     x9, [x0, #SequenceEaser.sequence]
    LDX     x9, sw_seq_second_len
    str     x9, [x0, #SequenceEaser.length]
    bl      sequence_easer_reset
    STB     wzr, sw_first_phase
    b       sweep_next_frame.tick
sweep_next_frame.done_sweeping:
    mov     w9, #1
    STB     w9, sw_complete
sweep_next_frame.tick:
    bl      update
    mov     w9, #1
    b       sweep_next_frame.out
sweep_next_frame.finished:
    mov     w9, #0
sweep_next_frame.out:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

    .section .rodata
    .balign 8
// A0A0A0, 808080, 404040, 202020, 101010
sw_grays:       .quad 0xa0a0a0, 0x808080, 0x404040, 0x202020, 0x101010

    TSTATE
    .balign 8
sw_easer:           .skip SequenceEaser_size
sw_spectrum:        .skip 8
sw_spectrum_len:    .skip 8
sw_map:             .skip 8
sw_map_width:       .skip 8
sw_palette:         .skip 8
sw_palette_len:     .skip 8
sw_gray_memo:       .skip 8
sw_color_memo:      .skip 8
sw_seq_first:       .skip 8
sw_seq_first_len:   .skip 8
sw_seq_second:      .skip 8
sw_seq_second_len:  .skip 8
sw_first_phase:     .skip 1
sw_complete:        .skip 1

    .text
