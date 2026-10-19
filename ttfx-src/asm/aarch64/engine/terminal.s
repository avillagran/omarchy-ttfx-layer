// engine/terminal.s - canvas layout, text anchoring, fill characters,
// neighbors and character queries of Terminal (src/engine/terminal.rs,
// canvas.rs).

    .text

// terminal_init: preprocess, lay out the canvas and anchor the text.
terminal_init:
    PUSH2   x19, x20
    PUSH2   x21, x22
    PUSH2   x23, x24
    PUSH1   x30
    bl      input_init
    bl      compute_layout
    bl      assign_coordinates
    bl      anchor_text
    bl      build_coord_map
    bl      make_fill_characters
    bl      setup_neighbors
    POP1    x30
    POP2    x23, x24
    POP2    x21, x22
    POP2    x19, x20
    ret

// floor_div2(x9) -> x9 = x9 // 2 (Python floor division).
floor_div2:
    asr     x9, x9, #1
    ret

// compute_layout: canvas dimensions, canvas offsets and the visible window.
compute_layout:
    PUSH2   x19, x30
    // input width = longest trimmed line; input height = line count
    LDX     x3, line_count
    LDX     x1, line_len
    mov     x9, #0
    mov     x2, #0
compute_layout.widest:
    cmp     x2, x3
    b.hs    compute_layout.width
    ldr     x8, [x1, x2, lsl #3]
    cmp     x9, x8
    csel    x9, x8, x9, lt
    add     x2, x2, #1
    b       compute_layout.widest
compute_layout.width:
    LDX     x19, cfg_canvas_width
    cmp     x19, #0
    b.gt    compute_layout.width_done
    b.eq    compute_layout.width_term
    mov     x19, x9
    LDB     w8, cfg_ignore_dims
    cbnz    w8, compute_layout.width_done
    LDX     x8, term_width
    cmp     x19, x8
    csel    x19, x8, x19, gt
    b       compute_layout.width_done
compute_layout.width_term:
    LDX     x19, term_width
compute_layout.width_done:
    STX     x19, canvas_right
    LDX     x9, cfg_canvas_height
    cmp     x9, #0
    b.gt    compute_layout.height_done
    b.eq    compute_layout.height_term
    LDX     x9, line_count
    LDB     w8, cfg_ignore_dims
    cbnz    w8, compute_layout.height_done
    LDB     w8, cfg_wrap_text
    cbz     w8, compute_layout.clip_height
    mov     x0, x19                     // wrapped at the canvas width
    bl      wrapped_line_count
compute_layout.clip_height:
    LDX     x8, term_height
    cmp     x9, x8
    csel    x9, x8, x9, gt
    b       compute_layout.height_done
compute_layout.height_term:
    LDX     x9, term_height
compute_layout.height_done:
    STX     x9, canvas_top
    // canvas center (Canvas::new)
    mov     x0, x9
    bl      center_of
    STX     x9, center_row
    LDX     x0, canvas_right
    bl      center_of
    STX     x9, center_col
    // offsets
    mov     x4, #0                      // column offset
    mov     x5, #0                      // row offset
    LDX     x10, term_width
    LDX     x11, term_height
    LDB     w8, cfg_ignore_dims
    cbz     w8, compute_layout.offsets
    LDX     x10, canvas_right
    LDX     x11, canvas_top
    b       compute_layout.visible
compute_layout.offsets:
    LDX     x3, cfg_anchor_canvas
    ADRG    x2, anchor_column_group
    ldrb    w2, [x2, x3]
    cmp     w2, #1
    b.ne    compute_layout.col_east
    asr     x9, x10, #1
    LDX     x4, canvas_right
    asr     x4, x4, #1
    sub     x4, x9, x4
    b       compute_layout.rows
compute_layout.col_east:
    cmp     w2, #2
    b.ne    compute_layout.rows
    LDX     x8, canvas_right
    sub     x4, x10, x8
compute_layout.rows:
    ADRG    x2, anchor_row_group
    ldrb    w2, [x2, x3]
    cmp     w2, #1
    b.ne    compute_layout.row_north
    asr     x9, x11, #1
    LDX     x5, canvas_top
    asr     x5, x5, #1
    sub     x5, x9, x5
    b       compute_layout.visible
compute_layout.row_north:
    cmp     w2, #2
    b.ne    compute_layout.visible
    LDX     x8, canvas_top
    sub     x5, x11, x8
compute_layout.visible:
    STX     x4, col_offset
    STX     x5, row_offset
    LDX     x9, canvas_top
    add     x9, x9, x5
    cmp     x9, x11
    csel    x9, x11, x9, gt
    STX     x9, visible_top
    add     x9, x5, #1
    mov     x3, #1
    cmp     x9, x3
    csel    x9, x3, x9, lt
    STX     x9, visible_bottom
    LDX     x9, canvas_right
    add     x9, x9, x4
    cmp     x9, x10
    csel    x9, x10, x9, gt
    STX     x9, visible_right
    add     x9, x4, #1
    cmp     x9, x3
    csel    x9, x3, x9, lt
    STX     x9, visible_left
    POP2    x19, x30
    ret

// center_of(x0=extent) -> x9: max(extent // 2, 1), +1 when odd and > 1.
// Clobbers x3.
center_of:
    asr     x9, x0, #1
    mov     x3, #1
    cmp     x9, x3
    csel    x9, x3, x9, lt
    tbz     x0, #0, center_of.done
    cmp     x0, #1
    b.le    center_of.done
    add     x9, x9, #1
center_of.done:
    ret

// anchor_text: Canvas::anchor_text over the input characters (in place).
anchor_text:
    PUSH2   x19, x21
    PUSH2   x22, x23
    PUSH2   x24, x30
    LDX     x21, input_chars
    LDX     x22, input_count
    cbnz    x22, anchor_text.have_input
    b       error_no_input_chars
anchor_text.have_input:
    LDX     x4, ch_col
    LDX     x5, ch_row
    // input extents: max column, max row
    mov     x3, #0
    mov     w10, #0x80000000
    mov     w11, #0x80000000
anchor_text.extent:
    cmp     x3, x22
    b.hs    anchor_text.extent_done
    ldr     w9, [x21, x3, lsl #2]
    ldr     w2, [x4, x9, lsl #2]
    cmp     w2, w10
    csel    w10, w2, w10, gt
    ldr     w2, [x5, x9, lsl #2]
    cmp     w2, w11
    csel    w11, w2, w11, gt
    add     x3, x3, #1
    b       anchor_text.extent
anchor_text.extent_done:
    sxtw    x10, w10                    // input_width
    sxtw    x11, w11                    // input_height
    LDX     x3, cfg_anchor_text
    mov     x1, #0                      // column delta
    mov     x0, #0                      // row delta
    LDX     x8, canvas_right
    cmp     x10, x8
    b.eq    anchor_text.row_delta
    ADRG    x2, anchor_column_group
    ldrb    w2, [x2, x3]
    cmp     w2, #1
    b.ne    anchor_text.col_east
    asr     x9, x10, #1
    LDX     x1, center_col
    sub     x1, x1, x9
    b       anchor_text.row_delta
anchor_text.col_east:
    cmp     w2, #2
    b.ne    anchor_text.row_delta
    LDX     x1, canvas_right
    sub     x1, x1, x10
anchor_text.row_delta:
    LDX     x8, canvas_top
    cmp     x11, x8
    b.eq    anchor_text.apply
    ADRG    x2, anchor_row_group
    ldrb    w2, [x2, x3]
    cmp     w2, #1
    b.ne    anchor_text.row_north
    asr     x9, x11, #1
    LDX     x0, center_row
    sub     x0, x0, x9
    b       anchor_text.apply
anchor_text.row_north:
    cmp     w2, #2
    b.ne    anchor_text.apply
    LDX     x0, canvas_top
    sub     x0, x0, x11
anchor_text.apply:
    // shift, then keep the in-canvas characters (order preserved)
    mov     x3, #0
    mov     x19, #0                     // kept count
    mov     w10, #0x7fffffff            // text_left
    mov     w11, #0x80000000            // text_right
    mov     w8, #0x80000000
    sxtw    x23, w8                     // text_top = -2147483648
    mov     x24, #0x7fffffff            // text_bottom = 2147483647
    LDX     x6, ch_icol
    LDX     x7, ch_irow
    LDX     x14, ch_flags
    LDX     x12, canvas_right
    LDX     x13, canvas_top
anchor_text.shift:
    cmp     x3, x22
    b.hs    anchor_text.shifted
    ldr     w9, [x21, x3, lsl #2]
    ldr     w2, [x4, x9, lsl #2]
    add     w2, w2, w1
    str     w2, [x4, x9, lsl #2]
    str     w2, [x6, x9, lsl #2]
    ldr     w15, [x5, x9, lsl #2]
    add     w15, w15, w0
    str     w15, [x5, x9, lsl #2]
    str     w15, [x7, x9, lsl #2]
    // in canvas: 1 <= column <= right, 1 <= row <= top
    cmp     w2, #1
    b.lt    anchor_text.drop
    sxtw    x2, w2
    cmp     x2, x12
    b.gt    anchor_text.drop
    cmp     w15, #1
    b.lt    anchor_text.drop
    sxtw    x15, w15
    cmp     x15, x13
    b.gt    anchor_text.drop
    str     w9, [x21, x19, lsl #2]
    add     x19, x19, #1
    ldrh    w8, [x14, x9, lsl #1]
    orr     w8, w8, #CF_INPUT
    strh    w8, [x14, x9, lsl #1]
    cmp     w2, w10
    csel    w10, w2, w10, lt
    cmp     w2, w11
    csel    w11, w2, w11, gt
    cmp     x15, x23
    b.le    anchor_text.not_top
    mov     x23, x15
anchor_text.not_top:
    cmp     x15, x24
    b.ge    anchor_text.drop
    mov     x24, x15
anchor_text.drop:
    add     x3, x3, #1
    b       anchor_text.shift
anchor_text.shifted:
    STX     x23, text_top
    STX     x24, text_bottom
    cbz     x19, anchor_text.all_outside
    STX     x19, input_count
    sxtw    x10, w10
    sxtw    x11, w11
    STX     x10, text_left
    STX     x11, text_right
    POP2    x24, x30
    POP2    x22, x23
    POP2    x19, x21
    ret
anchor_text.all_outside:
    FAIL    msg_all_outside

// build_coord_map: character_by_input_coord as a canvas-sized grid of slots
// (NONE where empty), seeded with the kept input characters; plus the text
// center.
build_coord_map:
    PUSH2   x19, x30
    LDX     x9, canvas_top
    LDX     x8, canvas_right
    mul     x9, x9, x8
    STX     x9, coord_map_cells
    lsl     x0, x9, #2
    add     x0, x0, #64
    bl      reserve
    STX     x9, coord_map
    // fill with NONE
    mov     x0, x9
    LDX     x3, coord_map_cells
    mov     w9, #NONE
    REP_STOSD
    mov     x19, #0
build_coord_map.each:
    LDX     x8, input_count
    cmp     x19, x8
    b.hs    build_coord_map.center
    LDX     x9, input_chars
    ldr     w0, [x9, x19, lsl #2]
    bl      char_input_coord
    mov     x1, x9
    bl      coord_map_index
    LDX     x8, input_chars
    ldr     w0, [x8, x19, lsl #2]
    LDX     x3, coord_map
    str     w0, [x3, x9, lsl #2]
    add     x19, x19, #1
    b       build_coord_map.each
build_coord_map.center:
    LDX     x9, text_top
    LDX     x8, text_bottom
    sub     x9, x9, x8
    asr     x9, x9, #1
    add     x9, x9, x8
    STX     x9, text_center_row
    LDX     x9, text_right
    LDX     x8, text_left
    sub     x9, x9, x8
    asr     x9, x9, #1
    add     x9, x9, x8
    STX     x9, text_center_col
    POP2    x19, x30
    ret

// coord_map_index(x1=coord inside the canvas) -> x9 = grid index.
// Clobbers x3.
coord_map_index:
    asr     x9, x1, #32
    sub     x9, x9, #1
    LDX     x3, canvas_right
    mul     x9, x9, x3
    sxtw    x3, w1
    add     x9, x9, x3
    sub     x9, x9, #1
    ret

// char_at_input_coord(x1=coord) -> w9 = slot or NONE
// (Terminal.get_character_by_input_coord). Clobbers x3, x16.
char_at_input_coord:
    asr     x9, x1, #32
    cmp     x9, #1
    b.lt    char_at_input_coord.none
    LDX     x3, canvas_top
    cmp     x9, x3
    b.gt    char_at_input_coord.none
    sxtw    x3, w1
    cmp     x3, #1
    b.lt    char_at_input_coord.none
    LDX     x16, canvas_right
    cmp     x3, x16
    b.gt    char_at_input_coord.none
    // coord_map_index, inline
    sub     x9, x9, #1
    mul     x9, x9, x16
    add     x9, x9, x3
    sub     x9, x9, #1
    LDX     x3, coord_map
    ldr     w9, [x3, x9, lsl #2]
    ret
char_at_input_coord.none:
    mov     w9, #NONE
    ret

// make_fill_characters: Terminal._make_fill_characters - row-major from
// (1, 1), a space for every unoccupied canvas cell, split inner/outer by the
// text box.
make_fill_characters:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x0, coord_map_cells
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      reserve
    STX     x9, inner_fill_chars
    LDX     x0, coord_map_cells
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      reserve
    STX     x9, outer_fill_chars
    LDW     w0, char_count
    LDX     x1, coord_map_cells
    LDX     x8, input_count
    sub     x1, x1, x8                  // the fill characters to come
    bl      populate_chars
    mov     x21, #1                     // row
make_fill_characters.row:
    LDX     x8, canvas_top
    cmp     x21, x8
    b.gt    make_fill_characters.done
    mov     x22, #1                     // column
make_fill_characters.column:
    LDX     x8, canvas_right
    cmp     x22, x8
    b.gt    make_fill_characters.next_row
    orr     x1, x22, x21, lsl #32
    bl      coord_map_index
    mov     x19, x9
    LDX     x3, coord_map
    ldr     w8, [x3, x19, lsl #2]
    cmn     w8, #1                      // NONE
    b.ne    make_fill_characters.next
    MOV64   x0, 0x100000020             // (1 << 32) | ' '
    mov     w1, w22
    mov     w2, w21
    bl      new_char
    mov     w9, w9
    LDX     x3, coord_map
    str     w9, [x3, x19, lsl #2]
    // inner when inside the text box
    mov     w2, #CF_FILL_OUTER
    LDX     x8, text_left
    cmp     x22, x8
    b.lt    make_fill_characters.flag
    LDX     x8, text_right
    cmp     x22, x8
    b.gt    make_fill_characters.flag
    LDX     x8, text_bottom
    cmp     x21, x8
    b.lt    make_fill_characters.flag
    LDX     x8, text_top
    cmp     x21, x8
    b.gt    make_fill_characters.flag
    mov     w2, #CF_FILL_INNER
make_fill_characters.flag:
    LDX     x3, ch_flags
    ldrh    w8, [x3, x9, lsl #1]
    orr     w8, w8, w2
    strh    w8, [x3, x9, lsl #1]
    cmp     w2, #CF_FILL_INNER
    b.ne    make_fill_characters.outer
    LDW     w3, inner_fill_count
    LDX     x2, inner_fill_chars
    str     w9, [x2, x3, lsl #2]
    add     w3, w3, #1
    STW     w3, inner_fill_count
    b       make_fill_characters.next
make_fill_characters.outer:
    LDW     w3, outer_fill_count
    LDX     x2, outer_fill_chars
    str     w9, [x2, x3, lsl #2]
    add     w3, w3, #1
    STW     w3, outer_fill_count
make_fill_characters.next:
    add     x22, x22, #1
    b       make_fill_characters.column
make_fill_characters.next_row:
    add     x21, x21, #1
    b       make_fill_characters.row
make_fill_characters.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// setup_neighbors: north/east/south/west of every mapped character, straight
// from the grid (every cell holds a character once the fill is made).
setup_neighbors:
    LDX     x4, coord_map
    LDX     x5, canvas_right            // row stride
    LDX     x10, ch_nbr
    LDX     x11, canvas_top
    lsl     x12, x5, #2                 // stride in bytes
    mov     x2, #1                      // row
setup_neighbors.row:
    cmp     x2, x11
    b.gt    setup_neighbors.done
    mov     x1, #1                      // column
setup_neighbors.column:
    cmp     x1, x5
    b.gt    setup_neighbors.next_row
    ldr     w3, [x4]                    // this character
    add     x3, x10, x3, lsl #4
    mov     w9, #NONE                   // north: row + 1
    cmp     x2, x11
    b.ge    setup_neighbors.north
    ldr     w9, [x4, x12]
setup_neighbors.north:
    str     w9, [x3, #NBR_NORTH]
    mov     w9, #NONE                   // east: column + 1
    cmp     x1, x5
    b.ge    setup_neighbors.east
    ldr     w9, [x4, #4]
setup_neighbors.east:
    str     w9, [x3, #NBR_EAST]
    mov     w9, #NONE                   // south: row - 1
    cmp     x2, #1
    b.le    setup_neighbors.south
    sub     x8, x4, x12
    ldr     w9, [x8]
setup_neighbors.south:
    str     w9, [x3, #NBR_SOUTH]
    mov     w9, #NONE                   // west: column - 1
    cmp     x1, #1
    b.le    setup_neighbors.west
    ldur    w9, [x4, #-4]
setup_neighbors.west:
    str     w9, [x3, #NBR_WEST]
    add     x4, x4, #4
    add     x1, x1, #1
    b       setup_neighbors.column
setup_neighbors.next_row:
    add     x2, x2, #1
    b       setup_neighbors.row
setup_neighbors.done:
    ret

// ---------------------------------------------------------------- queries
// FILTER_*, SORT_* (CharacterSort order) and GROUP_* (CharacterGroup order)
// are in defs.inc.

// collect_characters(w0=FILTER_* bits) -> x9 = u32 slot array, x2 = count.
// Input characters, inner fill, outer fill, added - in that order.
collect_characters:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w19, w0
    LDX     x0, input_count
    LDW     w9, inner_fill_count
    add     x0, x0, x9
    LDW     w9, outer_fill_count
    add     x0, x0, x9
    LDW     w9, added_count
    add     x0, x0, x9
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      alloc
    mov     x21, x9
    mov     x22, #0
    tst     w19, #FILTER_INPUT
    b.eq    collect_characters.inner
    LDX     x1, input_chars
    LDX     x3, input_count
    bl      collect_characters.append
collect_characters.inner:
    tst     w19, #FILTER_INNER_FILL
    b.eq    collect_characters.outer
    LDX     x1, inner_fill_chars
    LDW     w3, inner_fill_count
    bl      collect_characters.append
collect_characters.outer:
    tst     w19, #FILTER_OUTER_FILL
    b.eq    collect_characters.added
    LDX     x1, outer_fill_chars
    LDW     w3, outer_fill_count
    bl      collect_characters.append
collect_characters.added:
    tst     w19, #FILTER_ADDED
    b.eq    collect_characters.done
    LDX     x1, added_chars
    LDW     w3, added_count
    bl      collect_characters.append
collect_characters.done:
    mov     x9, x21
    mov     x2, x22
    POP2    x22, x30
    POP2    x19, x21
    ret
collect_characters.append:
    add     x0, x21, x22, lsl #2
    add     x22, x22, x3
    REP_MOVSD
    ret

// key_row_desc_col(w0=slot) -> x9: (-row, column) as an unsigned-ordered key
// over input coordinates. key_row_col: (row, column). Clobber x3.
key_row_desc_col:
    LDX     x9, ch_irow
    ldr     w9, [x9, w0, uxtw #2]
    neg     w9, w9
    b       key_with_column
key_row_col:
    LDX     x9, ch_irow
    ldr     w9, [x9, w0, uxtw #2]
key_with_column:
    eor     w9, w9, #0x80000000         // + 0x80000000
    lsl     x9, x9, #32
    LDX     x3, ch_icol
    ldr     w3, [x3, w0, uxtw #2]
    eor     w3, w3, #0x80000000
    orr     x9, x9, x3
    ret

// sort_slots_by(x0=u32 slots, x1=count, x2=key function) - stable sort by
// the 64-bit unsigned key the function computes for each slot. The key
// function takes w0 = slot, returns x9 and may clobber x0-x5, x8-x11.
sort_slots_by:
sort_slots_stable:
    PUSH2   x19, x21
    PUSH2   x22, x23
    PUSH2   x24, x30
    mov     x21, x0
    mov     x22, x1
    mov     x23, x2
    cmp     x22, #1
    b.ls    sort_slots_by.done
    // pairs of (key, slot)
    lsl     x0, x22, #4
    bl      alloc
    mov     x24, x9
    mov     x19, #0
sort_slots_by.keys:
    cmp     x19, x22
    b.hs    sort_slots_by.sort
    ldr     w0, [x21, x19, lsl #2]
    blr     x23
    add     x3, x24, x19, lsl #4
    str     x9, [x3]
    ldr     w0, [x21, x19, lsl #2]
    str     x0, [x3, #8]
    add     x19, x19, #1
    b       sort_slots_by.keys
sort_slots_by.sort:
    mov     x0, x24
    mov     x1, x22
    bl      sort_pairs
    mov     x19, #0
sort_slots_by.back:
    cmp     x19, x22
    b.hs    sort_slots_by.done
    add     x3, x24, x19, lsl #4
    ldr     w9, [x3, #8]
    str     w9, [x21, x19, lsl #2]
    add     x19, x19, #1
    b       sort_slots_by.back
sort_slots_by.done:
    POP2    x24, x30
    POP2    x22, x23
    POP2    x19, x21
    ret

// sort_pairs(x0=pairs of (u64 key, u64 value), x1=count): stable merge sort
// by key (unsigned), bottom-up with one scratch buffer.
sort_pairs:
    PUSH2   x19, x20
    PUSH2   x21, x22
    PUSH2   x23, x24
    PUSH1   x30
    mov     x21, x0                     // source
    STX     x0, sort_pairs_origin
    mov     x22, x1                     // count
    lsl     x0, x1, #4
    bl      alloc
    mov     x23, x9                     // destination
    mov     x20, #1                     // run width
sort_pairs.pass:
    cmp     x20, x22
    b.hs    sort_pairs.finished
    mov     x19, #0                     // left start
sort_pairs.merge:
    cmp     x19, x22
    b.hs    sort_pairs.swap
    add     x4, x19, x20                // middle
    cmp     x4, x22
    csel    x4, x22, x4, hi
    add     x5, x4, x20                 // end
    cmp     x5, x22
    csel    x5, x22, x5, hi
    mov     x10, x19                    // i (left)
    mov     x11, x4                     // j (right)
    mov     x24, x19                    // k (out)
sort_pairs.pick:
    cmp     x24, x5
    b.hs    sort_pairs.merged
    cmp     x10, x4
    b.hs    sort_pairs.take_right
    cmp     x11, x5
    b.hs    sort_pairs.take_left
    lsl     x9, x10, #4
    lsl     x3, x11, #4
    ldr     x2, [x21, x3]
    ldr     x8, [x21, x9]
    cmp     x2, x8
    b.lo    sort_pairs.take_right       // strictly smaller right wins; ties keep left
sort_pairs.take_left:
    mov     x9, x10
    add     x10, x10, #1
    b       sort_pairs.put
sort_pairs.take_right:
    mov     x9, x11
    add     x11, x11, #1
sort_pairs.put:
    add     x9, x21, x9, lsl #4
    add     x3, x23, x24, lsl #4
    ldp     x2, x8, [x9]
    stp     x2, x8, [x3]
    add     x24, x24, #1
    b       sort_pairs.pick
sort_pairs.merged:
    mov     x19, x5
    b       sort_pairs.merge
sort_pairs.swap:
    mov     x8, x21
    mov     x21, x23
    mov     x23, x8
    add     x20, x20, x20
    b       sort_pairs.pass
sort_pairs.finished:
    // the sorted data is in x21; copy it back if that is the scratch buffer
    mov     x1, x21
    LDX     x0, sort_pairs_origin
    cmp     x1, x0
    b.eq    sort_pairs.done
    lsl     x3, x22, #4
    REP_MOVSB
sort_pairs.done:
    POP1    x30
    POP2    x23, x24
    POP2    x21, x22
    POP2    x19, x20
    ret

// get_characters(w0=FILTER_* bits, w1=SORT_*) -> x9 = slots, x2 = count.
// Terminal.get_characters; the random sort shuffles with the engine RNG.
get_characters:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w19, w1
    bl      collect_characters
    mov     x21, x9
    mov     x22, x2
    mov     x0, x21
    mov     x1, x22
    ADRG    x2, key_row_desc_col
    bl      sort_slots_stable
    cmp     w19, #SORT_RANDOM
    b.eq    get_characters.random
    cmp     w19, #SORT_BOTTOM_TO_TOP_R2L
    b.eq    get_characters.reverse
    cmp     w19, #SORT_BOTTOM_TO_TOP_L2R
    b.eq    get_characters.row_col
    cmp     w19, #SORT_TOP_TO_BOTTOM_R2L
    b.eq    get_characters.row_col_reversed
    cmp     w19, #SORT_OUTSIDE_ROW_TO_MIDDLE
    b.eq    get_characters.interleave
    cmp     w19, #SORT_MIDDLE_ROW_TO_OUTSIDE
    b.eq    get_characters.interleave_reversed
    b       get_characters.done
get_characters.random:
    mov     x0, x21
    mov     x1, x22
    bl      rng_shuffle32
    b       get_characters.done
get_characters.row_col:
    mov     x0, x21
    mov     x1, x22
    ADRG    x2, key_row_col
    bl      sort_slots_stable
    b       get_characters.done
get_characters.row_col_reversed:
    mov     x0, x21
    mov     x1, x22
    ADRG    x2, key_row_col
    bl      sort_slots_stable
get_characters.reverse:
    mov     x0, x21
    mov     x1, x22
    bl      reverse_u32
    b       get_characters.done
get_characters.interleave:
    bl      get_characters.alternate
    b       get_characters.done
get_characters.interleave_reversed:
    bl      get_characters.alternate
    b       get_characters.reverse
get_characters.done:
    mov     x9, x21
    mov     x2, x22
    POP2    x22, x30
    POP2    x19, x21
    ret
get_characters.alternate:
    // alternately pop the front and the back (outside rows first)
    PUSH1   x30
    lsl     x0, x22, #2
    add     x0, x0, #64
    bl      alloc
    POP1    x30
    mov     x3, #0                      // front
    sub     x2, x22, #1                 // back
    mov     x4, #0                      // out index
get_characters.alt_next:
    cmp     x4, x22
    b.hs    get_characters.alt_done
    ldr     w5, [x21, x3, lsl #2]
    add     x3, x3, #1
    str     w5, [x9, x4, lsl #2]
    add     x4, x4, #1
    cmp     x4, x22
    b.hs    get_characters.alt_done
    ldr     w5, [x21, x2, lsl #2]
    sub     x2, x2, #1
    str     w5, [x9, x4, lsl #2]
    add     x4, x4, #1
    b       get_characters.alt_next
get_characters.alt_done:
    mov     x21, x9
    ret

// reverse_u32(x0=array, x1=count). Clobbers x3, x9.
reverse_u32:
    add     x1, x0, x1, lsl #2
    sub     x1, x1, #4
reverse_u32.loop:
    cmp     x0, x1
    b.hs    reverse_u32.done
    ldr     w9, [x0]
    ldr     w3, [x1]
    str     w3, [x0]
    str     w9, [x1]
    add     x0, x0, #4
    sub     x1, x1, #4
    b       reverse_u32.loop
reverse_u32.done:
    ret

// get_characters_grouped(w0=FILTER_* bits, w1=GROUP_*) -> x9 = groups,
// x2 = group count. Each group is 16 bytes: (u32 slot array, count).
// Terminal.get_characters_grouped: characters in (row, column) order, then
// bucketed by the grouping key (keys outside the canvas range dropped, empty
// buckets skipped), buckets in ascending key order, reversed for the
// opposite direction.
get_characters_grouped:
    PUSH2   x19, x20
    PUSH2   x21, x22
    PUSH2   x23, x24
    PUSH1   x30
    mov     w19, w1
    bl      collect_characters
    mov     x21, x9
    mov     x22, x2
    mov     x0, x21
    mov     x1, x22
    ADRG    x2, key_row_col
    bl      sort_slots_stable
    // key function and inclusive range per grouping
    lsr     w9, w19, #1                 // 0 column, 1 row, 2 diag, 3 anti, 4 center
    STW     w9, group_kind
    // (key, slot) pairs for the kept characters, stable-sorted by key
    lsl     x0, x22, #4
    add     x0, x0, #64
    bl      alloc
    mov     x23, x9
    mov     x24, #0                     // kept
    mov     x20, #0
get_characters_grouped.key:
    cmp     x20, x22
    b.hs    get_characters_grouped.keyed
    ldr     w0, [x21, x20, lsl #2]
    bl      group_key                   // x9 = key (i64), Z set when out of range
    b.eq    get_characters_grouped.skip
    add     x3, x23, x24, lsl #4
    eor     x9, x9, #0x8000000000000000 // unsigned order
    str     x9, [x3]
    ldr     w0, [x21, x20, lsl #2]
    str     x0, [x3, #8]
    add     x24, x24, #1
get_characters_grouped.skip:
    add     x20, x20, #1
    b       get_characters_grouped.key
get_characters_grouped.keyed:
    mov     x0, x23
    mov     x1, x24
    bl      sort_pairs
    // runs of equal keys become groups; members go to one slot array
    lsl     x0, x24, #2
    add     x0, x0, #64
    bl      alloc
    mov     x21, x9                     // members
    lsl     x0, x24, #4
    add     x0, x0, #64
    bl      alloc
    mov     x22, x9                     // groups
    mov     x20, #0                     // group count
    mov     x3, #0
get_characters_grouped.member:
    cmp     x3, x24
    b.hs    get_characters_grouped.grouped
    add     x2, x23, x3, lsl #4
    ldr     w9, [x2, #8]
    str     w9, [x21, x3, lsl #2]
    // a new group when the key differs from the previous one
    cbz     x3, get_characters_grouped.new_group
    ldr     x9, [x2]
    ldur    x8, [x2, #-16]
    cmp     x9, x8
    b.eq    get_characters_grouped.same_group
get_characters_grouped.new_group:
    add     x9, x22, x20, lsl #4
    add     x2, x21, x3, lsl #2
    str     x2, [x9]
    str     xzr, [x9, #8]
    add     x20, x20, #1
get_characters_grouped.same_group:
    sub     x9, x20, #1
    add     x9, x22, x9, lsl #4
    ldr     x8, [x9, #8]
    add     x8, x8, #1
    str     x8, [x9, #8]
    add     x3, x3, #1
    b       get_characters_grouped.member
get_characters_grouped.grouped:
    // column right-to-left, row top-to-bottom, and the diagonal/center
    // variants listed second run their buckets in reverse
    mov     w9, #((1 << GROUP_COLUMN_R2L) | (1 << GROUP_ROW_TOP_TO_BOTTOM) | (1 << GROUP_DIAG_TR_TO_BL) | (1 << GROUP_DIAG_BR_TO_TL) | (1 << GROUP_OUTSIDE_TO_CENTER))
    lsr     w9, w9, w19
    tbz     w9, #0, get_characters_grouped.result
get_characters_grouped.reverse:
    // reverse the order of the 16-byte group records
    mov     x0, x22
    add     x1, x22, x20, lsl #4
    sub     x1, x1, #16
get_characters_grouped.rev:
    cmp     x0, x1
    b.hs    get_characters_grouped.result
    ldp     x2, x3, [x0]
    ldp     x4, x5, [x1]
    stp     x4, x5, [x0]
    stp     x2, x3, [x1]
    add     x0, x0, #16
    sub     x1, x1, #16
    b       get_characters_grouped.rev
get_characters_grouped.result:
    mov     x9, x22
    mov     x2, x20
    POP1    x30
    POP2    x23, x24
    POP2    x21, x22
    POP2    x19, x20
    ret

// group_key(w0=slot) -> x9 = the grouping key of [group_kind]; Z set
// when it falls outside ordered_buckets' range (the character is dropped).
// Clobbers x2-x5, x8.
group_key:
    LDX     x9, ch_irow
    ldrsw   x4, [x9, w0, uxtw #2]       // row
    LDX     x9, ch_icol
    ldrsw   x5, [x9, w0, uxtw #2]       // column
    LDW     w9, group_kind
    cmp     w9, #1
    b.eq    group_key.row
    cmp     w9, #2
    b.eq    group_key.diagonal
    cmp     w9, #3
    b.eq    group_key.anti
    cmp     w9, #4
    b.eq    group_key.center
    // column in [0, right]
    mov     x9, x5
    mov     x3, #0
    LDX     x2, canvas_right
    b       group_key.range
group_key.row:
    mov     x9, x4
    mov     x3, #0
    LDX     x2, canvas_top
    b       group_key.range
group_key.diagonal:
    add     x9, x4, x5
    mov     x3, #0
    LDX     x2, canvas_top
    LDX     x8, canvas_right
    add     x2, x2, x8
    b       group_key.range
group_key.anti:
    sub     x9, x5, x4                  // column - row
    LDX     x8, canvas_top
    mov     x3, #1
    sub     x3, x3, x8                  // left - top
    LDX     x2, canvas_right
    sub     x2, x2, #1                  // right - bottom
    b       group_key.range
group_key.center:
    // Manhattan distance from the text center; every character is kept
    LDX     x8, text_center_col
    sub     x9, x5, x8
    cmp     x9, #0
    cneg    x9, x9, lt
    LDX     x8, text_center_row
    sub     x2, x4, x8
    cmp     x2, #0
    cneg    x2, x2, lt
    add     x9, x9, x2
    mov     w3, #1
    tst     w3, w3                      // Z clear
    ret
group_key.range:
    cmp     x9, x3
    b.lt    group_key.out
    cmp     x9, x2
    b.gt    group_key.out
    mov     w3, #1
    tst     w3, w3                      // Z clear
    ret
group_key.out:
    mov     w3, #0
    tst     w3, w3                      // Z set
    ret

// ---------------------------------------------------------------- canvas

// canvas_random_column(w0=within text) -> x9  (Canvas.random_column)
canvas_random_column:
    cbz     w0, canvas_random_column.canvas
    LDX     x0, text_left
    LDX     x1, text_right
    b       rng_randint
canvas_random_column.canvas:
    mov     x0, #1
    LDX     x1, canvas_right
    b       rng_randint

// rng_below_fill(x0=n > 0, x1=u32 out, x2=count): count rng_below(n)
// draws in a row, stored in order - the same draws, without a call and the
// batch position's memory round trip per draw. Clobbers x1-x4, x9-x11,
// x16, x17.
rng_below_fill:
    cbz     x2, rng_below_fill.none
    // the shift: 64 - max(bit_length(n - 1), 1) = min(clz(n - 1), 63)
    sub     x9, x0, #1
    clz     x3, x9
    add     x4, x1, x2, lsl #2          // the end
    mov     x2, #63
    cmp     x3, x2
    csel    x3, x2, x3, hi
    RNG_OPEN x10, x11
rng_below_fill.draw:
    RNG_TAKE x9, x10, x11
    lsr     x9, x9, x3
    cmp     x9, x0
    b.hs    rng_below_fill.draw         // rejected
    str     w9, [x1], #4
    cmp     x1, x4
    b.lo    rng_below_fill.draw
    RNG_CLOSE x10
rng_below_fill.none:
    ret

// canvas_random_row(w0=within text) -> x9  (Canvas.random_row)
canvas_random_row:
    cbz     w0, canvas_random_row.canvas
    LDX     x0, text_bottom
    LDX     x1, text_top
    b       rng_randint
canvas_random_row.canvas:
    mov     x0, #1
    LDX     x1, canvas_top
    b       rng_randint

// canvas_random_coord(w0=outside scope, w1=within text) -> x9 = coord.
// Canvas.random_coord, with its exact draw order: above, below, left, right
// are built (four draws), then one is chosen.
canvas_random_coord:
    sub     sp, sp, #64
    stp     x19, x21, [sp, #32]
    str     x30, [sp, #48]
    cbz     w0, canvas_random_coord.inside
    mov     w0, #0
    bl      canvas_random_column
    LDX     x3, canvas_top
    add     x3, x3, #1
    lsl     x3, x3, #32
    mov     w9, w9
    orr     x9, x9, x3
    str     x9, [sp]                    // above
    mov     w0, #0
    bl      canvas_random_column
    mov     w9, w9                      // row bottom - 1 = 0
    str     x9, [sp, #8]                // below
    mov     w0, #0
    bl      canvas_random_row
    lsl     x9, x9, #32                 // column left - 1 = 0
    str     x9, [sp, #16]               // left
    mov     w0, #0
    bl      canvas_random_row
    lsl     x9, x9, #32
    LDX     x3, canvas_right
    add     x3, x3, #1
    mov     w3, w3
    orr     x9, x9, x3
    str     x9, [sp, #24]               // right
    mov     x0, #4
    bl      rng_below
    ldr     x9, [sp, x9, lsl #3]
    b       canvas_random_coord.done
canvas_random_coord.inside:
    mov     w19, w1
    mov     w0, w1
    bl      canvas_random_column
    mov     x21, x9
    mov     w0, w19
    bl      canvas_random_row
    lsl     x9, x9, #32
    mov     w21, w21
    orr     x9, x9, x21
canvas_random_coord.done:
    ldr     x30, [sp, #48]
    ldp     x19, x21, [sp, #32]
    add     sp, sp, #64
    ret

// coord_in_canvas(x1=coord) -> w9 = 1 inside [1, right] x [1, top].
// Clobbers x16.
coord_in_canvas:
    asr     x9, x1, #32
    cmp     x9, #1
    b.lt    coord_in_canvas.no
    LDX     x16, canvas_top
    cmp     x9, x16
    b.gt    coord_in_canvas.no
    sxtw    x9, w1
    cmp     x9, #1
    b.lt    coord_in_canvas.no
    LDX     x16, canvas_right
    cmp     x9, x16
    b.gt    coord_in_canvas.no
    mov     w9, #1
    ret
coord_in_canvas.no:
    mov     w9, #0
    ret

    .section .rodata
// Anchor enum order: n ne e se s sw w nw c.
// column group: 1 = S|N|C (centered), 2 = SE|E|NE (east), 0 = west
anchor_column_group:    .byte 1, 2, 2, 2, 1, 0, 0, 0, 1
// row group: 1 = W|E|C (centered), 2 = NW|N|NE (north), 0 = south
anchor_row_group:       .byte 2, 2, 1, 0, 0, 0, 1, 2, 1

STRING msg_all_outside, "all input characters fall outside the canvas after anchoring"


    TSTATE
    .balign 8
term_width:         .skip 8
term_height:        .skip 8
canvas_top:         .skip 8
canvas_right:       .skip 8
center_row:         .skip 8
center_col:         .skip 8
col_offset:         .skip 8
row_offset:         .skip 8
visible_top:        .skip 8
visible_bottom:     .skip 8
visible_right:      .skip 8
visible_left:       .skip 8
text_top:           .skip 8
text_bottom:        .skip 8
text_left:          .skip 8
text_right:         .skip 8
text_center_row:    .skip 8
text_center_col:    .skip 8
coord_map:          .skip 8
coord_map_cells:    .skip 8
inner_fill_chars:   .skip 8
outer_fill_chars:   .skip 8
inner_fill_count:   .skip 4
outer_fill_count:   .skip 4
group_kind:         .skip 4
    .balign 8
sort_pairs_origin:  .skip 8
