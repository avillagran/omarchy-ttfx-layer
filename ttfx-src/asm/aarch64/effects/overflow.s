// effects/overflow.s - overflow (src/effects/overflow.rs).
//
// Rows scroll up from the bottom of the canvas: randint(lower, upper) cycles
// of shuffled copies of the input rows colored by the overflow gradient,
// then the real rows (input and fill characters) in their final colors.
//
// Every row (OverflowIterator.Row) is a 32-byte record in one array in
// pending order, so pending_rows is an index into it. A copied row's
// characters are consecutive in added_chars, which never moves. Active rows
// are a u32 list of row indices, retained in order like Vec::retain.

.equ HAVE_overflow, 1

.equ of_config.stops,               0       // overflow gradient stops
.equ of_config.stop_count,          8
.equ of_config.cycles_min,          16      // overflow_cycles_range
.equ of_config.cycles_max,          24
.equ of_config.speed,               32
.equ of_config.final_stops,         40
.equ of_config.final_stop_count,    48
.equ of_config.final_steps,         56
.equ of_config.final_step_count,    64
.equ of_config.final_direction,     72
.equ of_config_size,                80

// Row record
.equ OF_SLOTS,      0                       // u32* characters
.equ OF_COUNT,      8                       // u64 len(characters)
.equ OF_FINAL,      16                      // u32 Row.final_
.equ OF_LAST,       20                      // u32 spectrum index last applied, or NONE
.equ OF_SHIFT,      5

    .text

// overflow_build: Overflow::build.
// Locals: [sp, #56] total rows, [sp, #64] count, [sp, #72] source slots.
overflow_build:
    stp     x19, x20, [sp, #-80]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    bl      of_final_color_map
    // rows = get_characters_grouped(default filter, RowTopToBottom)
    mov     w0, #FILTER_INPUT
    mov     w1, #GROUP_ROW_TOP_TO_BOTTOM
    bl      get_characters_grouped
    mov     x21, x9                     // groups
    mov     x22, x2                     // group count
    // pointers to the group records, shuffled in place cycle after cycle
    lsl     x0, x22, #3
    add     x0, x0, #64
    bl      alloc
    mov     x23, x9
    mov     x3, #0
overflow_build.pointer:
    cmp     x3, x22
    b.hs    overflow_build.cycles
    lsl     x9, x3, #4
    add     x9, x9, x21
    str     x9, [x23, x3, lsl #3]
    add     x3, x3, #1
    b       overflow_build.pointer
overflow_build.cycles:
    mov     x24, #0                     // cycles
    ldr     x9, [x19, #of_config.cycles_max]
    cmp     x9, #0
    b.le    overflow_build.reserve
    ldr     x0, [x19, #of_config.cycles_min]
    ldr     x1, [x19, #of_config.cycles_max]
    bl      rng_randint
    mov     x24, x9
overflow_build.reserve:
    // rows: cycles * len(rows) copies plus at most canvas_top + 1 final rows
    mul     x9, x24, x22
    LDX     x10, canvas_top
    add     x9, x9, x10
    add     x9, x9, #1
    str     x9, [sp, #56]
    lsl     x9, x9, #OF_SHIFT
    add     x0, x9, #64
    bl      reserve
    STX     x9, of_rows
    ldr     x0, [sp, #56]
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      reserve
    STX     x9, of_active
    STX     xzr, of_row_count
    LDX     x9, cfg_existing_colors
    cbz     x9, overflow_build.cycle    // input colors win: no visual cache
    bl      of_symbol_ids
overflow_build.cycle:
    cbz     x24, overflow_build.final_rows
    sub     x24, x24, #1
    mov     x0, x23
    mov     x1, x22
    bl      rng_shuffle64
    mov     x20, #0                     // row
overflow_build.copy_row:
    cmp     x20, x22
    b.hs    overflow_build.cycle
    // the copies land consecutively in added_chars
    LDW     w9, added_count
    LDX     x3, added_chars
    add     x9, x3, x9, lsl #2
    ldr     x2, [x23, x20, lsl #3]
    ldr     x3, [x2, #8]
    str     x3, [sp, #64]               // count
    ldr     x2, [x2]
    str     x2, [sp, #72]               // source slots
    mov     w1, #0
    mov     x0, x9
    bl      of_push_row
    mov     x19, #0
overflow_build.copy_char:
    ldr     x9, [sp, #64]
    cmp     x19, x9
    b.hs    overflow_build.copied
    ldr     x9, [sp, #72]
    ldr     w21, [x9, x19, lsl #2]      // source slot
    mov     w0, w21
    bl      char_input_coord
    mov     x1, x9
    LDX     x0, ch_sym
    ldr     x0, [x0, x21, lsl #3]
    bl      add_character
    // uses_input_preexisting_colors, and the input colors (not the bold)
    LDX     x3, ch_flags
    ldrh    w2, [x3, x9, lsl #1]
    orr     w2, w2, #CF_PREEXISTING
    strh    w2, [x3, x9, lsl #1]
    LDX     x3, ch_fg
    ldr     x2, [x3, x21, lsl #3]
    str     x2, [x3, x9, lsl #3]
    LDX     x3, ch_bg
    ldr     x2, [x3, x21, lsl #3]
    str     x2, [x3, x9, lsl #3]
    LDX     x3, of_symid
    cbz     x3, overflow_build.copy_next
    ldr     w2, [x3, x21, lsl #2]       // the copy has the source's symbol
    str     w2, [x3, x9, lsl #2]
overflow_build.copy_next:
    add     x19, x19, #1
    b       overflow_build.copy_char
overflow_build.copied:
    add     x20, x20, #1
    b       overflow_build.copy_row
overflow_build.final_rows:
    // the real rows, top to bottom, in their final appearance
    mov     w0, #(FILTER_INPUT | FILTER_INNER_FILL | FILTER_OUTER_FILL)
    mov     w1, #GROUP_ROW_TOP_TO_BOTTOM
    bl      get_characters_grouped
    mov     x21, x9
    mov     x22, x2
    mov     x20, #0
overflow_build.final_row:
    cmp     x20, x22
    b.hs    overflow_build.spectrum
    lsl     x23, x20, #4
    add     x23, x23, x21               // group record
    mov     x19, #0
overflow_build.final_char:
    ldr     x9, [x23, #8]
    cmp     x19, x9
    b.hs    overflow_build.final_push
    ldr     x9, [x23]
    ldr     w24, [x9, x19, lsl #2]      // slot
    // the symbol of current_character_visual
    LDX     x9, ch_handle
    ldr     w9, [x9, x24, lsl #2]
    bl      visual_meta
    ldur    x1, [x9, #VH_SYMBOL]
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    overflow_build.final_color
    LDX     x2, ch_fg
    ldr     x2, [x2, x24, lsl #3]
    LDX     x3, ch_bg
    ldr     x3, [x3, x24, lsl #3]
    b       overflow_build.appearance
overflow_build.final_color:
    // character_final_color_map: the mapped color inside the text box,
    // 000000 outside it
    mov     x2, #0
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x24, lsl #2]
    LDX     x10, text_bottom
    subs    x9, x9, x10
    b.lt    overflow_build.final_fg
    LDX     x10, of_map_height
    cmp     x9, x10
    b.ge    overflow_build.final_fg
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x24, lsl #2]
    LDX     x10, text_left
    subs    x3, x3, x10
    b.lt    overflow_build.final_fg
    LDX     x10, of_map_width
    cmp     x3, x10
    b.ge    overflow_build.final_fg
    mul     x9, x9, x10
    add     x9, x9, x3
    LDX     x2, of_map
    ldr     x2, [x2, x9, lsl #3]
overflow_build.final_fg:
    mov     x3, #NONE
overflow_build.appearance:
    mov     w0, w24
    bl      set_appearance
    add     x19, x19, #1
    b       overflow_build.final_char
overflow_build.final_push:
    ldr     x0, [x23]
    ldr     x3, [x23, #8]
    mov     w1, #1
    bl      of_push_row
    add     x20, x20, #1
    b       overflow_build.final_row
overflow_build.spectrum:
    // steps = max(canvas_top // max(1, len(stops) - 1), 1)
    LDX     x19, effect_config
    ldr     x3, [x19, #of_config.stop_count]
    sub     x3, x3, #1
    mov     x9, #1
    cmp     x3, #1
    csel    x3, x9, x3, lt
    LDX     x9, canvas_top
    sdiv    x9, x9, x3
    cmp     x9, #1
    b.ge    overflow_build.steps
    mov     x9, #1
overflow_build.steps:
    STX     x9, of_steps
    ADRG    x0, of_steps
    mov     x3, #1
    ldr     x1, [x19, #of_config.stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    add     x0, x0, #64
    bl      alloc
    STX     x9, of_spectrum
    ldr     x0, [x19, #of_config.stops]
    ldr     x1, [x19, #of_config.stop_count]
    ADRG    x2, of_steps
    mov     w3, #1
    LDX     x4, of_spectrum
    bl      gradient_new
    mov     w9, w9
    STX     x9, of_spectrum_len
    LDX     x3, of_symid
    cbz     x3, overflow_build.no_cache
    // the visual cache: one handle per (spectrum index, symbol id), 0 = unmade
    LDX     x3, of_nsym
    mul     x9, x9, x3
    lsl     x0, x9, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, of_hcache
overflow_build.no_cache:
    STX     xzr, of_delay
    STX     xzr, of_next
    STX     xzr, of_active_count
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #80
    ret

// of_symbol_ids(x24=cycles): number the distinct input symbols of the
// input characters (x21=groups, x22=group count) into of_symid, a per-slot
// array sized for the copies still to come. Preserves x19, x21-x24.
of_symbol_ids:
    PUSH2   x19, x20
    PUSH2   x23, x30
    // slots: every current one plus cycles copies of the input
    add     x9, x24, #1
    LDW     w3, char_count
    mul     x9, x9, x3
    lsl     x0, x9, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, of_symid
    // open-addressed symbol table: 16-byte (symbol, id) entries, at least
    // twice as many as there are input characters
    LDW     w9, char_count
    add     x9, x9, x9
    mov     x3, #16
    mov     w2, #0
of_symbol_ids.size:
    cmp     x3, x9
    b.hs    of_symbol_ids.sized
    add     x3, x3, x3
    add     w2, w2, #1
    b       of_symbol_ids.size
of_symbol_ids.sized:
    add     w2, w2, #4                  // log2 of the entry count
    mov     w3, #64
    sub     w3, w3, w2
    STX     x3, of_symshift
    mov     x0, #16
    lsl     x0, x0, x2
    lsr     x3, x0, #4
    sub     x3, x3, #1
    STX     x3, of_symmask
    add     x0, x0, #64
    bl      alloc
    STX     x9, of_symtab
    STX     xzr, of_nsym
    mov     x20, #0                     // group
of_symbol_ids.group:
    cmp     x20, x22
    b.hs    of_symbol_ids.done
    lsl     x23, x20, #4
    add     x23, x23, x21
    mov     x19, #0
of_symbol_ids.char:
    ldr     x9, [x23, #8]
    cmp     x19, x9
    b.hs    of_symbol_ids.next_group
    ldr     x9, [x23]
    ldr     w4, [x9, x19, lsl #2]       // slot
    LDX     x9, ch_sym
    ldr     x0, [x9, x4, lsl #3]
    // probe
    MOV64   x9, 0x9E3779B97F4A7C15
    mul     x9, x9, x0
    LDX     x3, of_symshift
    lsr     x9, x9, x3
    LDX     x1, of_symtab
of_symbol_ids.probe:
    lsl     x2, x9, #4
    ldr     x3, [x1, x2]
    cbz     x3, of_symbol_ids.new
    cmp     x3, x0
    b.eq    of_symbol_ids.found
    add     x9, x9, #1
    LDX     x3, of_symmask
    and     x9, x9, x3
    b       of_symbol_ids.probe
of_symbol_ids.new:
    str     x0, [x1, x2]
    LDX     x3, of_nsym
    add     x5, x1, x2
    str     x3, [x5, #8]
    add     x3, x3, #1
    STX     x3, of_nsym
of_symbol_ids.found:
    add     x5, x1, x2
    ldr     w3, [x5, #8]
    LDX     x9, of_symid
    str     w3, [x9, x4, lsl #2]
    add     x19, x19, #1
    b       of_symbol_ids.char
of_symbol_ids.next_group:
    add     x20, x20, #1
    b       of_symbol_ids.group
of_symbol_ids.done:
    POP2    x23, x30
    POP2    x19, x20
    ret

// of_push_row(x0=slots, x3=count, w1=final): pending_rows.push_back(Row).
of_push_row:
    LDX     x9, of_row_count
    add     x10, x9, #1
    STX     x10, of_row_count
    lsl     x9, x9, #OF_SHIFT
    LDX     x10, of_rows
    add     x9, x9, x10
    str     x0, [x9, #OF_SLOTS]
    str     x3, [x9, #OF_COUNT]
    str     w1, [x9, #OF_FINAL]
    mov     w10, #NONE
    str     w10, [x9, #OF_LAST]
    ret

// of_final_color_map: Gradient::new(final stops, final steps) mapped over the
// text rectangle (build_coordinate_color_mapping), row-major.
of_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #of_config.final_steps]
    ldr     x3, [x19, #of_config.final_step_count]
    ldr     x1, [x19, #of_config.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, of_final_spectrum
    ldr     x0, [x19, #of_config.final_stops]
    ldr     x1, [x19, #of_config.final_stop_count]
    ldr     x2, [x19, #of_config.final_steps]
    ldr     x3, [x19, #of_config.final_step_count]
    LDX     x4, of_final_spectrum
    bl      gradient_new
    LDX     x0, of_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, of_map_width
    sub     x9, x3, x2
    add     x9, x9, #1
    STX     x9, of_map_height
    ldr     x6, [x19, #of_config.final_direction]
    bl      gradient_map
    STX     x9, of_map
    POP2    x19, x30
    ret

// overflow_next_frame -> w9 = 1 for a frame, 0 when done.
// Locals: [sp, #56] rows still to push this frame.
overflow_next_frame:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x9, of_next
    LDX     x10, of_row_count
    cmp     x9, x10
    b.hs    overflow_next_frame.done
    LDX     x9, of_delay
    cbnz    x9, overflow_next_frame.wait
    LDX     x9, effect_config
    mov     x0, #1
    ldr     x1, [x9, #of_config.speed]
    bl      rng_randint
    str     x9, [sp, #56]
overflow_next_frame.push:
    ldr     x9, [sp, #56]
    cmp     x9, #0
    b.le    overflow_next_frame.delay
    sub     x9, x9, #1
    str     x9, [sp, #56]
    LDX     x9, of_next
    LDX     x10, of_row_count
    cmp     x9, x10
    b.hs    overflow_next_frame.delay   // out of rows: the rest do nothing
    // move every active row up, recoloring the overflow rows by height
    mov     x21, #0
overflow_next_frame.active_row:
    LDX     x9, of_active_count
    cmp     x21, x9
    b.hs    overflow_next_frame.next_row
    LDX     x9, of_active
    ldr     w19, [x9, x21, lsl #2]
    lsl     x19, x19, #OF_SHIFT
    LDX     x9, of_rows
    add     x19, x19, x9
    mov     x0, x19
    bl      of_move_up
    ldr     w9, [x19, #OF_FINAL]
    cbnz    w9, overflow_next_frame.active_next
    ldr     x9, [x19, #OF_SLOTS]
    ldr     w9, [x9]
    LDX     x3, ch_row
    ldrsw   x1, [x3, x9, lsl #2]        // head row
    LDX     x9, of_spectrum_len
    sub     x9, x9, #1
    cmp     x1, x9
    csel    x1, x9, x1, gt
    mov     x0, x19
    bl      of_set_color
overflow_next_frame.active_next:
    add     x21, x21, #1
    b       overflow_next_frame.active_row
overflow_next_frame.next_row:
    // pending_rows.pop_front(): setup, move_up, color, reveal
    LDX     x22, of_next
    add     x9, x22, #1
    STX     x9, of_next
    lsl     x19, x22, #OF_SHIFT
    LDX     x9, of_rows
    add     x19, x19, x9
    mov     x21, #0
overflow_next_frame.setup:
    ldr     x9, [x19, #OF_COUNT]
    cmp     x21, x9
    b.hs    overflow_next_frame.setup_done
    ldr     x9, [x19, #OF_SLOTS]
    ldr     w23, [x9, x21, lsl #2]
    // (input column, 0) then one row up
    LDX     x9, ch_icol
    ldr     w1, [x9, x23, lsl #2]
    orr     x1, x1, #(1 << 32)
    mov     w0, w23
    bl      set_coordinate
    add     x21, x21, #1
    b       overflow_next_frame.setup
overflow_next_frame.setup_done:
    ldr     w9, [x19, #OF_FINAL]
    cbnz    w9, overflow_next_frame.reveal
    mov     x0, x19
    mov     w1, #0
    bl      of_set_color
overflow_next_frame.reveal:
    mov     x21, #0
overflow_next_frame.reveal_char:
    ldr     x9, [x19, #OF_COUNT]
    cmp     x21, x9
    b.hs    overflow_next_frame.activate
    ldr     x9, [x19, #OF_SLOTS]
    ldr     w0, [x9, x21, lsl #2]
    bl      set_visible
    add     x21, x21, #1
    b       overflow_next_frame.reveal_char
overflow_next_frame.activate:
    LDX     x9, of_active_count
    add     x10, x9, #1
    STX     x10, of_active_count
    LDX     x3, of_active
    str     w22, [x3, x9, lsl #2]
    b       overflow_next_frame.push
overflow_next_frame.delay:
    mov     x0, #0
    mov     x1, #3
    bl      rng_randint
    STX     x9, of_delay
    b       overflow_next_frame.retain
overflow_next_frame.wait:
    sub     x9, x9, #1
    STX     x9, of_delay
overflow_next_frame.retain:
    // active_rows.retain(head row <= canvas_top)
    LDX     x4, of_active
    LDX     x5, of_rows
    LDX     x10, ch_row
    LDX     x11, canvas_top
    LDX     x12, of_active_count
    mov     x3, #0                      // read
    mov     x2, #0                      // write
overflow_next_frame.keep:
    cmp     x3, x12
    b.hs    overflow_next_frame.kept
    ldr     w9, [x4, x3, lsl #2]
    lsl     x1, x9, #OF_SHIFT
    add     x1, x1, x5
    ldr     x1, [x1, #OF_SLOTS]
    ldr     w1, [x1]
    ldrsw   x1, [x10, x1, lsl #2]
    cmp     x1, x11
    b.gt    overflow_next_frame.drop
    str     w9, [x4, x2, lsl #2]
    add     x2, x2, #1
overflow_next_frame.drop:
    add     x3, x3, #1
    b       overflow_next_frame.keep
overflow_next_frame.kept:
    STX     x2, of_active_count
    bl      update
    mov     w9, #1
    b       overflow_next_frame.out
overflow_next_frame.done:
    mov     w9, #0
overflow_next_frame.out:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// of_move_up(x0=row): Row.move_up - every character one row up.
of_move_up:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     x19, x0
    mov     x21, #0
of_move_up.char:
    ldr     x9, [x19, #OF_COUNT]
    cmp     x21, x9
    b.hs    of_move_up.done
    ldr     x9, [x19, #OF_SLOTS]
    ldr     w22, [x9, x21, lsl #2]
    mov     w0, w22
    bl      char_coord
    mov     x3, #(1 << 32)
    add     x1, x9, x3
    mov     w0, w22
    bl      set_coordinate
    add     x21, x21, #1
    b       of_move_up.char
of_move_up.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// of_set_color(x0=row, x1=spectrum index): Row.set_color(spectrum[index],
// None) - the input symbol in that color. The visual depends only on the
// symbol and the color, so a row already showing that index is unchanged.
of_set_color:
    ldr     w9, [x0, #OF_LAST]
    cmp     w9, w1
    b.eq    of_set_color.same
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     x19, x0
    str     w1, [x19, #OF_LAST]
    LDX     x9, of_spectrum
    ldr     x22, [x9, x1, lsl #3]       // color
    mov     x21, #0
    LDX     x9, of_hcache
    cbz     x9, of_set_color.char
    // set_appearance through the (index, symbol) cache: the visual is
    // (input symbol, color, no bg, no attributes) for every copy here
    PUSH2   x23, x24
    LDX     x3, of_nsym
    mul     x1, x1, x3
    add     x23, x9, x1, lsl #2         // this index's handles
of_set_color.cached:
    ldr     x9, [x19, #OF_COUNT]
    cmp     x21, x9
    b.hs    of_set_color.cached_done
    ldr     x9, [x19, #OF_SLOTS]
    ldr     w0, [x9, x21, lsl #2]
    LDX     x9, of_symid
    ldr     w24, [x9, x0, lsl #2]
    ldr     w9, [x23, x24, lsl #2]
    cbnz    w9, of_set_color.have
    LDX     x9, ch_sym
    ldr     x2, [x9, x0, lsl #3]
    mov     x0, x22
    mov     x1, #NONE
    mov     w3, #0
    bl      visual_make
    str     w9, [x23, x24, lsl #2]
    ldr     x3, [x19, #OF_SLOTS]
    ldr     w0, [x3, x21, lsl #2]
of_set_color.have:
    // (no doze_wake: a copy never joins the active set, so never dozes)
    SET_HANDLE
    add     x21, x21, #1
    b       of_set_color.cached
of_set_color.cached_done:
    POP2    x23, x24
    b       of_set_color.done
of_set_color.char:
    ldr     x9, [x19, #OF_COUNT]
    cmp     x21, x9
    b.hs    of_set_color.done
    ldr     x9, [x19, #OF_SLOTS]
    ldr     w0, [x9, x21, lsl #2]
    mov     x1, #0
    mov     x2, x22
    mov     x3, #NONE
    bl      set_appearance
    add     x21, x21, #1
    b       of_set_color.char
of_set_color.done:
    POP2    x22, x30
    POP2    x19, x21
of_set_color.same:
    ret

    TSTATE
    .balign 8
of_rows:            .skip 8
of_row_count:       .skip 8
of_next:            .skip 8             // first pending row
of_active:          .skip 8             // u32 row indices
of_active_count:    .skip 8
of_delay:           .skip 8
of_steps:           .skip 8
of_spectrum:        .skip 8
of_spectrum_len:    .skip 8
of_final_spectrum:  .skip 8
of_map:             .skip 8
of_map_width:       .skip 8
of_map_height:      .skip 8
of_symid:           .skip 8             // u32 symbol id per slot, or 0 = no cache
of_symtab:          .skip 8             // (symbol, id) entries
of_symshift:        .skip 8
of_symmask:         .skip 8
of_nsym:            .skip 8
of_hcache:          .skip 8             // u32 handles [index * nsym + id]

    .text
