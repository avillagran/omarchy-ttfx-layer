// effects/decrypt.s - "Display a movie style decryption effect"
// (src/effects/decrypt.rs).
//
// Config (src/asm/effects.rs DecryptAsm):
//
// Scenes are created in exactly the order Rust creates them, so every RNG
// draw lines up.

.equ HAVE_decrypt, 1

.equ DECRYPT.typing_speed,      0
.equ DECRYPT.cipher_colors,     8       // *const u64
.equ DECRYPT.cipher_count,      16
.equ DECRYPT.final_stops,       24      // *const u64
.equ DECRYPT.final_stop_count,  32
.equ DECRYPT.final_steps,       40      // *const i64
.equ DECRYPT.final_step_count,  48
.equ DECRYPT.final_direction,   56
.equ DECRYPT_size,              64

.equ DECRYPT_ENCRYPTED_COUNT,   523     // 94 + 24 + 127 + 278 symbols

// scene names
.equ DECRYPT_TYPING,            NAME_LITERAL + 0
.equ DECRYPT_FAST_DECRYPT,      NAME_LITERAL + 1
.equ DECRYPT_SLOW_DECRYPT,      NAME_LITERAL + 2
.equ DECRYPT_DISCOVERED,        NAME_LITERAL + 3

// ch_user0 holds the typing scene (low half) and fast_decrypt (high half)

    .text

// decrypt_build: DecryptIterator.__init__ + build().
decrypt_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    bl      make_encrypted_symbols
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    cset    w9, eq
    STB     w9, decrypt_dynamic
    // memo of (cipher color, symbol) -> handle: colors x (523 + 4 blocks)
    LDX     x9, effect_config
    ldr     x0, [x9, #DECRYPT.cipher_count]
    mov     x8, #((DECRYPT_ENCRYPTED_COUNT + 4) * 4)
    mul     x0, x0, x8
    bl      alloc
    STX     x9, decrypt_memo
    // the final gradient mapped over the text rectangle
    bl      final_color_map
    LDX     x21, input_chars
    LDX     x22, input_count
    // --- prepare_data_for_type_effect: one "typing" scene per character
    mov     x19, #0
decrypt_build.typing:
    cmp     x19, x22
    b.hs    decrypt_build.decrypting
    ldr     w0, [x21, x19, lsl #2]
    mov     w1, #DECRYPT_TYPING
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w23, w9
    ldr     w0, [x21, x19, lsl #2]
    LDX     x3, ch_user0
    add     x3, x3, x0, lsl #3
    str     w9, [x3]
    mov     w20, #0
decrypt_build.block:
    bl      choose_cipher               // w9 = color index
    add     w1, w20, #DECRYPT_ENCRYPTED_COUNT
    bl      memo_visual
    mov     w1, w9
    mov     w0, w23
    mov     w2, #2
    bl      scene_add_frame_visual
    add     w20, w20, #1
    cmp     w20, #4
    b.lo    decrypt_build.block
    mov     w0, #DECRYPT_ENCRYPTED_COUNT
    bl      rng_below                   // symbol first ...
    mov     w24, w9
    bl      choose_cipher               // ... then its color
    mov     w1, w24
    bl      memo_visual
    mov     w1, w9
    mov     w0, w23
    mov     w2, #1
    bl      scene_add_frame_visual
    add     x19, x19, #1
    b       decrypt_build.typing
decrypt_build.decrypting:
    // --- prepare_data_for_decrypt_effect
    mov     x19, #0
decrypt_build.decrypt_char:
    cmp     x19, x22
    b.hs    decrypt_build.built
    ldr     w0, [x21, x19, lsl #2]
    bl      make_decrypting_scenes
    add     x19, x19, #1
    b       decrypt_build.decrypt_char
decrypt_build.built:
    STX     xzr, typing_pos
    STB     wzr, decrypt_phase
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// make_decrypting_scenes(w0=slot) with x19 = the character's position k.
make_decrypting_scenes:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56]: a frame's duration
    mov     w21, w0                     // slot
    // fast_decrypt: one color, 80 random symbols of duration 2
    MOV64   w1, DECRYPT_FAST_DECRYPT
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w22, w9                     // fast scene
    LDX     x3, ch_user0
    add     x3, x3, x21, lsl #3
    str     w9, [x3, #4]
    bl      choose_cipher
    mov     w23, w9                     // color index for the whole character
    mov     w20, #0
make_decrypting_scenes.fast:
    mov     w0, #DECRYPT_ENCRYPTED_COUNT
    bl      rng_below
    mov     w1, w9
    mov     w9, w23
    bl      memo_visual
    mov     w1, w9
    mov     w0, w22
    mov     w2, #2
    bl      scene_add_frame_visual
    add     w20, w20, #1
    cmp     w20, #80
    b.lo    make_decrypting_scenes.fast
    // slow_decrypt: 1-15 frames of long or flickering durations
    mov     w0, w21
    MOV64   w1, DECRYPT_SLOW_DECRYPT
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9                     // slow scene
    mov     x0, #1
    mov     x1, #15
    bl      rng_randint
    mov     w20, w9
make_decrypting_scenes.slow:
    cbz     w20, make_decrypting_scenes.discovered
    mov     w0, #DECRYPT_ENCRYPTED_COUNT
    bl      rng_below
    mov     w19, w9                     // symbol
    mov     x0, #0
    mov     x1, #100
    bl      rng_randint
    cmp     x9, #30
    b.gt    make_decrypting_scenes.short
    mov     x0, #35
    mov     x1, #60
    b       make_decrypting_scenes.duration
make_decrypting_scenes.short:
    mov     x0, #3
    mov     x1, #6
make_decrypting_scenes.duration:
    bl      rng_randrange
    str     x9, [sp, #56]
    mov     w1, w19
    mov     w9, w23
    bl      memo_visual
    mov     w1, w9
    mov     w0, w24
    ldr     x2, [sp, #56]
    bl      scene_add_frame_visual
    sub     w20, w20, #1
    b       make_decrypting_scenes.slow
make_decrypting_scenes.discovered:
    // discovered: white -> final color in 10 steps (11 colors), duration 5
    mov     w0, w21
    MOV64   w1, DECRYPT_DISCOVERED
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9                     // discovered scene
    LDB     w9, decrypt_dynamic
    cbnz    w9, make_decrypting_scenes.dynamic
    LDX     x9, ch_row
    ldrsw   x9, [x9, x21, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, final_map_width
    mul     x9, x9, x10
    LDX     x3, ch_col
    ldrsw   x3, [x3, x21, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, final_map
    ldr     x9, [x3, x9, lsl #3]
    ADRG    x0, pair_stops
    mov     x10, #0xffffff
    str     x10, [x0]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, ten_steps
    mov     w3, #1
    ADRG    x4, pair_spectrum
    bl      gradient_new
    mov     w19, w9                     // 11
    mov     w23, #0
make_decrypting_scenes.gradient:
    cmp     w23, w19
    b.hs    make_decrypting_scenes.events
    ADRG    x9, pair_spectrum
    ldr     x3, [x9, x23, lsl #3]
    mov     x4, #NONE
    LDX     x1, ch_sym
    ldr     x1, [x1, x21, lsl #3]
    mov     w0, w20
    mov     w2, #5
    mov     w5, #0
    bl      scene_add_frame
    add     w23, w23, #1
    b       make_decrypting_scenes.gradient
make_decrypting_scenes.dynamic:
    // dynamic: white -> the input fg and/or bg in 10 steps, or one plain frame
    // when the character has no input colors
    mov     w19, #0                     // fg spectrum length (0 = none)
    mov     w23, #0                     // bg spectrum length
    LDX     x9, ch_fg
    ldr     x9, [x9, x21, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    make_decrypting_scenes.dynamic_bg
    ADRG    x0, pair_stops
    mov     x10, #0xffffff
    str     x10, [x0]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, ten_steps
    mov     w3, #1
    ADRG    x4, pair_spectrum
    bl      gradient_new
    mov     w19, w9
make_decrypting_scenes.dynamic_bg:
    LDX     x9, ch_bg
    ldr     x9, [x9, x21, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    make_decrypting_scenes.dynamic_frames
    ADRG    x0, pair_stops
    mov     x10, #0xffffff
    str     x10, [x0]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, ten_steps
    mov     w3, #1
    ADRG    x4, bg_spectrum
    bl      gradient_new
    mov     w23, w9
make_decrypting_scenes.dynamic_frames:
    orr     w9, w19, w23
    cbz     w9, make_decrypting_scenes.dynamic_plain
    mov     x4, #0
    cbz     w19, make_decrypting_scenes.dynamic_no_fg
    ADRG    x4, pair_spectrum
make_decrypting_scenes.dynamic_no_fg:
    mov     x6, #0
    cbz     w23, make_decrypting_scenes.dynamic_no_bg
    ADRG    x6, bg_spectrum
make_decrypting_scenes.dynamic_no_bg:
    mov     x7, x23
    mov     w0, w20
    LDX     x1, ch_sym
    add     x1, x1, x21, lsl #3
    mov     w2, #1
    mov     w3, #5
    mov     w5, w19
    bl      scene_apply_gradient
    b       make_decrypting_scenes.events
make_decrypting_scenes.dynamic_plain:
    LDX     x1, ch_sym
    ldr     x1, [x1, x21, lsl #3]
    mov     w0, w20
    mov     w2, #5
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
make_decrypting_scenes.events:
    // fast complete -> slow; slow complete -> discovered; start on fast
    mov     w0, w21
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, DECRYPT_FAST_DECRYPT
    mov     w4, #ACT_ACTIVATE_SCENE
    MOV64   w5, DECRYPT_SLOW_DECRYPT
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, DECRYPT_SLOW_DECRYPT
    mov     w4, #ACT_ACTIVATE_SCENE
    MOV64   w5, DECRYPT_DISCOVERED
    mov     x6, #0
    bl      event_register
    mov     w0, w21
    mov     w1, w22
    bl      scene_activate
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// choose_cipher -> w9 = rng.choice over the ciphertext colors (an index).
choose_cipher:
    LDX     x9, effect_config
    ldr     x0, [x9, #DECRYPT.cipher_count]
    b       rng_below

// memo_visual(w9=color index, w1=symbol index) -> w9 = handle. Symbol
// indices past DECRYPT_ENCRYPTED_COUNT are the four typing blocks.
memo_visual:
    PUSH2   x19, x30
    mov     w8, #(DECRYPT_ENCRYPTED_COUNT + 4)
    mul     w19, w9, w8
    add     w19, w19, w1
    LDX     x3, decrypt_memo
    ldr     w2, [x3, x19, lsl #2]
    cbnz    w2, memo_visual.hit
    LDX     x3, effect_config
    ldr     x3, [x3, #DECRYPT.cipher_colors]
    ldr     x0, [x3, w9, uxtw #3]
    ADRG    x3, encrypted_symbols
    ldr     x2, [x3, w1, uxtw #3]
    mov     x1, #NONE
    mov     w3, #0
    bl      visual_make
    LDX     x3, decrypt_memo
    str     w9, [x3, x19, lsl #2]
    mov     w2, w9
memo_visual.hit:
    mov     w9, w2
    POP2    x19, x30
    ret

// make_encrypted_symbols: the _DecryptChars ranges, then the typing blocks.
make_encrypted_symbols:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w21, #0
    ADRG    x19, symbol_ranges
make_encrypted_symbols.range:
    ldr     w22, [x19]
    cbz     w22, make_encrypted_symbols.blocks
make_encrypted_symbols.code:
    ldr     w9, [x19, #4]
    cmp     w22, w9
    b.hs    make_encrypted_symbols.next_range
    mov     w0, w22
    bl      utf8_pack
    ADRG    x3, encrypted_symbols
    str     x9, [x3, x21, lsl #3]
    add     w21, w21, #1
    add     w22, w22, #1
    b       make_encrypted_symbols.code
make_encrypted_symbols.next_range:
    add     x19, x19, #8
    b       make_encrypted_symbols.range
make_encrypted_symbols.blocks:
    ADRG    x19, block_codes
make_encrypted_symbols.block:
    ldr     w0, [x19]
    cbz     w0, make_encrypted_symbols.done
    bl      utf8_pack
    ADRG    x3, encrypted_symbols
    str     x9, [x3, x21, lsl #3]
    add     w21, w21, #1
    add     x19, x19, #4
    b       make_encrypted_symbols.block
make_encrypted_symbols.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    // spectrum capacity: sum over pairs of the step counts, plus one per pair
    ldr     x0, [x19, #DECRYPT.final_steps]
    ldr     x3, [x19, #DECRYPT.final_step_count]
    ldr     x1, [x19, #DECRYPT.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, final_spectrum
    ldr     x0, [x19, #DECRYPT.final_stops]
    ldr     x1, [x19, #DECRYPT.final_stop_count]
    ldr     x2, [x19, #DECRYPT.final_steps]
    ldr     x3, [x19, #DECRYPT.final_step_count]
    LDX     x4, final_spectrum
    bl      gradient_new
    LDX     x0, final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, final_map_width
    ldr     x6, [x19, #DECRYPT.final_direction]
    bl      gradient_map
    STX     x9, final_map
    POP2    x19, x30
    ret

// decrypt_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
decrypt_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    // The analyzer holds a beat across several audio chunks. Reward its
    // rising edge once, rather than one typing bonus for every video frame.
    LDX     x9, audio_word
    ubfx    w9, w9, #16, #8             // beat byte
    cmp     w9, #0
    cset    w9, ne
    LDB     w10, decrypt_beat_previous
    STB     w9, decrypt_beat_previous
    eor     w10, w10, #1
    and     w9, w9, w10
    STB     w9, decrypt_beat_edge
    LDB     w9, decrypt_phase
    cbnz    w9, decrypt_next_frame.decrypting
    LDX     x9, typing_pos
    LDX     x10, input_count
    cmp     x9, x10
    b.lo    decrypt_next_frame.typing
    bl      active_empty
    cbz     w9, decrypt_next_frame.typing
    // every character typed and settled: start decrypting all of them
    mov     x19, #0
decrypt_next_frame.start:
    LDX     x10, input_count
    cmp     x19, x10
    b.hs    decrypt_next_frame.started
    LDX     x9, input_chars
    ldr     w22, [x9, x19, lsl #2]
    mov     w0, w22
    bl      active_insert
    LDX     x9, ch_user0
    add     x9, x9, x22, lsl #3
    ldr     w1, [x9, #4]                // fast_decrypt
    mov     w0, w22
    bl      scene_activate
    add     x19, x19, #1
    b       decrypt_next_frame.start
decrypt_next_frame.started:
    mov     w9, #1
    STB     w9, decrypt_phase
decrypt_next_frame.decrypting:
    bl      active_empty
    cbnz    w9, decrypt_next_frame.finished
    bl      update
    mov     w9, #1
    b       decrypt_next_frame.out
decrypt_next_frame.typing:
    LDX     x9, typing_pos
    LDX     x10, input_count
    cmp     x9, x10
    b.hs    decrypt_next_frame.tick
    mov     x0, #0
    mov     x1, #100
    bl      rng_randint
    cmp     x9, #75
    b.le    decrypt_next_frame.typing_ready
    LDB     w9, decrypt_beat_edge
    cbz     w9, decrypt_next_frame.tick
decrypt_next_frame.typing_ready:
    LDX     x21, effect_config
    ldr     x21, [x21, #DECRYPT.typing_speed]
    // Treble adds 0..3 characters to this burst, a new beat adds one.
    // At most four additional characters: no new scenes, RNG calls, or
    // global animation ticks. In silence the reference burst is unchanged.
    LDX     x9, audio_word
    ubfx    w9, w9, #24, #8             // treble
    lsr     w9, w9, #6
    LDB     w10, decrypt_beat_edge
    add     w9, w9, w10
    add     x21, x21, x9
decrypt_next_frame.type:
    cbz     x21, decrypt_next_frame.tick
    sub     x21, x21, #1
    LDX     x19, typing_pos
    LDX     x10, input_count
    cmp     x19, x10
    b.hs    decrypt_next_frame.tick     // typed out: the rest draw nothing
    add     x9, x19, #1
    STX     x9, typing_pos
    LDX     x9, input_chars
    ldr     w22, [x9, x19, lsl #2]
    mov     w0, w22
    bl      set_visible
    LDX     x9, ch_user0
    add     x9, x9, x22, lsl #3
    ldr     w1, [x9]                    // typing
    mov     w0, w22
    bl      scene_activate
    mov     w0, w22
    bl      active_insert
    b       decrypt_next_frame.type
decrypt_next_frame.tick:
    bl      update
    mov     w9, #1
    b       decrypt_next_frame.out
decrypt_next_frame.finished:
    mov     w9, #0
decrypt_next_frame.out:
    POP2    x22, x30
    POP2    x19, x21
    ret

    .section .rodata
    .balign 8
ten_steps:      .quad 10
// [start, end) codepoint ranges of the encrypted symbol alphabet
symbol_ranges:  .4byte 33, 127, 9608, 9632, 9472, 9599, 174, 452, 0, 0
// typing blocks: ▉ ▓ ▒ ░
block_codes:    .4byte 0x2589, 0x2593, 0x2592, 0x2591, 0

    TSTATE
    .balign 8
encrypted_symbols:  .skip 8 * (DECRYPT_ENCRYPTED_COUNT + 4)
decrypt_memo:       .skip 8
final_spectrum:     .skip 8
final_map:          .skip 8
final_map_width:    .skip 8
typing_pos:         .skip 8
pair_stops:         .skip 8 * 2
pair_spectrum:      .skip 8 * 16
bg_spectrum:        .skip 8 * 16
decrypt_phase:      .skip 1
decrypt_dynamic:    .skip 1
decrypt_beat_edge:  .skip 1         // rising edge of this frame's beat byte
decrypt_beat_previous: .skip 1

    .text
