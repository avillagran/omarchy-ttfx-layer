// effects/burn.s - "Burns vertically in the canvas" (src/effects/burn.rs).
//
// RNG order is BurnIterator.__init__'s: PrimsSimple's starting coord, the
// smoke pool's 2000 symbol draws, then build() runs PrimsSimple to
// completion. Each frame draws randint(2, 4) and every finished burn may
// draw random() and emit a smoke particle (one randint for its target).
//
// Each emission registers a fresh reclaim callback on the particle (its
// payload is the emission count), exactly like Rust's per-emission closure,
// so the particle's action list grows the same way.

.equ HAVE_burn, 1

.equ BURN.starting_color,       0
.equ BURN.burn_colors,          8       // *const u64
.equ BURN.burn_color_count,     16
.equ BURN.smoke_chance,         24      // f64
.equ BURN.final_stops,          32      // *const u64
.equ BURN.final_stop_count,     40
.equ BURN.final_steps,          48      // *const i64
.equ BURN.final_step_count,     56
.equ BURN.final_direction,      64
.equ BURN_size,                 72

// scene names
.equ BRN_BURN,                  NAME_LITERAL + 0
.equ BRN_SMOKE,                 NAME_LITERAL + 1

.equ BRN_CHAR_ORDER,            9
.equ BRN_SMOKE_SYMBOLS,         6
.equ BRN_SMOKE_LEN,             10      // 504F4F -> C7C7C7 in 9 steps
.equ BRN_CHAR_LEN,              9       // fire end -> final color in 8 steps

    .text

// burn_build: BurnIterator.__init__ + build().
burn_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    bl      brn_symbols
    mov     w0, #1
    bl      ps_new
    // the smoke pool: 2000 particles at (0, 0), at most 2000
    ADRG    x0, brn_smoke_stops
    mov     w1, #2
    ADRG    x2, brn_nine
    mov     w3, #1
    ADRG    x4, brn_smoke_spectrum
    bl      gradient_new
    ADRG    x0, brn_pool
    ADRG    x1, brn_smoke_symbols
    mov     w2, #BRN_SMOKE_SYMBOLS
    mov     w3, #2000
    mov     x4, #0
    bl      pool_init
    ADRG    x0, brn_pool
    ADRG    x9, brn_init_smoke
    str     x9, [x0, #POOL.initializer]
    mov     w1, #2000
    bl      pool_preallocate
    // build(): the final gradient mapping and the fire gradient
    bl      brn_final_map
    ADRG    x0, brn_ten
    mov     w3, #1
    ldr     x1, [x19, #BURN.burn_color_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, brn_fire
    ldr     x0, [x19, #BURN.burn_colors]
    ldr     x1, [x19, #BURN.burn_color_count]
    ADRG    x2, brn_ten
    mov     w3, #1
    LDX     x4, brn_fire
    bl      gradient_new
    mov     w9, w9
    STX     x9, brn_fire_len
    LDX     x3, brn_fire
    add     x3, x3, x9, lsl #3
    ldur    x9, [x3, #-8]
    STX     x9, brn_pair_stops          // the fire gradient's last color
    bl      ps_run
    STX     x9, brn_order
    STX     x2, brn_order_count
    STX     xzr, brn_order_head
    // every input character, top to bottom, left to right
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x23, x9
    mov     x24, x2
    mov     x22, #0
burn_build.character:
    cmp     x22, x24
    b.hs    burn_build.built
    ldr     w0, [x23, x22, lsl #2]
    bl      brn_character
    add     x22, x22, #1
    b       burn_build.character
burn_build.built:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// brn_symbols: pack the burn and smoke symbol tables.
brn_symbols:
    PUSH2   x19, x30
    mov     w19, #0
brn_symbols.next:
    cmp     w19, #(BRN_CHAR_ORDER + BRN_SMOKE_SYMBOLS)
    b.hs    brn_symbols.done
    ADRG    x9, brn_codepoints
    ldr     w0, [x9, x19, lsl #2]
    bl      utf8_pack
    ADRG    x3, brn_char_order
    str     x9, [x3, x19, lsl #3]
    add     w19, w19, #1
    b       brn_symbols.next
brn_symbols.done:
    POP2    x19, x30
    ret

// brn_character(w0=slot): the starting appearance, the burn scene, the
// final color scene and the two burn-complete events.
brn_character:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w19, w0
    bl      set_visible
    mov     w0, w19
    mov     x1, #0
    LDX     x9, effect_config
    ldr     x2, [x9, #BURN.starting_color]
    mov     x3, #NONE
    bl      set_appearance
    // the burn scene is the same for every character whose input colors
    // don't enter it: later ones clone the first one's
    LDX     x9, brn_template
    cbz     x9, brn_character.burn_fresh
    LDX     x10, cfg_existing_colors
    cbnz    x10, brn_character.burn_clone
    LDX     x3, ch_flags
    ldrh    w3, [x3, x19, lsl #1]
    tst     w3, #CF_PREEXISTING
    b.ne    brn_character.burn_fresh
brn_character.burn_clone:
    mov     w0, w19
    sub     w1, w9, #1
    MOV64   w2, BRN_BURN
    bl      scene_copy
    b       brn_character.final_scene
brn_character.burn_fresh:
    mov     w0, w19
    MOV64   w1, BRN_BURN
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    mov     x6, #0
    mov     x7, #0
    mov     w0, w9
    ADRG    x1, brn_char_order
    mov     w2, #BRN_CHAR_ORDER
    mov     w3, #4
    LDX     x4, brn_fire
    LDX     x5, brn_fire_len
    bl      scene_apply_gradient
    LDX     x9, brn_template
    cbnz    x9, brn_character.final_scene
    SCENE_PTR x9, x20
    ldr     w9, [x9, #SC_FLAGS]
    tst     w9, #(SCF_PREEXISTING | SCF_PRE_BOLD)
    b.ne    brn_character.final_scene
    add     w9, w20, #1
    STX     x9, brn_template
brn_character.final_scene:
    // the final color scene takes the next auto id
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    SCENE_PTR x9, x20
    MOV64   x10, SC_NAME
    ldr     w24, [x9, x10]
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    brn_character.dynamic
    // fire end -> the mapped final color, 8 steps, duration 4
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x19, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, brn_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x19, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, brn_map
    ldr     x9, [x3, x9, lsl #3]
    ADRG    x0, brn_pair_stops
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, brn_eight
    mov     w3, #1
    ADRG    x4, brn_pair_spectrum
    bl      gradient_new
    mov     w21, w9
    mov     w22, #0
brn_character.frame:
    cmp     w22, w21
    b.hs    brn_character.events
    mov     w0, w20
    LDX     x1, ch_sym
    ldr     x1, [x1, x19, lsl #3]
    mov     w2, #4
    ADRG    x3, brn_pair_spectrum
    ldr     x3, [x3, x22, lsl #3]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    add     w22, w22, #1
    b       brn_character.frame
brn_character.dynamic:
    // fire end -> the input fg / bg, when present
    mov     w21, #0                     // fg length
    mov     w22, #0                     // bg length
    LDX     x9, ch_fg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    brn_character.dynamic_bg
    ADRG    x0, brn_pair_stops
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, brn_eight
    mov     w3, #1
    ADRG    x4, brn_pair_spectrum
    bl      gradient_new
    mov     w21, w9
brn_character.dynamic_bg:
    LDX     x9, ch_bg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    brn_character.dynamic_apply
    ADRG    x0, brn_pair_stops
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, brn_eight
    mov     w3, #1
    ADRG    x4, brn_bg_spectrum
    bl      gradient_new
    mov     w22, w9
brn_character.dynamic_apply:
    orr     w9, w21, w22
    cbz     w9, brn_character.plain
    mov     x4, #0
    cbz     w21, brn_character.no_fg
    ADRG    x4, brn_pair_spectrum
brn_character.no_fg:
    mov     x6, #0
    cbz     w22, brn_character.no_bg
    ADRG    x6, brn_bg_spectrum
brn_character.no_bg:
    mov     x7, x22
    mov     w0, w20
    LDX     x1, ch_sym
    add     x1, x1, x19, lsl #3
    mov     w2, #1
    mov     w3, #4
    mov     w5, w21
    bl      scene_apply_gradient
    b       brn_character.events
brn_character.plain:
    mov     w0, w20
    LDX     x1, ch_sym
    ldr     x1, [x1, x19, lsl #3]
    mov     w2, #4
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
brn_character.events:
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, BRN_BURN
    mov     w4, #ACT_ACTIVATE_SCENE
    mov     w5, w24
    mov     x6, #0
    bl      event_register
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, BRN_BURN
    mov     w4, #ACT_CALLBACK
    ADRG    x5, brn_emit_smoke
    mov     x6, #0
    bl      event_register
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// brn_final_map: Gradient::new(final stops, final steps) mapped over the
// text rectangle.
brn_final_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #BURN.final_steps]
    ldr     x3, [x19, #BURN.final_step_count]
    ldr     x1, [x19, #BURN.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, brn_final_spectrum
    ldr     x0, [x19, #BURN.final_stops]
    ldr     x1, [x19, #BURN.final_stop_count]
    ldr     x2, [x19, #BURN.final_steps]
    ldr     x3, [x19, #BURN.final_step_count]
    LDX     x4, brn_final_spectrum
    bl      gradient_new
    LDX     x0, brn_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, brn_map_width
    ldr     x6, [x19, #BURN.final_direction]
    bl      gradient_map
    STX     x9, brn_map
    POP2    x19, x30
    ret

// ------------------------------------------------------------ smoke

// brn_init_smoke(w0=slot): initialize_smoke - a "smoke" scene fading
// 504F4F -> C7C7C7 over the particle's symbol, layer 2.
brn_init_smoke:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w19, w0
    MOV64   w1, BRN_SMOKE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w21, w9
    // the smoke gradient over the symbol: six symbols, one spectrum
    LDX     x0, ch_sym
    ldr     x0, [x0, x19, lsl #3]
    ADRG    x1, brn_smoke_spectrum
    mov     w2, #BRN_SMOKE_LEN
    mov     x3, #NONE
    mov     x4, #0
    bl      visual_run
    mov     w0, w21
    mov     x1, x9
    mov     w3, #10
    bl      visual_frames
brn_init_smoke.layer:
    mov     w0, w19
    mov     x1, #2
    bl      set_layer
    POP2    x22, x30
    POP2    x19, x21
    ret

// brn_emit_smoke(w0=slot): the burn-complete callback, _emit_smoke at the
// character's input coordinate.
brn_emit_smoke:
    PUSH2   x19, x30
    mov     w19, w0
    bl      rng_random
    LDX     x9, effect_config
    ldr     d1, [x9, #BURN.smoke_chance]
    fcmp    d0, d1
    b.gt    brn_emit_smoke.done
    LDX     x9, brn_emissions
    add     x9, x9, #1
    STX     x9, brn_emissions
    mov     w0, w19
    bl      char_input_coord
    STX     x9, brn_origin
    ADRG    x0, brn_pool
    mov     x1, x9
    mov     x2, #0
    mov     w3, #1
    ADRG    x4, brn_on_emit
    LDX     x5, brn_emissions
    bl      pool_emit
brn_emit_smoke.done:
    POP2    x19, x30
    ret

// brn_on_emit(w0=particle, x1=emission): on_emit_smoke - restart the
// smoke scene, rise to a random column near the origin above the canvas,
// and reclaim when the scene completes.
brn_on_emit:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w19, w0
    mov     x21, x1
    MOV64   w1, BRN_SMOKE
    bl      scene_find
    mov     w0, w9
    bl      scene_reset
    LDD     d0, brn_half
    mov     w0, w19
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w22, w9
    LDSW    x0, brn_origin
    add     x1, x0, #4
    sub     x0, x0, #4
    bl      rng_randint
    mov     w1, w9
    LDX     x9, canvas_top
    add     x9, x9, #1
    orr     x1, x1, x9, lsl #32
    mov     w0, w22
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w19
    mov     w1, w22
    bl      path_activate
    mov     w0, w19
    MOV64   w1, BRN_SMOKE
    bl      scene_activate_name
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   w3, BRN_SMOKE
    mov     w4, #ACT_CALLBACK
    ADRG    x5, brn_reclaim
    mov     x6, x21
    bl      event_register
    POP2    x22, x30
    POP2    x19, x21
    ret

// brn_reclaim(w0=particle): reclaim(hide=True, deactivate=True).
brn_reclaim:
    mov     w1, w0
    ADRG    x0, brn_pool
    mov     w2, #1
    mov     w3, #1
    b       pool_reclaim

// burn_next_frame -> w9 = 1 for a frame, 0 when done.
burn_next_frame:
    stp     x19, x21, [sp, #-32]!
    str     x30, [sp, #16]
    LDX     x9, brn_order_head
    LDX     x10, brn_order_count
    cmp     x9, x10
    b.lo    burn_next_frame.frame
    bl      active_empty
    cbnz    w9, burn_next_frame.finished
burn_next_frame.frame:
    mov     x0, #2
    mov     x1, #4
    bl      rng_randint
    mov     x21, x9
burn_next_frame.ignite:
    cbz     x21, burn_next_frame.tick
    sub     x21, x21, #1
    LDX     x9, brn_order_head
    LDX     x10, brn_order_count
    cmp     x9, x10
    b.hs    burn_next_frame.ignite
    add     x10, x9, #1
    STX     x10, brn_order_head
    LDX     x3, brn_order
    ldr     w19, [x3, x9, lsl #2]
    // _is_burnable: a visible symbol, or input colors unless ignored
    LDX     x9, ch_sym
    ldr     x9, [x9, x19, lsl #3]
    MOV64   x3, ((1 << 32) | 0x20)
    cmp     x9, x3
    b.ne    burn_next_frame.burn
    LDX     x9, cfg_existing_colors
    cmp     x9, #2
    b.eq    burn_next_frame.ignite
    LDX     x9, ch_fg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.ne    burn_next_frame.burn
    LDX     x9, ch_bg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1
    b.eq    burn_next_frame.ignite
burn_next_frame.burn:
    mov     w0, w19
    MOV64   w1, BRN_BURN
    bl      scene_activate_name
    mov     w0, w19
    bl      active_insert
    b       burn_next_frame.ignite
burn_next_frame.tick:
    bl      update
    mov     w9, #1
    b       burn_next_frame.out
burn_next_frame.finished:
    mov     w9, #0
burn_next_frame.out:
    ldr     x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

    .section .rodata
    .balign 8
brn_half:           .double 0.5
brn_nine:           .quad 9
brn_ten:            .quad 10
brn_eight:          .quad 8
brn_smoke_stops:    .quad 0x504F4F, 0xC7C7C7
// ' . ▖ ▙ █ ▜ ▀ ▝ .   then the smoke symbols . , ' ` # *
brn_codepoints:     .4byte 0x27, 0x2E, 0x2596, 0x2599, 0x2588, 0x259C, 0x2580, 0x259D, 0x2E
                    .4byte 0x2E, 0x2C, 0x27, 0x60, 0x23, 0x2A

    TSTATE
    .balign 8
brn_template:       .skip 8             // the burn scene to clone, + 1 (0 = none yet)
brn_pool:           .skip POOL_size
    .balign 8
brn_char_order:     .skip 8 * BRN_CHAR_ORDER
brn_smoke_symbols:  .skip 8 * BRN_SMOKE_SYMBOLS
brn_smoke_spectrum: .skip 8 * (BRN_SMOKE_LEN + 2)
brn_pair_stops:     .skip 8 * 2
brn_pair_spectrum:  .skip 8 * (BRN_CHAR_LEN + 2)
brn_bg_spectrum:    .skip 8 * (BRN_CHAR_LEN + 2)
brn_fire:           .skip 8
brn_fire_len:       .skip 8
brn_final_spectrum: .skip 8
brn_map:            .skip 8
brn_map_width:      .skip 8
brn_order:          .skip 8
brn_order_count:    .skip 8
brn_order_head:     .skip 8
brn_origin:         .skip 8
brn_emissions:      .skip 8

    .text
