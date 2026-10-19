// effects/smoke.s - "Smoke floods the canvas colorizing any text it
// crosses" (src/effects/smoke.rs).
//
// RNG order is SmokeIterator.__init__'s: PrimsWeighted (starting coord and
// one weight per character), the BreadthFirst starting coord, then build()
// runs PrimsWeighted to completion. next_frame draws nothing; the flood
// follows BreadthFirst layers over the generated tree.

.equ HAVE_smoke, 1

.equ SMOKE.starting_color,      0
.equ SMOKE.smoke_symbols,       8       // *const u64 packed symbols
.equ SMOKE.smoke_symbol_count,  16
.equ SMOKE.smoke_stops,         24      // *const u64
.equ SMOKE.smoke_stop_count,    32
.equ SMOKE.whole_canvas,        40
.equ SMOKE.final_stops,         48      // *const u64
.equ SMOKE.final_stop_count,    56
.equ SMOKE.final_steps,         64      // *const i64
.equ SMOKE.final_step_count,    72
.equ SMOKE.final_direction,     80
.equ SMOKE_size,                88

// scene names
.equ SMK_PAINT,                 NAME_LITERAL + 0
.equ SMK_SMOKE,                 NAME_LITERAL + 1

    .text

// smoke_build: SmokeIterator.__init__ + build().
smoke_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    ldr     x9, [x19, #SMOKE.whole_canvas]
    cmp     x9, #0
    cset    w21, eq                     // limit to the text boundary
    mov     w0, w21
    bl      pw_new
    // the fill start: a random coord's character, else BreadthFirst's own draw
    mov     w0, #0
    mov     w1, w21
    bl      canvas_random_coord
    mov     x1, x9
    bl      char_at_input_coord
    mov     w0, w9
    mov     w1, w21
    bl      bf_new
    // the final gradient over the text rectangle
    bl      smk_final_map
    // smoke gradient: smoke stops then the final stops reversed, steps (3, 4)
    ldr     x9, [x19, #SMOKE.smoke_stop_count]
    ldr     x10, [x19, #SMOKE.final_stop_count]
    add     x9, x9, x10
    STX     x9, smk_smoke_stop_count
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, smk_smoke_stops
    mov     x0, x9
    ldr     x1, [x19, #SMOKE.smoke_stops]
    ldr     x3, [x19, #SMOKE.smoke_stop_count]
    REP_MOVSQ
    ldr     x3, [x19, #SMOKE.final_stop_count]
    ldr     x1, [x19, #SMOKE.final_stops]
smoke_build.reverse:
    add     x9, x1, x3, lsl #3
    ldur    x9, [x9, #-8]
    str     x9, [x0], #8
    subs    x3, x3, #1
    b.ne    smoke_build.reverse
    ADRG    x0, smk_three_four
    mov     w3, #2
    LDX     x1, smk_smoke_stop_count
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, smk_smoke_spectrum
    LDX     x0, smk_smoke_stops
    LDX     x1, smk_smoke_stop_count
    ADRG    x2, smk_three_four
    mov     w3, #2
    LDX     x4, smk_smoke_spectrum
    bl      gradient_new
    STX     x9, smk_smoke_len
    // paint gradient stops: the final stops then the character's final color
    ldr     x9, [x19, #SMOKE.final_stop_count]
    add     x9, x9, #1
    STX     x9, smk_paint_stop_count
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, smk_paint_stops
    mov     x0, x9
    ldr     x1, [x19, #SMOKE.final_stops]
    ldr     x3, [x19, #SMOKE.final_stop_count]
    REP_MOVSQ
    ADRG    x0, smk_five
    mov     w3, #1
    LDX     x1, smk_paint_stop_count
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, smk_paint_spectrum
    // every input and fill character, top to bottom, left to right
    mov     w0, #(FILTER_INPUT | FILTER_INNER_FILL | FILTER_OUTER_FILL)
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x23, x9
    mov     x24, x2
    mov     x22, #0
smoke_build.character:
    cmp     x22, x24
    b.hs    smoke_build.generate
    ldr     w0, [x23, x22, lsl #2]
    bl      smk_character
    add     x22, x22, #1
    b       smoke_build.character
smoke_build.generate:
    bl      pw_run
    // the starting character is never 'explored': start it by hand
    LDW     w0, bf_start
    MOV64   w1, SMK_SMOKE
    bl      scene_activate_name
    LDW     w0, bf_start
    bl      active_insert
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// smk_character(w0=slot): one character's paint and smoke scenes, the
// smoke -> paint event and its starting appearance.
smk_character:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w19, w0
    bl      set_visible
    // final colors (x21 fg, x22 bg) and base colors (x23 fg; bg is none)
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    smk_character.mapped
    LDX     x9, ch_fg
    ldr     x21, [x9, x19, lsl #3]
    LDX     x9, ch_bg
    ldr     x22, [x9, x19, lsl #3]
    mov     x23, #0                     // 000000
    b       smk_character.paint
smk_character.mapped:
    mov     x21, #0                     // black outside the text rectangle
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x19, lsl #2]
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x19, lsl #2]
    LDX     x10, text_bottom
    cmp     x9, x10
    b.lt    smk_character.outside
    LDX     x11, text_top
    cmp     x9, x11
    b.gt    smk_character.outside
    LDX     x11, text_left
    cmp     x3, x11
    b.lt    smk_character.outside
    LDX     x12, text_right
    cmp     x3, x12
    b.gt    smk_character.outside
    sub     x9, x9, x10
    LDX     x12, smk_map_width
    mul     x9, x9, x12
    add     x9, x9, x3
    sub     x9, x9, x11
    LDX     x3, smk_map
    ldr     x21, [x3, x9, lsl #3]
smk_character.outside:
    mov     x22, #NONE
    LDX     x9, effect_config
    ldr     x23, [x9, #SMOKE.starting_color]
smk_character.paint:
    mov     w0, w19
    MOV64   w1, SMK_PAINT
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    smk_character.paint_gradient
    mov     w0, w20
    LDX     x1, ch_sym
    ldr     x1, [x1, x19, lsl #3]
    mov     w2, #5
    mov     x3, x21
    mov     x4, x22
    mov     w5, #0
    bl      scene_add_frame
    b       smk_character.smoke
smk_character.paint_gradient:
    // Gradient(*final stops, final fg, steps=5) over the input symbol: a
    // frame per color, the visuals shared by symbol and final fg
    LDX     x0, ch_sym
    ldr     x0, [x0, x19, lsl #3]
    mov     x1, x21
    mov     x2, #NONE
    bl      visual_run_find
    cbnz    x9, smk_character.paint_frames
    LDX     x9, smk_paint_stop_count
    LDX     x3, smk_paint_stops
    add     x10, x3, x9, lsl #3
    stur    x21, [x10, #-8]
    mov     x0, x3
    mov     x1, x9
    ADRG    x2, smk_five
    mov     w3, #1
    LDX     x4, smk_paint_spectrum
    bl      gradient_new
    LDX     x0, ch_sym
    ldr     x0, [x0, x19, lsl #3]
    LDX     x1, smk_paint_spectrum
    mov     w2, w9
    mov     x3, #NONE
    mov     x4, x21
    bl      visual_run
smk_character.paint_frames:
    mov     w0, w20
    mov     x1, x9
    mov     w3, #5
    bl      visual_frames
smk_character.smoke:
    mov     w0, w19
    MOV64   w1, SMK_SMOKE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    LDX     x9, effect_config
    LDX     x10, cfg_existing_colors
    cmp     x10, #1
    b.ne    smk_character.smoke_gradient
    mov     x24, #0
smk_character.smoke_frame:
    LDX     x9, effect_config
    ldr     x10, [x9, #SMOKE.smoke_symbol_count]
    cmp     x24, x10
    b.hs    smk_character.event
    ldr     x1, [x9, #SMOKE.smoke_symbols]
    ldr     x1, [x1, x24, lsl #3]
    mov     w0, w20
    mov     w2, #10
    mov     x3, x21
    mov     x4, x22
    mov     w5, #0
    bl      scene_add_frame
    add     x24, x24, #1
    b       smk_character.smoke_frame
smk_character.smoke_gradient:
    // the same frames for every character: the first one's, copied, unless
    // either scene applies preexisting colors
    SCENE_PTR x3, x20
    ldr     w10, [x3, #SC_FLAGS]
    tst     w10, #(SCF_PREEXISTING | SCF_PRE_BOLD)
    b.ne    smk_character.smoke_apply
    LDW     w2, smk_smoke_template
    cbz     w2, smk_character.smoke_first
    sub     w2, w2, #1
    SCENE_PTR x2, x2
    mov     w0, w20
    ldr     x1, [x2, #SC_FRAMES]
    ldr     w2, [x2, #SC_COUNT]
    bl      scene_append_frames
    b       smk_character.event
smk_character.smoke_first:
    add     w3, w20, #1
    STW     w3, smk_smoke_template
smk_character.smoke_apply:
    mov     w0, w20
    ldr     x1, [x9, #SMOKE.smoke_symbols]
    ldr     x2, [x9, #SMOKE.smoke_symbol_count]
    mov     w3, #3
    LDX     x4, smk_smoke_spectrum
    LDX     x5, smk_smoke_len
    mov     x6, #0
    mov     x7, #0
    bl      scene_apply_gradient
smk_character.event:
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    MOV64   x3, SMK_SMOKE
    mov     w4, #ACT_ACTIVATE_SCENE
    MOV64   x5, SMK_PAINT
    mov     x6, #0
    bl      event_register
    mov     w0, w19
    mov     x1, #0
    mov     x2, x23
    mov     x3, #NONE
    bl      set_appearance
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// smk_final_map: Gradient::new(final stops, final steps) mapped over the
// text rectangle.
smk_final_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #SMOKE.final_steps]
    ldr     x3, [x19, #SMOKE.final_step_count]
    ldr     x1, [x19, #SMOKE.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, smk_final_spectrum
    ldr     x0, [x19, #SMOKE.final_stops]
    ldr     x1, [x19, #SMOKE.final_stop_count]
    ldr     x2, [x19, #SMOKE.final_steps]
    ldr     x3, [x19, #SMOKE.final_step_count]
    LDX     x4, smk_final_spectrum
    bl      gradient_new
    LDX     x0, smk_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, smk_map_width
    ldr     x6, [x19, #SMOKE.final_direction]
    bl      gradient_map
    STX     x9, smk_map
    POP2    x19, x30
    ret

// smoke_next_frame -> w9 = 1 for a frame, 0 when done.
smoke_next_frame:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    LDB     w9, bf_complete
    cbz     w9, smoke_next_frame.flood
    bl      active_empty
    cbnz    w9, smoke_next_frame.finished
    b       smoke_next_frame.tick
smoke_next_frame.flood:
    bl      bf_step
    mov     x21, x9
    mov     x22, x2
    mov     x19, #0
smoke_next_frame.explored:
    cmp     x19, x22
    b.hs    smoke_next_frame.tick
    ldr     w0, [x21, x19, lsl #2]
    MOV64   w1, SMK_SMOKE
    bl      scene_activate_name
    ldr     w0, [x21, x19, lsl #2]
    bl      active_insert
    add     x19, x19, #1
    b       smoke_next_frame.explored
smoke_next_frame.tick:
    bl      update
    mov     w9, #1
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret
smoke_next_frame.finished:
    mov     w9, #0
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

    .section .rodata
    .balign 8
smk_three_four:     .quad 3, 4
smk_five:           .quad 5

    TSTATE
    .balign 8
smk_smoke_template:     .skip 4     // scene + 1 whose frames every smoke scene copies
    .balign 8
smk_map:                .skip 8
smk_map_width:          .skip 8
smk_final_spectrum:     .skip 8
smk_smoke_stops:        .skip 8
smk_smoke_stop_count:   .skip 8
smk_smoke_spectrum:     .skip 8
smk_smoke_len:          .skip 8
smk_paint_stops:        .skip 8
smk_paint_stop_count:   .skip 8
smk_paint_spectrum:     .skip 8
