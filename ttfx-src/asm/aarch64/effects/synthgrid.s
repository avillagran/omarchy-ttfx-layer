// effects/synthgrid.s - "Create a grid which fills with characters
// dissolving into the final text" (src/effects/synthgrid.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Synthgrid):
//
// Grid lines are consecutive added slots, so a line is (first slot, count)
// plus how many of its characters are extended. Extension reveals from the
// front; Rust's collapse reverses the fully extended list once and then
// hides from its front, which is hiding from the back of the line, so the
// extended characters are always a prefix [first, first + ext).
//
// Groups (blocks) keep their members in one flat array in group-number
// order. pending_groups is a shuffled array of group numbers consumed from
// the front; the group tracker is an i64 per group number.

.equ HAVE_synthgrid, 1

.equ SYNTHGRID.grid_stops,          0       // *const u64
.equ SYNTHGRID.grid_stop_count,     8
.equ SYNTHGRID.grid_steps,          16      // *const i64
.equ SYNTHGRID.grid_step_count,     24
.equ SYNTHGRID.grid_direction,      32
.equ SYNTHGRID.text_stops,          40      // *const u64
.equ SYNTHGRID.text_stop_count,     48
.equ SYNTHGRID.text_steps,          56      // *const i64
.equ SYNTHGRID.text_step_count,     64
.equ SYNTHGRID.text_direction,      72
.equ SYNTHGRID.row_symbol,          80      // packed symbol
.equ SYNTHGRID.column_symbol,       88      // packed symbol
.equ SYNTHGRID.gen_symbols,         96      // *const u64 packed symbols
.equ SYNTHGRID.gen_symbol_count,    104
.equ SYNTHGRID.max_active_blocks,   112     // f64
.equ SYNTHGRID_size,                120

// grid line record
.equ SG_LN_FIRST,        0              // u32 first slot
.equ SG_LN_COUNT,        4              // u32 characters
.equ SG_LN_EXT,          8              // u32 extended prefix length
.equ SG_LN_STEP,         12             // u32 characters per extend/collapse
.equ SG_LINE_SIZE,       16

.equ SG_PH_GRID_EXPAND,  0
.equ SG_PH_ADD_CHARS,    1
.equ SG_PH_COLLAPSE,     2
.equ SG_PH_COMPLETE,     3

    .text

// synthgrid_build: SynthGrid::build.
synthgrid_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56]: column_gap
    LDX     x19, effect_config
    // --- the grid gradient, mapped over the whole canvas
    ldr     x0, [x19, #SYNTHGRID.grid_steps]
    ldr     x3, [x19, #SYNTHGRID.grid_step_count]
    ldr     x1, [x19, #SYNTHGRID.grid_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    mov     x21, x9
    ldr     x0, [x19, #SYNTHGRID.grid_stops]
    ldr     x1, [x19, #SYNTHGRID.grid_stop_count]
    ldr     x2, [x19, #SYNTHGRID.grid_steps]
    ldr     x3, [x19, #SYNTHGRID.grid_step_count]
    mov     x4, x21
    bl      gradient_new
    LDX     x10, canvas_top
    cmp     x10, #1
    b.lt    synthgrid_build.bad_max
    LDX     x10, canvas_right
    cmp     x10, #1
    b.lt    synthgrid_build.bad_max
    mov     x0, x21
    mov     w1, w9
    mov     x2, #1
    LDX     x3, canvas_top
    mov     x4, #1
    LDX     x5, canvas_right
    ldr     x6, [x19, #SYNTHGRID.grid_direction]
    bl      gradient_map
    STX     x9, sg_grid_map
    // --- the text gradient (its spectrum feeds the dissolve colors)
    ldr     x0, [x19, #SYNTHGRID.text_steps]
    ldr     x3, [x19, #SYNTHGRID.text_step_count]
    ldr     x1, [x19, #SYNTHGRID.text_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, sg_text_spectrum
    ldr     x0, [x19, #SYNTHGRID.text_stops]
    ldr     x1, [x19, #SYNTHGRID.text_stop_count]
    ldr     x2, [x19, #SYNTHGRID.text_steps]
    ldr     x3, [x19, #SYNTHGRID.text_step_count]
    LDX     x4, sg_text_spectrum
    bl      gradient_new
    mov     w9, w9
    STX     x9, sg_text_spectrum_len
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    cmp     x3, #1
    b.lt    synthgrid_build.bad_max
    cmp     x5, #1
    b.lt    synthgrid_build.bad_max
    cmp     x2, #1
    b.lt    synthgrid_build.bad_max
    cmp     x4, #1
    b.lt    synthgrid_build.bad_max
    cmp     x2, x3
    b.gt    synthgrid_build.bad_min
    cmp     x4, x5
    b.gt    synthgrid_build.bad_min
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, sg_text_map_width
    LDX     x0, sg_text_spectrum
    LDW     w1, sg_text_spectrum_len
    ldr     x6, [x19, #SYNTHGRID.text_direction]
    bl      gradient_map
    STX     x9, sg_text_map
    // memo of (generation symbol, spectrum color) -> handle
    ldr     x0, [x19, #SYNTHGRID.gen_symbol_count]
    LDX     x9, sg_text_spectrum_len
    mul     x0, x0, x9
    lsl     x0, x0, #2
    bl      alloc
    STX     x9, sg_gen_memo
    // --- grid lines: room for the four borders plus one per row and column
    LDX     x0, canvas_top
    LDX     x9, canvas_right
    add     x0, x0, x9
    add     x0, x0, #4
    lsl     x0, x0, #4                  // SG_LINE_SIZE
    bl      alloc
    STX     x9, sg_lines
    mov     w0, #0
    mov     x1, #1                      // bottom row
    bl      sg_make_grid_line
    mov     w0, #0
    LDX     x1, canvas_top
    bl      sg_make_grid_line
    mov     w0, #1
    mov     x1, #1                      // left column
    bl      sg_make_grid_line
    mov     w0, #1
    LDX     x1, canvas_right
    bl      sg_make_grid_line
    // row and column indexes (each list ends with its sentinel)
    LDX     x0, canvas_top
    lsl     x0, x0, #3
    add     x0, x0, #16
    bl      alloc
    STX     x9, sg_row_indexes
    LDX     x0, canvas_right
    lsl     x0, x0, #3
    add     x0, x0, #16
    bl      alloc
    STX     x9, sg_column_indexes
    LDX     x9, canvas_right
    add     x3, x9, x9
    LDX     x10, canvas_top
    cmp     x10, x3
    b.le    synthgrid_build.by_columns
    LDX     x0, canvas_top
    bl      sg_find_even_gap
    add     x21, x9, #1                 // row_gap
    add     x22, x21, x21               // column_gap
    b       synthgrid_build.gaps
synthgrid_build.by_columns:
    LDX     x0, canvas_right
    bl      sg_find_even_gap
    add     x22, x9, #1                 // column_gap (>= 1)
    lsr     x21, x22, #1                // row_gap = column_gap // 2
synthgrid_build.gaps:
    str     x22, [sp, #56]
    // range(bottom + row_gap, top, max(row_gap, 1))
    mov     x23, x21
    mov     x9, #1
    cmp     x23, #1
    csel    x23, x9, x23, lt            // row step
    add     x24, x21, #1                // row_index
    mov     x20, #0                     // row count
synthgrid_build.rows:
    LDX     x9, canvas_top
    cmp     x24, x9
    b.ge    synthgrid_build.rows_done
    sub     x9, x9, x24
    cmp     x9, #2
    b.lt    synthgrid_build.row_next
    LDX     x9, sg_row_indexes
    str     x24, [x9, x20, lsl #3]
    add     x20, x20, #1
    mov     w0, #0
    mov     x1, x24
    bl      sg_make_grid_line
synthgrid_build.row_next:
    add     x24, x24, x23
    b       synthgrid_build.rows
synthgrid_build.rows_done:
    LDX     x9, canvas_top
    add     x9, x9, #1
    LDX     x3, sg_row_indexes
    str     x9, [x3, x20, lsl #3]
    add     x20, x20, #1
    STX     x20, sg_row_index_count
    // range(left + column_gap, right, max(column_gap, 1))
    ldr     x22, [sp, #56]
    mov     x23, x22
    mov     x9, #1
    cmp     x23, #1
    csel    x23, x9, x23, lt
    add     x24, x22, #1
    mov     x20, #0
synthgrid_build.columns:
    LDX     x9, canvas_right
    cmp     x24, x9
    b.ge    synthgrid_build.columns_done
    sub     x9, x9, x24
    cmp     x9, #2
    b.lt    synthgrid_build.column_next
    LDX     x9, sg_column_indexes
    str     x24, [x9, x20, lsl #3]
    add     x20, x20, #1
    mov     w0, #1
    mov     x1, x24
    bl      sg_make_grid_line
synthgrid_build.column_next:
    add     x24, x24, x23
    b       synthgrid_build.columns
synthgrid_build.columns_done:
    LDX     x9, canvas_right
    add     x9, x9, #1
    LDX     x3, sg_column_indexes
    str     x9, [x3, x20, lsl #3]
    add     x20, x20, #1
    STX     x20, sg_column_index_count
    bl      sg_collect_groups
    bl      sg_build_dissolves
    // shuffle pending_groups (the group numbers, in group order)
    LDX     x0, sg_group_count
    lsl     x0, x0, #3
    bl      alloc
    STX     x9, sg_pending
    mov     x3, #0
    LDX     x10, sg_group_count
synthgrid_build.order:
    cmp     x3, x10
    b.hs    synthgrid_build.shuffle
    str     x3, [x9, x3, lsl #3]
    add     x3, x3, #1
    b       synthgrid_build.order
synthgrid_build.shuffle:
    LDX     x0, sg_pending
    LDX     x1, sg_group_count
    bl      rng_shuffle64
    STX     xzr, sg_pending_next
    mov     w9, #SG_PH_GRID_EXPAND
    STB     w9, sg_phase
    LDX     x9, sg_group_count
    cbnz    x9, synthgrid_build.built
    // no groups: every input character is shown and active at once
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    mov     x19, #0
synthgrid_build.show:
    cmp     x19, x22
    b.hs    synthgrid_build.built
    ldr     w0, [x21, x19, lsl #2]
    bl      set_visible
    ldr     w0, [x21, x19, lsl #2]
    bl      active_insert
    add     x19, x19, #1
    b       synthgrid_build.show
synthgrid_build.built:
    STX     xzr, sg_active_groups
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret
synthgrid_build.bad_max:
    FAIL    msg_sg_max
synthgrid_build.bad_min:
    FAIL    msg_sg_min

// sg_find_even_gap(x0=dimension) -> x9: SynthGrid::find_even_gap. The gap
// closest to (dimension - 2) // 5 among i in (dimension-2 .. 4] with
// (dimension-2) % i <= 1, the first (largest) on ties; 4 when none; 0 when
// dimension - 2 <= 0.
sg_find_even_gap:
    subs    x0, x0, #2
    b.le    sg_find_even_gap.zero
    mov     x4, x0
    mov     x8, #5
    sdiv    x5, x0, x8                  // target (dimension > 0: plain division)
    mov     x10, #-1                    // best gap, none yet
    mov     x11, #0                     // its key
    mov     x1, x4                      // i
sg_find_even_gap.scan:
    cmp     x1, #4
    b.le    sg_find_even_gap.done
    sdiv    x9, x4, x1
    msub    x2, x9, x1, x4
    cmp     x2, #1
    b.gt    sg_find_even_gap.next
    sub     x9, x1, x5
    cmp     x9, #0
    cneg    x3, x9, mi                  // |i - target|
    cmn     x10, #1
    b.eq    sg_find_even_gap.take
    cmp     x3, x11
    b.ge    sg_find_even_gap.next
sg_find_even_gap.take:
    mov     x10, x1
    mov     x11, x3
sg_find_even_gap.next:
    sub     x1, x1, #1
    b       sg_find_even_gap.scan
sg_find_even_gap.done:
    mov     x9, x10
    cmn     x9, #1
    b.ne    sg_find_even_gap.ret
    mov     x9, #4
sg_find_even_gap.ret:
    ret
sg_find_even_gap.zero:
    mov     x9, #0
    ret

// sg_make_grid_line(w0=0 horizontal / 1 vertical, x1=row or column):
// GridLine::new via make_grid_line. A horizontal line spans columns
// left..=right on its row, a vertical one rows bottom..top (top excluded) on
// its column. Each character: added at (0, 0), one scene with one frame of
// the grid color at its coordinate, activated, layer 2, then moved.
sg_make_grid_line:
    stp     x19, x20, [sp, #-80]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56]: line record, [sp, #64]: scene
    mov     w21, w0                     // direction
    mov     x22, x1                     // fixed row / column
    LDX     x19, effect_config
    ldr     x23, [x19, #SYNTHGRID.row_symbol]
    mov     w9, #3
    cbz     w21, sg_make_grid_line.kind
    ldr     x23, [x19, #SYNTHGRID.column_symbol]
    mov     w9, #1
sg_make_grid_line.kind:
    // the line record
    LDX     x3, sg_line_count
    lsl     x3, x3, #4
    LDX     x10, sg_lines
    add     x3, x3, x10
    str     x3, [sp, #56]
    LDW     w2, char_count
    str     w2, [x3, #SG_LN_FIRST]
    str     wzr, [x3, #SG_LN_COUNT]
    str     wzr, [x3, #SG_LN_EXT]
    str     w9, [x3, #SG_LN_STEP]
    LDX     x10, sg_line_count
    add     x10, x10, #1
    STX     x10, sg_line_count
    mov     w24, #1                     // the running column / row
sg_make_grid_line.each:
    cbnz    w21, sg_make_grid_line.vertical
    LDX     x9, canvas_right
    cmp     x24, x9
    b.gt    sg_make_grid_line.done
    lsl     x20, x22, #32
    orr     x20, x20, x24               // (column x24, row x22)
    b       sg_make_grid_line.make
sg_make_grid_line.vertical:
    LDX     x9, canvas_top
    cmp     x24, x9
    b.ge    sg_make_grid_line.done
    lsl     x20, x24, #32
    mov     w9, w22
    orr     x20, x20, x9                // (column x22, row x24)
sg_make_grid_line.make:
    mov     x0, x23
    mov     x1, #0
    bl      add_character
    mov     w19, w9                     // slot
    mov     w0, w9
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     x9, [sp, #64]
    // sg_grid_map[(row - 1) * right + column - 1]
    asr     x9, x20, #32
    sub     x9, x9, #1
    LDX     x10, canvas_right
    mul     x9, x9, x10
    sxtw    x3, w20
    add     x9, x9, x3
    LDX     x3, sg_grid_map
    add     x3, x3, x9, lsl #3
    ldur    x3, [x3, #-8]
    ldr     w0, [sp, #64]
    mov     x1, x23
    mov     w2, #1
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w19
    ldr     w1, [sp, #64]
    bl      scene_activate
    mov     w0, w19
    mov     x1, #2
    bl      set_layer
    mov     w0, w19
    mov     x1, x20
    bl      set_coordinate
    ldr     x3, [sp, #56]
    ldr     w9, [x3, #SG_LN_COUNT]
    add     w9, w9, #1
    str     w9, [x3, #SG_LN_COUNT]
    add     x24, x24, #1
    b       sg_make_grid_line.each
sg_make_grid_line.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #80
    ret

// sg_collect_groups: the blocks between consecutive row and column indexes,
// row-major inside each block; a block with any character is a group.
// Locals: [sp, #64] prev_row_index, 72 row_index, 80 prev_column_index,
// 88 group start, 96 canvas cells.
sg_collect_groups:
    stp     x19, x20, [sp, #-112]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    // members: at most one per canvas cell; groups: at most one per member
    LDX     x0, canvas_top
    LDX     x9, canvas_right
    mul     x0, x0, x9
    str     x0, [sp, #96]
    lsl     x0, x0, #2
    bl      alloc
    STX     x9, sg_members
    ldr     x0, [sp, #96]
    lsl     x0, x0, #3
    add     x0, x0, #8
    bl      alloc
    STX     x9, sg_groups               // (u32 start, u32 count) per group
    STX     xzr, sg_group_count
    mov     x24, #0                     // member count
    mov     x9, #1
    str     x9, [sp, #64]               // prev_row_index
    mov     x19, #0                     // row list position
sg_collect_groups.row:
    LDX     x9, sg_row_index_count
    cmp     x19, x9
    b.hs    sg_collect_groups.done
    LDX     x9, sg_row_indexes
    ldr     x9, [x9, x19, lsl #3]
    str     x9, [sp, #72]               // row_index
    mov     x9, #1
    str     x9, [sp, #80]               // prev_column_index
    mov     x20, #0                     // column list position
sg_collect_groups.column:
    LDX     x9, sg_column_index_count
    cmp     x20, x9
    b.hs    sg_collect_groups.row_done
    ldr     x9, [sp, #72]
    LDX     x10, canvas_top
    cmp     x9, x10
    b.ne    sg_collect_groups.block
    add     x9, x9, #1                  // make sure the top row is included
    str     x9, [sp, #72]
sg_collect_groups.block:
    str     x24, [sp, #88]              // group start
    ldr     x21, [sp, #64]              // row
sg_collect_groups.block_row:
    ldr     x9, [sp, #72]
    cmp     x21, x9
    b.ge    sg_collect_groups.block_done
    ldr     x22, [sp, #80]              // column
    LDX     x9, sg_column_indexes
    ldr     x23, [x9, x20, lsl #3]      // column_index
sg_collect_groups.block_column:
    cmp     x22, x23
    b.ge    sg_collect_groups.block_row_next
    lsl     x1, x21, #32
    mov     w9, w22
    orr     x1, x1, x9
    bl      char_at_input_coord
    cmn     w9, #1                      // NONE
    b.eq    sg_collect_groups.no_char
    LDX     x3, sg_members
    str     w9, [x3, x24, lsl #2]
    add     x24, x24, #1
sg_collect_groups.no_char:
    add     x22, x22, #1
    b       sg_collect_groups.block_column
sg_collect_groups.block_row_next:
    add     x21, x21, #1
    b       sg_collect_groups.block_row
sg_collect_groups.block_done:
    ldr     x9, [sp, #88]
    cmp     x24, x9
    b.eq    sg_collect_groups.next_column
    LDX     x3, sg_group_count
    LDX     x2, sg_groups
    add     x2, x2, x3, lsl #3
    str     w9, [x2]
    sub     x4, x24, x9
    str     w4, [x2, #4]
    add     x3, x3, #1
    STX     x3, sg_group_count
sg_collect_groups.next_column:
    LDX     x9, sg_column_indexes
    ldr     x9, [x9, x20, lsl #3]
    str     x9, [sp, #80]
    add     x20, x20, #1
    b       sg_collect_groups.column
sg_collect_groups.row_done:
    ldr     x9, [sp, #72]
    str     x9, [sp, #64]
    add     x19, x19, #1
    b       sg_collect_groups.row
sg_collect_groups.done:
    STX     x24, sg_member_count
    LDX     x0, sg_group_count
    lsl     x0, x0, #3
    bl      alloc
    STX     x9, sg_tracker
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #112
    ret

// sg_build_dissolves: per group, per member, in order: a dissolve scene of
// randint(15, 30) frames (choice of symbol, then choice of spectrum color,
// duration 2), the final frame (input symbol, final colors, duration 1),
// activated, and SCENE_COMPLETE -> update_group_tracker(group_number).
sg_build_dissolves:
    stp     x19, x20, [sp, #-80]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56]: symbol index, [sp, #64]: memo index
    mov     x24, #0                     // group number
sg_build_dissolves.group:
    LDX     x9, sg_group_count
    cmp     x24, x9
    b.hs    sg_build_dissolves.done
    LDX     x9, sg_groups
    add     x9, x9, x24, lsl #3
    ldr     w22, [x9]                   // member index
    ldr     w23, [x9, #4]
    add     x23, x23, x22               // member end
sg_build_dissolves.member:
    cmp     x22, x23
    b.hs    sg_build_dissolves.next_group
    LDX     x9, sg_members
    ldr     w19, [x9, x22, lsl #2]      // slot
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w21, w9                     // dissolve scene
    mov     x0, #15
    mov     x1, #30
    bl      rng_randint
    mov     w20, w9
sg_build_dissolves.frame:
    cbz     w20, sg_build_dissolves.final
    LDX     x9, effect_config
    ldr     x0, [x9, #SYNTHGRID.gen_symbol_count]
    bl      rng_below
    str     x9, [sp, #56]               // symbol index
    LDX     x0, sg_text_spectrum_len
    bl      rng_below
    ldr     x3, [sp, #56]
    LDX     x10, sg_text_spectrum_len
    mul     x3, x3, x10
    add     x3, x3, x9                  // memo index
    LDX     x2, sg_gen_memo
    ldr     w1, [x2, x3, lsl #2]
    cbnz    w1, sg_build_dissolves.have_visual
    str     x3, [sp, #64]
    LDX     x2, sg_text_spectrum
    ldr     x0, [x2, x9, lsl #3]        // fg
    LDX     x2, effect_config
    ldr     x2, [x2, #SYNTHGRID.gen_symbols]
    ldr     x9, [sp, #56]
    ldr     x2, [x2, x9, lsl #3]        // symbol
    mov     x1, #NONE
    mov     w3, #0
    bl      visual_make
    ldr     x3, [sp, #64]
    LDX     x2, sg_gen_memo
    str     w9, [x2, x3, lsl #2]
    mov     w1, w9
sg_build_dissolves.have_visual:
    mov     w0, w21
    mov     w2, #2
    bl      scene_add_frame_visual
    sub     w20, w20, #1
    b       sg_build_dissolves.frame
sg_build_dissolves.final:
    // final colors: character_final_color_map, (None, None) for fill
    mov     x3, #NONE
    mov     x4, #NONE
    LDX     x9, ch_flags
    ldrh    w9, [x9, x19, lsl #1]
    tst     w9, #CF_INPUT
    b.eq    sg_build_dissolves.final_frame
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    sg_build_dissolves.gradient_color
    LDX     x9, ch_fg
    ldr     x3, [x9, x19, lsl #3]
    LDX     x9, ch_bg
    ldr     x4, [x9, x19, lsl #3]
    b       sg_build_dissolves.final_frame
sg_build_dissolves.gradient_color:
    LDX     x9, ch_sym
    ldr     x9, [x9, x19, lsl #3]
    MOV64   x2, ((1 << 32) | 0x20)      // ' '
    cmp     x9, x2
    b.eq    sg_build_dissolves.final_frame
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x19, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, sg_text_map_width
    mul     x9, x9, x10
    LDX     x2, ch_icol
    ldrsw   x2, [x2, x19, lsl #2]
    add     x9, x9, x2
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x2, sg_text_map
    ldr     x3, [x2, x9, lsl #3]
sg_build_dissolves.final_frame:
    LDX     x1, ch_sym
    ldr     x1, [x1, x19, lsl #3]
    mov     w0, w21
    mov     w2, #1
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w19
    mov     w1, w21
    bl      scene_activate
    // SCENE_COMPLETE on this scene's name -> update_group_tracker
    SCENE_PTR x9, x21
    MOV64   x12, SC_NAME
    ldr     w3, [x9, x12]
    mov     x6, x24
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    mov     w4, #ACT_CALLBACK
    ADRG    x5, sg_update_group_tracker
    bl      event_register
    add     x22, x22, #1
    b       sg_build_dissolves.member
sg_build_dissolves.next_group:
    add     x24, x24, #1
    b       sg_build_dissolves.group
sg_build_dissolves.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #80
    ret

// sg_update_group_tracker(w0=slot, x1=group number): the effect callback
// update_group_tracker.
sg_update_group_tracker:
    LDX     x9, sg_tracker
    ldr     x2, [x9, x1, lsl #3]
    sub     x2, x2, #1
    str     x2, [x9, x1, lsl #3]
    ret

// synthgrid_next_frame -> w9 = 1 for a frame, 0 when done.
synthgrid_next_frame:
    PUSH2   x19, x20
    PUSH2   x21, x22
    PUSH2   x23, x30
    LDX     x9, sg_pending_next
    LDX     x10, sg_group_count
    cmp     x9, x10
    b.lo    synthgrid_next_frame.run
    LDB     w9, sg_phase
    cmp     w9, #SG_PH_COMPLETE
    b.ne    synthgrid_next_frame.run
    bl      active_empty
    cbnz    w9, synthgrid_next_frame.finished
synthgrid_next_frame.run:
    LDB     w9, sg_phase
    cmp     w9, #SG_PH_GRID_EXPAND
    b.eq    synthgrid_next_frame.expand
    cmp     w9, #SG_PH_ADD_CHARS
    b.eq    synthgrid_next_frame.add
    cmp     w9, #SG_PH_COLLAPSE
    b.eq    synthgrid_next_frame.collapse
    b       synthgrid_next_frame.update
synthgrid_next_frame.expand:
    // extend every line that is not yet extended; none left -> AddChars
    mov     w21, #0                     // any extended this frame
    mov     x19, #0
synthgrid_next_frame.expand_line:
    LDX     x9, sg_line_count
    cmp     x19, x9
    b.hs    synthgrid_next_frame.expand_done
    LDX     x22, sg_lines
    add     x22, x22, x19, lsl #4
    ldr     w9, [x22, #SG_LN_EXT]
    ldr     w10, [x22, #SG_LN_COUNT]
    cmp     w9, w10
    b.hs    synthgrid_next_frame.expand_next
    mov     w21, #1
    ldr     w20, [x22, #SG_LN_STEP]
synthgrid_next_frame.extend:
    cbz     w20, synthgrid_next_frame.expand_next
    sub     w20, w20, #1
    ldr     w9, [x22, #SG_LN_EXT]
    ldr     w10, [x22, #SG_LN_COUNT]
    cmp     w9, w10
    b.hs    synthgrid_next_frame.extend
    add     w10, w9, #1
    str     w10, [x22, #SG_LN_EXT]
    ldr     w0, [x22, #SG_LN_FIRST]
    add     w0, w0, w9
    bl      set_visible
    b       synthgrid_next_frame.extend
synthgrid_next_frame.expand_next:
    add     x19, x19, #1
    b       synthgrid_next_frame.expand_line
synthgrid_next_frame.expand_done:
    cbnz    w21, synthgrid_next_frame.update
    mov     w9, #SG_PH_ADD_CHARS
    STB     w9, sg_phase
    b       synthgrid_next_frame.update
synthgrid_next_frame.add:
    LDX     x9, sg_pending_next
    LDX     x10, sg_group_count
    cmp     x9, x10
    b.hs    synthgrid_next_frame.add_check
    // active_groups < total_group_count * max_active_blocks
    LDX     x11, sg_active_groups
    scvtf   d0, x11
    scvtf   d1, x10
    LDX     x3, effect_config
    ldr     d2, [x3, #SYNTHGRID.max_active_blocks]
    fmul    d1, d1, d2
    fcmp    d1, d0
    b.le    synthgrid_next_frame.add_check
    LDX     x3, sg_pending
    ldr     x21, [x3, x9, lsl #3]       // group number
    add     x9, x9, #1
    STX     x9, sg_pending_next
    LDX     x3, sg_groups
    add     x3, x3, x21, lsl #3
    ldr     w19, [x3]
    ldr     w22, [x3, #4]
    add     x22, x22, x19
synthgrid_next_frame.add_member:
    cmp     x19, x22
    b.hs    synthgrid_next_frame.add_check
    LDX     x9, sg_members
    ldr     w20, [x9, x19, lsl #2]
    mov     w0, w20
    bl      set_visible
    mov     w0, w20
    bl      active_insert
    LDX     x9, sg_tracker
    ldr     x3, [x9, x21, lsl #3]
    add     x3, x3, #1
    str     x3, [x9, x21, lsl #3]
    add     x19, x19, #1
    b       synthgrid_next_frame.add_member
synthgrid_next_frame.add_check:
    LDX     x9, sg_pending_next
    LDX     x10, sg_group_count
    cmp     x9, x10
    b.lo    synthgrid_next_frame.update
    LDX     x9, sg_active_groups
    cbnz    x9, synthgrid_next_frame.update
    bl      active_empty
    cbz     w9, synthgrid_next_frame.update
    mov     w9, #SG_PH_COLLAPSE
    STB     w9, sg_phase
    b       synthgrid_next_frame.update
synthgrid_next_frame.collapse:
    // collapse every line that is not yet collapsed; none left -> Complete
    mov     w21, #0
    mov     x19, #0
synthgrid_next_frame.collapse_line:
    LDX     x9, sg_line_count
    cmp     x19, x9
    b.hs    synthgrid_next_frame.collapse_done
    LDX     x22, sg_lines
    add     x22, x22, x19, lsl #4
    ldr     w9, [x22, #SG_LN_EXT]
    cbz     w9, synthgrid_next_frame.collapse_next
    mov     w21, #1
    ldr     w20, [x22, #SG_LN_STEP]
synthgrid_next_frame.shrink:
    cbz     w20, synthgrid_next_frame.collapse_next
    sub     w20, w20, #1
    ldr     w9, [x22, #SG_LN_EXT]
    cbz     w9, synthgrid_next_frame.shrink
    sub     w9, w9, #1
    str     w9, [x22, #SG_LN_EXT]
    ldr     w0, [x22, #SG_LN_FIRST]
    add     w0, w0, w9
    mov     w1, #0
    bl      set_visibility
    b       synthgrid_next_frame.shrink
synthgrid_next_frame.collapse_next:
    add     x19, x19, #1
    b       synthgrid_next_frame.collapse_line
synthgrid_next_frame.collapse_done:
    cbnz    w21, synthgrid_next_frame.update
    mov     w9, #SG_PH_COMPLETE
    STB     w9, sg_phase
synthgrid_next_frame.update:
    bl      update
    // active_groups = the groups whose tracker is nonzero
    mov     x9, #0
    mov     x3, #0
    LDX     x2, sg_tracker
    LDX     x4, sg_group_count
synthgrid_next_frame.count:
    cmp     x3, x4
    b.hs    synthgrid_next_frame.counted
    ldr     x5, [x2, x3, lsl #3]
    cmp     x5, #0
    cinc    x9, x9, ne
    add     x3, x3, #1
    b       synthgrid_next_frame.count
synthgrid_next_frame.counted:
    STX     x9, sg_active_groups
    mov     w9, #1
    b       synthgrid_next_frame.ret
synthgrid_next_frame.finished:
    mov     w9, #0
synthgrid_next_frame.ret:
    POP2    x23, x30
    POP2    x21, x22
    POP2    x19, x20
    ret

    .section .rodata
STRING msg_sg_max, "max_row and max_column must be greater than 0."
STRING msg_sg_min, "min_row and min_column must be less than or equal to max_row and max_column."

    TSTATE
    .balign 8
sg_grid_map:           .skip 8
sg_text_spectrum:      .skip 8
sg_text_spectrum_len:  .skip 8
sg_text_map:           .skip 8
sg_text_map_width:     .skip 8
sg_gen_memo:           .skip 8
sg_lines:              .skip 8
sg_line_count:         .skip 8
sg_row_indexes:        .skip 8
sg_row_index_count:    .skip 8
sg_column_indexes:     .skip 8
sg_column_index_count: .skip 8
sg_members:            .skip 8
sg_member_count:       .skip 8
sg_groups:             .skip 8
sg_group_count:        .skip 8
sg_tracker:            .skip 8
sg_pending:            .skip 8
sg_pending_next:       .skip 8
sg_active_groups:      .skip 8
sg_phase:              .skip 1

    .text
