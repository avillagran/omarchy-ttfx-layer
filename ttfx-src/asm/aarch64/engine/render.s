// engine/render.s - visibility and the frame renderer
// (terminal.rs update_render_cells + get_formatted_output_string).
//
// The cell grid keeps, per cell, the winning slot (maximum (layer,
// character_id), exactly Rust's painter order) and that winner's visual
// handle. Rust rebuilds it every frame; here it is maintained incrementally,
// by the renderer alone. The effect's side (set_visibility,
// coordinate_changed, layer_changed, SET_HANDLE) only appends records to the
// frame's change log (ttfx.inc), and the renderer replays the log before it
// formats the frame (render_apply):
//
//   * a visual change reaches the grid when its character owns the cell;
//   * a character entering a cell competes for it on the spot;
//   * movement, layer changes and hiding move the character between the
//     cells' occupant lists, and a cell whose owner left picks the best of the
//     rest (cell_rewin) once the log is replayed.
//
// Replaying the log in order gives the grid a direct update would, since the
// winner of a cell depends only on its occupants. That lets the renderer run
// on a thread of its own (the frame ring, below). Nothing outside this file
// reads the grid.
//
// Character ids ascend with slots (chars.s), so the character_id tie-break
// compares slots.
//
// Emission: each row keeps its bytes (with its joining newline) in storage of
// its own, and a frame is handed to the kernel as one iovec per row. A row
// none of whose cells changed since the previous frame is not touched at all.
// A row with changed cells is rebuilt into its second buffer: the runs of
// unchanged BLOCK-cell blocks are copied from the previous bytes (each row records
// where every block's bytes start), and only the changed blocks are formatted
// from the handle grid.
//
// EMPTY_SLOT, PENDING_SLOT, CR_*, RL_*, RC_*, RENDER_SPIN, BLOCK,
// BLOCK_SHIFT, MIN_GAP, CHUNK_SHIFT and OUTPUT_RESERVE are in defs.inc.

// dropped: pc_55 pc_33 pc_0f pc_01 (the popcount is NEON cnt, R_POPCNT)

// Offsets into the render block (the .tstate block from grid_width on),
// checked against the labels at the end of the file.
.equ RB_GRID_WIDTH, 0
.equ RB_CO_RBASE, 24
.equ RB_CO_RSPAN, 32
.equ RB_CO_CBASE, 40
.equ RB_CO_CSPAN, 48
.equ RB_CO_CELL0, 56

// R_POPCNT xreg, n: xn = the population count of xreg, through vn (n is a
// register number; xreg is left as it is).
.macro R_POPCNT xreg, vtmp
    fmov    d\vtmp, \xreg
    cnt     v\vtmp\().8b, v\vtmp\().8b
    addv    b\vtmp, v\vtmp\().8b
    umov    w\()\vtmp, v\vtmp\().b[0]
.endm

// R_CELL_AT: x9 = grid cell of (row x3, column x2), or NONE outside the
// visible window (row + co_rbase and col + co_cbase are unsigned in-window
// indices; the cell is row * width + col + co_cell0). Clobbers x3, x2, x16,
// x17.
.macro R_CELL_AT
    ADRG    x17, grid_width
    ldr     x16, [x17, #RB_CO_RBASE]
    add     x9, x3, x16
    ldr     x16, [x17, #RB_CO_RSPAN]
    cmp     x9, x16
    b.hi    .Lca_out\@
    ldr     x16, [x17, #RB_CO_CBASE]
    add     x9, x2, x16
    ldr     x16, [x17, #RB_CO_CSPAN]
    cmp     x9, x16
    b.hi    .Lca_out\@
    ldr     x16, [x17, #RB_GRID_WIDTH]
    mul     x3, x3, x16
    ldr     x16, [x17, #RB_CO_CELL0]
    add     x2, x2, x16
    add     x9, x3, x2
    b       .Lca_done\@
.Lca_out\@:
    mov     w9, #-1                     // NONE
.Lca_done\@:
.endm

// R_CELL_OF: x9 = grid cell of slot w0's current coordinate, or NONE outside
// the visible window. Clobbers x3, x2, x16, x17.
.macro R_CELL_OF
    LDX     x9, ch_row
    ldrsw   x3, [x9, w0, uxtw #2]
    LDX     x9, ch_col
    ldrsw   x2, [x9, w0, uxtw #2]
    R_CELL_AT
.endm

// R_MARK_DIRTY xcell, xscr: the cell changed; so did its CHUNK-cell chunk,
// which render_frame checks before it scans a row's cells. Both registers
// are clobbered, and x17.
.macro R_MARK_DIRTY xcell, xscr
    LDX     \xscr, dirty_cells
    mov     w17, #1
    strb    w17, [\xscr, \xcell]
    lsr     \xcell, \xcell, #CHUNK_SHIFT
    LDX     \xscr, dirty_chunks
    strb    w17, [\xscr, \xcell]
.endm

// The frame ring's atomics (no LSE: exclusive pairs). lib.s defines the same
// ones first when it includes this file.
// R_XCHGB wold, xaddr, wnew: xchg on a byte (wold = old value), then a full
// fence: the sleeping-flag handshakes load another variable next.
// R_ATOMIC_INC xaddr, wtmp: lock inc on a dword, a release.
// Both clobber w17.
.ifndef RENDER_ATOMICS
.equ RENDER_ATOMICS, 1
.macro R_XCHGB wold, xaddr, wnew
.Lrx\@:
    ldaxrb  \wold, [\xaddr]
    stlxrb  w17, \wnew, [\xaddr]
    cbnz    w17, .Lrx\@
    dmb     ish
.endm
.macro R_ATOMIC_INC xaddr, wtmp
.Lri\@:
    ldaxr   \wtmp, [\xaddr]
    add     \wtmp, \wtmp, #1
    stlxr   w17, \wtmp, [\xaddr]
    cbnz    w17, .Lri\@
.endm
.endif

    .section .rodata
    .balign 16
// bit weights: 16 bytes of 0/0xff to a 16-bit mask
render_mask_weights:
    .byte   1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128

    .text

// render_init: grids for the visible window, the visible list, the output.
render_init:
    stp     x19, x30, [sp, #-16]!
    LDX     x9, visible_right
    cmp     x9, #0
    csel    x9, xzr, x9, mi
    STX     x9, grid_width
    LDX     x2, visible_top
    cmp     x2, #0
    csel    x2, xzr, x2, mi
    STX     x2, grid_height
    mul     x9, x9, x2
    STX     x9, grid_cells
    // cell_of: row + co_rbase and col + co_cbase are unsigned in-window
    // indices, and the cell is row * width + col + co_cell0
    LDX     x9, row_offset
    LDX     x10, visible_bottom
    sub     x9, x9, x10
    LDX     x3, visible_top
    sub     x3, x3, x10
    mov     x2, #(1 << 40)              // an empty window matches nothing
    cmp     x3, #0
    csel    x9, x2, x9, mi
    csel    x3, xzr, x3, mi
    STX     x9, co_rbase
    STX     x3, co_rspan
    LDX     x9, col_offset
    LDX     x10, visible_left
    sub     x9, x9, x10
    LDX     x3, visible_right
    sub     x3, x3, x10
    cmp     x3, #0
    csel    x9, x2, x9, mi
    csel    x3, xzr, x3, mi
    STX     x9, co_cbase
    STX     x3, co_cspan
    LDX     x9, row_offset
    sub     x9, x9, #1
    LDX     x10, grid_width
    mul     x9, x9, x10
    LDX     x10, col_offset
    add     x9, x9, x10
    sub     x9, x9, #1
    STX     x9, co_cell0
    LDX     x0, grid_cells
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, handle_grid
    mov     x0, #(CHAR_LIMIT * 8)
    bl      reserve_small
    STX     x9, rs_link
    mov     x0, #(CHAR_LIMIT * 8)
    bl      reserve_small
    STX     x9, rs_cell
    LDX     x0, grid_cells
    lsl     x0, x0, #3
    add     x0, x0, #64
    bl      reserve
    STX     x9, cell_rec
    LDX     x0, grid_cells
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      reserve
    STX     x9, owner_grid
    // the empty grid: no occupants, no owners, blank cells, all to be drawn
    LDX     x0, cell_rec
    LDX     x3, grid_cells
    mov     w9, #-1                     // NONE
render_init.records:
    cbz     x3, render_init.owners
    str     w9, [x0], #8                // CR_HEAD
    sub     x3, x3, #1
    b       render_init.records
render_init.owners:
    LDX     x0, owner_grid
    LDX     x3, grid_cells
    REP_STOSD
    LDX     x0, handle_grid
    LDW     w9, space_handle
    LDX     x3, grid_cells
    REP_STOSD
    mov     w9, #1
    STB     w9, all_dirty
    // the change logs, one per ring entry, and the first one open
    mov     x0, #(FRAME_RING << LOG_SHIFT)
    bl      reserve_small               // touched a stretch per entry
    STX     x9, log_ptr
    ADRG    x3, ring
    mov     w2, #0
    mov     x10, #(1 << LOG_SHIFT)
render_init.logs:
    str     x9, [x3, #FS_LOG]
    add     x9, x9, x10
    add     x3, x3, #FS_SIZE
    add     w2, w2, #1
    cmp     w2, #FRAME_RING
    b.lo    render_init.logs
    MOV64   x0, OUTPUT_RESERVE
    bl      reserve_small
    STX     x9, out_base
    // each ring entry's pending bytes (a frame's prefix) get an equal share
    ADRG    x3, ring
    mov     w2, #0
    MOV64   x10, (OUTPUT_RESERVE / FRAME_RING)
render_init.prefixes:
    str     x9, [x3, #FS_PREFIX]
    add     x9, x9, x10
    add     x3, x3, #FS_SIZE
    add     w2, w2, #1
    cmp     w2, #FRAME_RING
    b.lo    render_init.prefixes
    // per-row storage, two buffers per row: width * VISUAL_MAX bytes +
    // newline + copy slack
    LDX     x9, grid_width
    lsl     x9, x9, #7
    add     x9, x9, #256
    STX     x9, row_stride
    LDX     x10, grid_height
    mul     x9, x9, x10
    lsl     x0, x9, #1
    add     x0, x0, #4096
    bl      reserve
    STX     x9, row_store
    // and two arrays of BLOCK-cell block starts per row: blocks + 1 entries +
    // slack
    LDX     x9, grid_width
    add     x9, x9, #(BLOCK - 1)
    lsr     x9, x9, #BLOCK_SHIFT
    STX     x9, row_blocks
    add     x3, x9, x9, lsl #1
    STX     x3, full_blocks             // 3/4 of the row's blocks, times 4
    lsl     x9, x9, #2
    add     x9, x9, #(33 * 4 + 63)
    and     x9, x9, #-64
    STX     x9, offs_stride
    LDX     x10, grid_height
    mul     x9, x9, x10
    lsl     x0, x9, #1
    add     x0, x0, #4096
    bl      reserve
    STX     x9, row_offs
    LDX     x19, grid_height
    add     x0, x19, #64
    bl      alloc
    STX     x9, row_sel
    // the rows' iovecs, top row first, and the frame's: one slot before
    // the rows (prefix / header) and one after them (the dump's trailing
    // newline)
    add     x0, x19, #2
    lsl     x0, x0, #4
    bl      alloc
    STX     x9, frame_iov
    add     x9, x9, #16
    STX     x9, row_iov
    add     x0, x19, #2
    lsl     x0, x0, #4
    bl      alloc
    STX     x9, iov_scratch
    LDX     x0, grid_cells
    add     x0, x0, #128
    bl      alloc
    STX     x9, dirty_cells
    LDX     x0, grid_cells
    lsr     x0, x0, #CHUNK_SHIFT
    add     x0, x0, #64
    bl      alloc
    STX     x9, dirty_chunks
    LDX     x0, grid_height
    add     x0, x0, #64
    bl      alloc
    STX     x9, dirty_rows
    LDX     x0, grid_cells
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, pending_cells
    LDX     x0, grid_width
    lsr     x0, x0, #6
    add     x0, x0, #64
    bl      alloc
    STX     x9, dirty_bits
    ldp     x19, x30, [sp], #16
    ret

// cell_of(w0=slot) -> x9 = grid cell of the character's current coordinate,
// or NONE outside the visible window. Clobbers x3, x2.
cell_of:
    R_CELL_OF
    ret

// ------------------------------------------------------------ the main side
// These run with the effect: they keep ch_cell and append to the frame's
// change log (ttfx.inc), and never touch the grid itself.

// R_LOG_REC op, wval: append (slot w0 | op, wval). Clobbers x3, x2, x17.
.macro R_LOG_REC op, wval
    adrp    x17, log_ptr
    ldr     x3, [x17, :lo12:log_ptr]
    orr     w2, w0, #\op
    stp     w2, \wval, [x3], #8
    str     x3, [x17, :lo12:log_ptr]
.endm

// set_visibility(w0=slot, w1=visible): Terminal.set_character_visibility.
// Clobbers x9, x3, x2.
set_visibility:
    cbnz    w1, set_visible
    LDX     x9, ch_flags
    ldrh    w2, [x9, w0, uxtw #1]
    tst     w2, #CF_VISIBLE
    b.eq    set_visibility.done
    and     w2, w2, #(0xffff & ~CF_VISIBLE)
    strh    w2, [x9, w0, uxtw #1]
    LDX     x9, ch_cell
    ldr     w2, [x9, w0, uxtw #2]
    cmn     w2, #1                      // NONE
    b.eq    set_visibility.done
    mov     w2, #-1
    str     w2, [x9, w0, uxtw #2]
    mov     w9, #-1
    R_LOG_REC LOG_MOVE, w9
set_visibility.done:
    ret

// set_visible(w0=slot): Terminal.set_character_visibility(id, true).
// Clobbers x9, x3, x2.
set_visible:
    LDX     x9, ch_flags
    ldrh    w2, [x9, w0, uxtw #1]
    tst     w2, #CF_VISIBLE
    b.ne    set_visible.done
    orr     w2, w2, #CF_VISIBLE
    strh    w2, [x9, w0, uxtw #1]
    R_CELL_OF
    cmn     w9, #1
    b.ne    enter_cell
set_visible.done:
    ret

// enter_cell(w0=slot, w9=cell): a character without a cell takes one. The
// renderer learns its visual and layer first. Clobbers x3, x2.
enter_cell:
    LDX     x3, ch_cell
    str     w9, [x3, w0, uxtw #2]
    adrp    x17, log_ptr
    ldr     x3, [x17, :lo12:log_ptr]
    LDB     w2, log_handles
    cbz     w2, enter_cell.unlogged
    LDX     x2, ch_handle
    ldr     w2, [x2, w0, uxtw #2]
    stp     w0, w2, [x3]                // LOG_HANDLE
    LDX     x2, ch_layer
    ldr     w2, [x2, w0, uxtw #2]
    str     w2, [x3, #12]
    orr     w2, w0, #LOG_LAYER
    str     w2, [x3, #8]
    eor     w2, w2, #(LOG_LAYER | LOG_MOVE)
    stp     w2, w9, [x3, #16]
    add     x3, x3, #24
    str     x3, [x17, :lo12:log_ptr]
    ret
enter_cell.unlogged:
    // the renderer reads the visual from ch_handle
    LDX     x2, ch_layer
    ldr     w2, [x2, w0, uxtw #2]
    str     w2, [x3, #4]
    orr     w2, w0, #LOG_LAYER
    str     w2, [x3]
    eor     w2, w2, #(LOG_LAYER | LOG_MOVE)
    stp     w2, w9, [x3, #8]
    add     x3, x3, #16
    str     x3, [x17, :lo12:log_ptr]
    ret

// handle_direct(w0=slot, w9=handle, x3=its cell): SET_HANDLE without a
// render thread: the cell shows the visual at once when the character owns
// it. The grid is only the renderer's between frames, and one whose move is
// still in the log settles when the log is replayed. Preserves everything
// but x3, x16, x17 (and the flags).
handle_direct:
    LDX     x16, owner_grid
    ldr     w17, [x16, x3, lsl #2]
    cmp     w17, w0
    b.ne    handle_direct.done
    LDX     x16, handle_grid
    ldr     w17, [x16, x3, lsl #2]
    cmp     w17, w9
    b.eq    handle_direct.done
    str     w9, [x16, x3, lsl #2]
    R_MARK_DIRTY x3, x16
handle_direct.done:
    ret

// coordinate_changed(w0=slot, x1=its new coordinate, packed): the
// character's current coordinate changed (set_coordinate); a visible
// character that changes cells logs the move. Clobbers x9, x3, x2.
coordinate_changed:
    LDX     x9, ch_flags
    ldrh    w2, [x9, w0, uxtw #1]
    tst     w2, #CF_VISIBLE
    b.eq    coordinate_changed.done
    // the cell, as R_CELL_OF
    asr     x3, x1, #32
    sxtw    x2, w1
    R_CELL_AT
coordinate_changed.cell:
    LDX     x3, ch_cell
    ldr     w2, [x3, w0, uxtw #2]
    cmp     w2, w9
    b.eq    coordinate_changed.done
    cmn     w2, #1
    b.eq    enter_cell
    str     w9, [x3, w0, uxtw #2]
    R_LOG_REC LOG_MOVE, w9
coordinate_changed.done:
    ret

// layer_changed(w0=slot): the character's layer changed. Clobbers x9, x3,
// x2.
layer_changed:
    LDX     x9, ch_cell
    ldr     w9, [x9, w0, uxtw #2]
    cmn     w9, #1
    b.eq    layer_changed.done
    LDX     x9, ch_layer
    ldr     w9, [x9, w0, uxtw #2]
    R_LOG_REC LOG_LAYER, w9
layer_changed.done:
    ret

// set_handle(w0=slot, w9=handle): see SET_HANDLE in ttfx.inc.
set_handle:
    str     x30, [sp, #-16]!
    SET_HANDLE
    ldr     x30, [sp], #16
    ret

// ------------------------------------------------------------ the render side
// The renderer's own copy of what it needs per character, in dense arrays
// indexed by slot like the ch_* fields: [rs_link] holds (next occupant of
// the cell, layer), the pair a cell's scan reads; [rs_cell] holds (previous
// occupant, cell + 1, 0 = none), so the zeroed reservation starts out right
// for every slot; [rs_handle] the visual.

// render_apply(x1=log start, x0=log end): replay a change log onto the
// grid. Clobbers C but x19-x24.
render_apply:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    str     x30, [sp, #32]
    mov     x21, x1
    mov     x22, x0
    LDX     x19, rs_link
    LDX     x23, rs_cell
render_apply.next:
    cmp     x21, x22
    b.hs    render_apply.done
    ldp     w0, w9, [x21], #8
    lsr     w3, w0, #30
    and     w0, w0, #LOG_SLOT_MASK
    cmp     w3, #1
    b.lo    render_apply.handle
    b.eq    render_apply.layer
    // a move: leave the old cell, join the new one
    mov     w11, w9
    bl      cell_unlink
    cmn     w11, #1
    b.eq    render_apply.next
    mov     w9, w11
    bl      cell_link
    b       render_apply.next
render_apply.handle:
    // the owner of a cell shows its new visual at once
    LDX     x2, rs_handle
    str     w9, [x2, x0, lsl #2]
    add     x16, x23, x0, lsl #3
    ldr     w3, [x16, #RC_CELL]
    cbz     w3, render_apply.next
    sub     w3, w3, #1
    LDX     x2, owner_grid
    ldr     w16, [x2, x3, lsl #2]
    cmp     w16, w0
    b.ne    render_apply.next
    LDX     x2, handle_grid
    ldr     w16, [x2, x3, lsl #2]
    cmp     w16, w9
    b.eq    render_apply.next
    str     w9, [x2, x3, lsl #2]
    R_MARK_DIRTY x3, x2
    b       render_apply.next
render_apply.layer:
    // the cell picks its owner again
    add     x16, x19, x0, lsl #3
    str     w9, [x16, #RL_LAYER]
    add     x16, x23, x0, lsl #3
    ldr     w9, [x16, #RC_CELL]
    cbz     w9, render_apply.next
    sub     w9, w9, #1
    bl      cell_pending
    b       render_apply.next
render_apply.done:
    // the crowded cells whose owner left choose again, once each
    LDX     x21, pending_cells
    ADRG    x16, pending_count
    ldr     w22, [x16]
    str     wzr, [x16]
render_apply.pending:
    cbz     w22, render_apply.applied
    sub     w22, w22, #1
    ldr     w9, [x21, x22, lsl #2]
    bl      cell_rewin
    b       render_apply.pending
render_apply.applied:
    ldr     x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// cell_link(w0=slot, w9=cell): put a visible character into a cell's list
// and let it compete for the cell. x19 = [rs_link], x23 = [rs_cell].
// Clobbers x3, x2, x4.
cell_link:
    add     w3, w9, #1
    add     x16, x23, x0, lsl #3
    str     w3, [x16, #RC_CELL]
    mov     w3, #-1                     // NONE
    str     w3, [x16, #RC_PREV]
    LDX     x4, cell_rec
    add     x4, x4, x9, lsl #3
    ldr     w2, [x4, #CR_HEAD]
    add     x16, x19, x0, lsl #3
    str     w2, [x16, #RL_NEXT]
    str     w0, [x4, #CR_HEAD]
    cmn     w2, #1
    b.eq    paint_rec
    add     x16, x23, x2, lsl #3
    str     w0, [x16, #RC_PREV]
// paint_rec(w0=slot, w9=cell, x4=the cell's record): take the cell when
// empty or when this character outranks its owner on (layer, character_id);
// a cell whose owner is pending chooses later. Clobbers x3, x2.
paint_rec:
    add     x16, x19, x0, lsl #3
    ldr     w3, [x16, #RL_LAYER]
    LDX     x2, owner_grid
    ldr     w2, [x2, x9, lsl #2]
    cmn     w2, #2                      // PENDING_SLOT
    b.hs    paint_rec.other
    ldr     w16, [x4, #CR_LAYER]
    cmp     w3, w16
    b.gt    paint_rec.take
    b.lt    paint_rec.keep
    cmp     w0, w2
    b.ls    paint_rec.keep
paint_rec.take:
    LDX     x2, owner_grid
    str     w0, [x2, x9, lsl #2]
    str     w3, [x4, #CR_LAYER]
    LDX     x3, rs_handle
    ldr     w3, [x3, x0, lsl #2]
    LDX     x2, handle_grid
    str     w3, [x2, x9, lsl #2]
    mov     x3, x9
    R_MARK_DIRTY x3, x2
paint_rec.keep:
    ret
paint_rec.other:
    b.eq    paint_rec.keep              // pending
    b       paint_rec.take              // empty

// cell_unlink(w0=slot): take a character out of its cell; if it owned the
// cell, the best remaining character (or nobody) takes over: at once when
// one or none is left, else once the log is replayed. x19 = [rs_link],
// x23 = [rs_cell]. Clobbers x9, x3, x2, x1, x4, x5.
cell_unlink:
    add     x16, x23, x0, lsl #3
    ldr     w5, [x16, #RC_CELL]
    cbz     w5, cell_unlink.done
    str     wzr, [x16, #RC_CELL]
    sub     w5, w5, #1
    LDX     x4, cell_rec
    add     x4, x4, x5, lsl #3
    add     x17, x19, x0, lsl #3
    ldr     w2, [x17, #RL_NEXT]
    ldr     w9, [x16, #RC_PREV]
    cmn     w9, #1
    b.eq    cell_unlink.was_head
    add     x17, x19, x9, lsl #3
    str     w2, [x17, #RL_NEXT]
    b       cell_unlink.fix_next
cell_unlink.was_head:
    str     w2, [x4, #CR_HEAD]
cell_unlink.fix_next:
    cmn     w2, #1
    b.eq    cell_unlink.owner
    add     x17, x23, x2, lsl #3
    str     w9, [x17, #RC_PREV]
cell_unlink.owner:
    LDX     x3, owner_grid
    ldr     w3, [x3, x5, lsl #2]
    cmp     w3, w0
    b.ne    cell_unlink.done
    mov     w9, w5
    ldr     w3, [x4, #CR_HEAD]
    cmn     w3, #1
    b.eq    cell_rewin                  // empty now
    add     x17, x19, x3, lsl #3
    ldr     w17, [x17, #RL_NEXT]
    cmn     w17, #1
    b.eq    cell_rewin                  // one left
    b       cell_pending
cell_unlink.done:
    ret

// cell_pending(w9=cell): the cell's owner is chosen again (cell_rewin) once
// the log is replayed: a crowded cell that many leave in one frame is
// scanned once, not once per departure. Clobbers x3, x2.
cell_pending:
    LDX     x3, owner_grid
    ldr     w2, [x3, x9, lsl #2]
    cmn     w2, #2                      // PENDING_SLOT
    b.eq    cell_pending.done
    mov     w2, #-2
    str     w2, [x3, x9, lsl #2]
    LDX     x3, pending_cells
    ADRG    x17, pending_count
    ldr     w2, [x17]
    str     w9, [x3, x2, lsl #2]
    add     w2, w2, #1
    str     w2, [x17]
cell_pending.done:
    ret

// cell_rewin(w9=cell): the cell's owner is the best of its list, or nobody;
// the cell shows it and is marked changed. x19 = [rs_link]. Clobbers x3,
// x2, x1, x4, x5.
cell_rewin:
    LDX     x5, cell_rec
    add     x5, x5, x9, lsl #3
    ldr     w3, [x5, #CR_HEAD]          // candidate
    mov     w6, #-1                     // best so far, w1 its layer
cell_rewin.scan:
    cmn     w3, #1
    b.eq    cell_rewin.chosen
    add     x17, x19, x3, lsl #3
    ldr     w4, [x17, #RL_LAYER]
    ldr     w2, [x17, #RL_NEXT]
    cmn     w6, #1
    b.eq    cell_rewin.take
    cmp     w4, w1
    b.gt    cell_rewin.take
    b.lt    cell_rewin.next
    cmp     w3, w6
    b.ls    cell_rewin.next
cell_rewin.take:
    mov     w6, w3
    mov     w1, w4
cell_rewin.next:
    mov     w3, w2
    b       cell_rewin.scan
cell_rewin.chosen:
    LDX     x2, owner_grid
    str     w6, [x2, x9, lsl #2]
    LDX     x2, handle_grid
    cmn     w6, #1
    b.eq    cell_rewin.empty
    str     w1, [x5, #CR_LAYER]
    LDX     x3, rs_handle
    ldr     w3, [x3, x6, lsl #2]
    str     w3, [x2, x9, lsl #2]
    b       cell_rewin.dirty
cell_rewin.empty:
    LDW     w3, space_handle
    str     w3, [x2, x9, lsl #2]
cell_rewin.dirty:
    mov     x3, x9
    R_MARK_DIRTY x3, x1
    ret

// row_dirty(x9=first cell of the row) -> x9 = the number of the row's
// BLOCK-cell blocks with changed cells. When there are any, also writes the
// row's block bitmap to [dirty_bits] (one bit per block, plus a set bit at
// index row_blocks: a sentinel for the run scan) and clears the row's dirty
// bytes. x23 = width, nonzero. Clobbers x3, x2, x1, x4, x5, x10, x11,
// vector registers.
//
// The last 64-byte chunk reads and clears past the row's end: that is the
// next row up in memory, which render_frame has already scanned (it goes
// from the highest row index down), or the zeroed slack after the grid.
row_dirty:
    LDX     x3, dirty_cells
    add     x3, x3, x9
    // most rows are clean: a first pass only ORs the bytes together
    mov     x2, x3
    mov     x5, x23
    movi    v4.16b, #0
    movi    v0.16b, #0
row_dirty.any:
    ldp     q1, q2, [x2]
    orr     v0.16b, v0.16b, v1.16b
    orr     v0.16b, v0.16b, v2.16b
    ldp     q1, q2, [x2, #32]
    orr     v0.16b, v0.16b, v1.16b
    orr     v0.16b, v0.16b, v2.16b
    add     x2, x2, #64
    subs    x5, x5, #64
    b.hi    row_dirty.any
    umaxv   b0, v0.16b
    umov    w9, v0.b[0]
    cbz     w9, row_dirty.clean
    // 64 / BLOCK block bits per 64 cells, gathered into whole words in x9
    // (narrow stores would stall the word loads of the run scan)
    mov     x1, x3
    LDX     x4, dirty_bits
    mov     x5, x23
    mov     x10, #0
    mov     x9, #0
    mov     w3, #0
    ADRG    x16, render_mask_weights
    ldr     q5, [x16]
row_dirty.chunk:
    // a block's 4 dirty bytes are one 32-bit lane: nonzero lanes to 0xff
    // bytes in block order, then to a 16-bit mask
    ldp     q0, q1, [x1]
    ldp     q2, q3, [x1, #32]
    cmtst   v0.4s, v0.4s, v0.4s
    cmtst   v1.4s, v1.4s, v1.4s
    cmtst   v2.4s, v2.4s, v2.4s
    cmtst   v3.4s, v3.4s, v3.4s
    uzp1    v0.8h, v0.8h, v1.8h
    uzp1    v2.8h, v2.8h, v3.8h
    uzp1    v0.16b, v0.16b, v2.16b
    and     v0.16b, v0.16b, v5.16b
    addp    v0.16b, v0.16b, v0.16b
    addp    v0.16b, v0.16b, v0.16b
    addp    v0.16b, v0.16b, v0.16b
    umov    w11, v0.h[0]
    stp     q4, q4, [x1]
    stp     q4, q4, [x1, #32]
    lsl     x11, x11, x3
    orr     x9, x9, x11
    add     w3, w3, #(64 / BLOCK)
    cmp     w3, #64
    b.lo    row_dirty.next
    str     x9, [x4], #8
    R_POPCNT x9, 6
    add     x10, x10, x6
    mov     x9, #0
    mov     w3, #0
row_dirty.next:
    add     x1, x1, #64
    subs    x5, x5, #64
    b.hi    row_dirty.chunk
    str     x9, [x4]
    str     xzr, [x4, #8]
    R_POPCNT x9, 6
    add     x10, x10, x6
    LDX     x4, dirty_bits
    LDX     x2, row_blocks
    mov     w3, w2
    lsr     x2, x2, #6
    mov     x5, #1
    lsl     x5, x5, x3
    ldr     x16, [x4, x2, lsl #3]
    orr     x16, x16, x5
    str     x16, [x4, x2, lsl #3]
    mov     x9, x10
    ret
row_dirty.clean:
    mov     x9, #0
    ret

// next_set(x9=block) -> x9 = the first block >= x9 whose dirty bit is set
// (row_blocks at the latest, through the sentinel). Clobbers x3, x2, x4.
next_set:
    LDX     x4, dirty_bits
    mov     w3, w9
    lsr     x9, x9, #6
    mov     x2, #-1
    lsl     x2, x2, x3
    ldr     x16, [x4, x9, lsl #3]
    ands    x2, x2, x16
    b.ne    next_set.found
next_set.word:
    add     x9, x9, #1
    ldr     x2, [x4, x9, lsl #3]
    cbz     x2, next_set.word
next_set.found:
    rbit    x2, x2
    clz     x2, x2
    add     x9, x2, x9, lsl #6
    ret

// next_clear(x9=block < row_blocks) -> x9 = the first block >= x9 whose
// dirty bit is clear, capped at row_blocks. Clobbers x3, x2, x4.
next_clear:
    LDX     x4, dirty_bits
    mov     w3, w9
    lsr     x9, x9, #6
    mov     x2, #-1
    lsl     x2, x2, x3
    ldr     x3, [x4, x9, lsl #3]
    bics    x2, x2, x3
    b.ne    next_clear.found
next_clear.word:
    add     x9, x9, #1
    ldr     x2, [x4, x9, lsl #3]
    mvn     x2, x2
    cbz     x2, next_clear.word
next_clear.found:
    rbit    x2, x2
    clz     x2, x2
    add     x9, x2, x9, lsl #6
    LDX     x3, row_blocks
    cmp     x9, x3
    csel    x9, x3, x9, hi
    ret

// row_buffers(x20=row, w3=0 current / 1 other) -> x9 = bytes, x2 = block
// starts. Clobbers x3.
row_buffers:
    LDX     x9, row_sel
    ldrb    w9, [x9, x20]
    eor     w3, w3, w9
    add     x3, x3, x20, lsl #1         // buffer index
    LDX     x9, row_stride
    mul     x9, x9, x3
    LDX     x16, row_store
    add     x9, x9, x16
    LDX     x16, offs_stride
    mul     x3, x3, x16
    LDX     x2, row_offs
    add     x2, x2, x3
    ret

// render_frame -> x9 = frame length. The frame's rows are frame_iov[1] ..
// frame_iov[grid_height] (row_iov[0] ..), top row first; frame_iov[0] and the
// entry after the rows are the caller's. The array is the rows' own record, so
// it is written with writev_keep.
render_frame:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x22, handle_grid
    LDX     x23, grid_width
    LDX     x20, grid_height
    LDX     x19, pool_base
    cbz     x20, render_frame.done
    LDB     w9, all_dirty
    cbnz    w9, render_frame.row
    bl      rows_marked
render_frame.row:
    sub     x20, x20, #1                // row index, counting down
    cbz     x23, render_frame.scanned
    mul     x9, x20, x23
    LDB     w3, all_dirty
    cbnz    w3, render_frame.whole
    // a row none of whose chunks changed is clean
    LDX     x3, dirty_rows
    ldrb    w2, [x3, x20]
    cbz     w2, render_frame.next
    strb    wzr, [x3, x20]
    bl      row_dirty
    cbz     x9, render_frame.next
    // a mostly dirty row is cheaper to format whole
    lsl     x9, x9, #2
    LDX     x3, full_blocks
    cmp     x9, x3
    b.hs    render_frame.full
    bl      row_rebuild
    b       render_frame.next
render_frame.whole:
    bl      row_dirty                   // clears the row's marks
    b       render_frame.full
render_frame.scanned:
    LDB     w3, all_dirty
    cbz     w3, render_frame.next
render_frame.full:
    bl      row_full
render_frame.next:
    cbnz    x20, render_frame.row
    LDB     w3, all_dirty
    cbz     w3, render_frame.done
    // the chunk marks are spent
    LDX     x0, dirty_chunks
    LDX     x3, grid_cells
    lsr     x3, x3, #CHUNK_SHIFT
    add     x3, x3, #1
    mov     w9, #0
    REP_STOSB
render_frame.done:
    STB     wzr, all_dirty
    LDX     x9, frame_len
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// rows_marked: flag in [dirty_rows] every row that a marked chunk touches,
// and clear the chunk marks. One walk up the chunks and rows together.
// x23 = width, nonzero. Clobbers x9, x3, x2, x1, x0, x4, x5, x10, x11.
rows_marked:
    LDX     x1, dirty_chunks
    LDX     x0, dirty_rows
    LDX     x10, grid_cells
    add     x10, x10, #((1 << CHUNK_SHIFT) - 1)
    lsr     x10, x10, #CHUNK_SHIFT      // chunks
    mov     x3, #0                      // chunk
    mov     x4, #0                      // row
    mov     x5, x23                     // the row's end cell
    LDX     x11, grid_height
    mov     w13, #1
rows_marked.eight:
    cmp     x3, x10
    b.hs    rows_marked.done
    ldr     x16, [x1, x3]
    cbnz    x16, rows_marked.bytes
    add     x3, x3, #8
    b       rows_marked.eight
rows_marked.bytes:
    add     x2, x3, #8                  // the eight's end
rows_marked.byte:
    ldrb    w16, [x1, x3]
    cbz     w16, rows_marked.skip
    strb    wzr, [x1, x3]
    lsl     x9, x3, #CHUNK_SHIFT        // the chunk's first cell
rows_marked.find:
    cmp     x5, x9
    b.hi    rows_marked.found
    add     x4, x4, #1
    add     x5, x5, x23
    b       rows_marked.find
rows_marked.found:
    add     x9, x9, #((1 << CHUNK_SHIFT) - 1)   // its last
    mov     x14, x4
    mov     x15, x5
rows_marked.flag:
    cmp     x14, x11
    b.hs    rows_marked.skip
    strb    w13, [x0, x14]
    cmp     x15, x9
    b.hi    rows_marked.skip
    add     x14, x14, #1
    add     x15, x15, x23
    b       rows_marked.flag
rows_marked.skip:
    add     x3, x3, #1
    cmp     x3, x2
    b.lo    rows_marked.byte
    b       rows_marked.eight
rows_marked.done:
    ret

// row_end(x20=row, x0=end of the row's cells, x1=block starts, x24 = the
// row's first byte): record the end of the cells, add the joining newline,
// and update the row's iovec and the frame length. Clobbers x9, x3, x0.
row_end:
    LDX     x9, row_blocks
    str     w0, [x1, x9, lsl #2]
    // every row but the bottom one carries the newline that joins it to the next
    cbz     x20, row_end.emitted
    mov     w16, #10
    strb    w16, [x0], #1
row_end.emitted:
    sub     x0, x0, x24
    // the top row's iovec comes first
    LDX     x9, grid_height
    sub     x9, x9, x20
    LDX     x16, row_iov
    add     x9, x16, x9, lsl #4
    stur    x24, [x9, #-16]
    ldur    x3, [x9, #-8]
    sub     x3, x0, x3
    ADRG    x16, frame_len
    ldr     x17, [x16]
    add     x17, x17, x3
    str     x17, [x16]
    stur    x0, [x9, #-8]
    ret

// row_full(x20=row): format every cell of the row into its current buffer.
// x22 = handle grid, x23 = width, x19 = pool base. Clobbers C but x19-x23.
row_full:
    stp     x24, x30, [sp, #-32]!
    mov     w3, #0
    bl      row_buffers
    mov     x24, x9
    mov     x0, x9
    mov     x5, x2
    str     x2, [sp, #16]
    mul     x1, x20, x23
    add     x1, x22, x1, lsl #2         // handles of this row
    add     x2, x1, x23, lsl #2
    bl      emit_blocks
    ldr     x1, [sp, #16]
    bl      row_end
    ldp     x24, x30, [sp], #32
    ret

// row_rebuild(x20=row): rebuild a row with dirty blocks into its other
// buffer, which becomes current. Runs of clean blocks are copied from the old
// bytes, their starts shifted; dirty runs are formatted. x22 = handle grid,
// x23 = width, x19 = pool base, the row's bitmap in [dirty_bits].
// Clobbers C but x19-x23.
row_rebuild:
    sub     sp, sp, #64
    stp     x21, x24, [sp, #32]
    str     x30, [sp, #48]
    // [sp] = old bytes, [sp + 8] = old block starts, [sp + 16] = new ones,
    // [sp + 24] = the run end across next_set
    mov     w3, #0
    bl      row_buffers
    stp     x9, x2, [sp]
    mov     w3, #1
    bl      row_buffers
    mov     x24, x9
    str     x2, [sp, #16]
    LDX     x9, row_sel
    ldrb    w3, [x9, x20]
    eor     w3, w3, #1
    strb    w3, [x9, x20]
    mov     x0, x24                     // output
    mov     x21, #0                     // block
row_rebuild.run:
    mov     x9, x21
    bl      next_set
    cmp     x9, x21
    b.eq    row_rebuild.dirty
    // clean blocks x21 .. x9 - 1: their bytes move as one piece
    ldr     x10, [sp, #8]
    ldr     x11, [sp, #16]
    ldr     w3, [x10, x21, lsl #2]      // start of the piece, old
    ldr     w2, [x10, x9, lsl #2]       // its end
    sub     w2, w2, w3                  // its length
    sub     w4, w0, w3                  // start shift (mod 2^32)
    // the starts: new = old + shift, a vector at a time (runs over into the
    // next dirty run's entries, which it rewrites, or the slack)
    lsl     x5, x21, #2
    lsl     x1, x9, #2
    dup     v0.4s, w4
row_rebuild.starts:
    ldr     q1, [x10, x5]
    add     v1.4s, v1.4s, v0.4s
    str     q1, [x11, x5]
    add     x5, x5, #16
    cmp     x5, x1
    b.lo    row_rebuild.starts
    // the bytes (also running over by up to one 64-byte line)
    mov     x21, x9
    ldr     x1, [sp]
    sub     w9, w3, w1                  // offset of the piece in the old row
    add     x1, x1, x9
    mov     x3, x0
    add     x0, x0, x2
    // one unaligned line, then the next lines from the aligned one on
    ldp     q0, q1, [x1]
    ldp     q2, q3, [x1, #32]
    stp     q0, q1, [x3]
    stp     q2, q3, [x3, #32]
    neg     w9, w3
    ands    w9, w9, #63
    b.ne    row_rebuild.align
    mov     w9, #64                     // already aligned: the first line is done
row_rebuild.align:
    add     x1, x1, x9
    add     x3, x3, x9
    subs    x2, x2, x9
    b.ls    row_rebuild.copied
row_rebuild.bytes:
    ldp     q0, q1, [x1]
    ldp     q2, q3, [x1, #32]
    stp     q0, q1, [x3]
    stp     q2, q3, [x3, #32]
    add     x1, x1, #64
    add     x3, x3, #64
    subs    x2, x2, #64
    b.hi    row_rebuild.bytes
row_rebuild.copied:
    LDX     x9, row_blocks
    cmp     x21, x9
    b.hs    row_rebuild.end
row_rebuild.dirty:
    // dirty blocks x21 .. next clear - 1
    mov     x9, x21
    bl      next_clear
row_rebuild.extend:
    // a short clean gap joins the run
    LDX     x3, row_blocks
    cmp     x9, x3
    b.hs    row_rebuild.extended
    str     x9, [sp, #24]
    bl      next_set
    ldr     x3, [sp, #24]
    add     x2, x3, #MIN_GAP
    cmp     x9, x2
    b.hs    row_rebuild.gap_kept
    LDX     x2, row_blocks
    cmp     x9, x2
    b.hs    row_rebuild.gap_kept
    bl      next_clear
    b       row_rebuild.extend
row_rebuild.gap_kept:
    mov     x9, x3
row_rebuild.extended:
    ldr     x5, [sp, #16]
    add     x5, x5, x21, lsl #2
    mul     x3, x20, x23
    add     x3, x22, x3, lsl #2         // handles of this row
    add     x2, x3, x23, lsl #2         // their end
    lsl     x21, x21, #(BLOCK_SHIFT + 2)
    add     x1, x3, x21
    mov     x21, x9
    add     x9, x3, x9, lsl #(BLOCK_SHIFT + 2)
    cmp     x9, x2
    csel    x2, x9, x2, lo
    bl      emit_blocks
    LDX     x9, row_blocks
    cmp     x21, x9
    b.lo    row_rebuild.run
row_rebuild.end:
    ldr     x1, [sp, #16]
    bl      row_end
    ldr     x30, [sp, #48]
    ldp     x21, x24, [sp, #32]
    add     sp, sp, #64
    ret

// emit_blocks(x1=first handle, at a block start, x2=end, x0=output,
// x5=the first block's start entry) -> x0 advanced. Each 4-cell block's
// start (the low half of its output address) is stored at [x5], x5
// advancing. x19 = pool base. Clobbers x9, x3, x1, x4, x5, x10, x11,
// vector registers.
emit_blocks:
emit_blocks.quad:
    // a block per iteration; one test covers all four lengths (< 32 bytes
    // leaves bits 5-7 of every length clear, so their OR stays below 32)
    add     x4, x1, #16
    cmp     x4, x2
    b.hi    emit_blocks.tail
    str     w0, [x5], #4
    ldp     w9, w3, [x1]
    ldp     w4, w10, [x1, #8]
    orr     w11, w9, w3
    orr     w11, w11, w4
    orr     w11, w11, w10
    tst     w11, #0xE0000000
    b.ne    emit_blocks.slow
    add     x1, x1, #16
    lsr     w11, w9, #HANDLE_LEN_SHIFT
    and     w9, w9, #HANDLE_OFFSET_MASK
    add     x16, x19, x9
    ldp     q0, q1, [x16]
    lsr     w12, w3, #HANDLE_LEN_SHIFT
    and     w3, w3, #HANDLE_OFFSET_MASK
    add     x17, x19, x3
    ldp     q2, q3, [x17]
    stp     q0, q1, [x0]
    add     x0, x0, x11
    lsr     w11, w4, #HANDLE_LEN_SHIFT
    and     w4, w4, #HANDLE_OFFSET_MASK
    add     x16, x19, x4
    ldp     q4, q5, [x16]
    stp     q2, q3, [x0]
    add     x0, x0, x12
    lsr     w12, w10, #HANDLE_LEN_SHIFT
    and     w10, w10, #HANDLE_OFFSET_MASK
    add     x17, x19, x10
    ldp     q6, q7, [x17]
    stp     q4, q5, [x0]
    add     x0, x0, x11
    stp     q6, q7, [x0]
    add     x0, x0, x12
    b       emit_blocks.quad
emit_blocks.slow:
    // a block with a visual of 32 bytes or more: cell by cell
    add     x12, x1, #16
    b       emit_blocks.cell
emit_blocks.tail:
    // the row's last, partial block
    cmp     x1, x2
    b.hs    emit_blocks.done
    str     w0, [x5], #4
    mov     x12, x2
emit_blocks.cell:
    cmp     x1, x12
    b.hs    emit_blocks.quad
    ldr     w9, [x1], #4
    lsr     w3, w9, #HANDLE_LEN_SHIFT
    and     w9, w9, #HANDLE_OFFSET_MASK
    add     x16, x19, x9
    cmp     w3, #32
    b.hi    emit_blocks.wide
    ldp     q0, q1, [x16]
    stp     q0, q1, [x0]
    add     x0, x0, x3
    b       emit_blocks.cell
emit_blocks.wide:
    // 128 bytes: a visual of any length
    ldp     q0, q1, [x16]
    ldp     q2, q3, [x16, #32]
    ldp     q4, q5, [x16, #64]
    ldp     q6, q7, [x16, #96]
    stp     q0, q1, [x0]
    stp     q2, q3, [x0, #32]
    stp     q4, q5, [x0, #64]
    stp     q6, q7, [x0, #96]
    add     x0, x0, x3
    b       emit_blocks.cell
emit_blocks.done:
    ret

// ------------------------------------------------------------ the frame ring
// Unpaced runs render on a thread of their own: the main thread runs the
// effect and logs its grid changes; the render thread replays frame N's log,
// formats and writes frame N while the main thread computes frame N+1. A
// frame is handed over through a ring of FRAME_RING entries (its log and its
// prefix bytes); r_submitted and r_completed count frames through it. Each
// side sleeps on a futex word of its own (render_seq, main_seq) with a
// *_sleeping flag the other side checks after its update and a full fence,
// so a wake is only a syscall when someone sleeps. Without the thread, the
// main thread renders each frame itself from ring entry 0 (frame_submit,
// lib.s).
//
// Ordering (x86 gets it from TSO and its locked instructions): the count
// increments are releases (stlxr), so a frame's log, ring entry and prefix
// bytes (main) or its reads of the entry and render_err (renderer) come
// before them; the other side reads the count with ldar. Each sleeping-flag
// check is a store (count or flag) and a later load of the other variable,
// with a dmb ish in between on both sides.

// render_emit(x0=ring entry) -> x9 = 0 or -errno: replay the entry's log,
// format the frame, and write it with the entry's prefix bytes first.
// Clobbers C but x19-x24.
render_emit:
    stp     x19, x30, [sp, #-16]!
    mov     x19, x0
    ldr     x1, [x19, #FS_LOG]
    ldr     x0, [x19, #FS_LOG_END]
    bl      render_apply
    bl      render_frame
    LDX     x0, frame_iov
    ldr     x9, [x19, #FS_PREFIX]
    ldr     x2, [x19, #FS_PREFIX_LEN]
    stp     x9, x2, [x0]
    LDX     x16, frame_len
    add     x2, x2, x16
    LDW     w1, grid_height
    add     w1, w1, #1
    LDX     x3, iov_scratch
    bl      writev_keep
    ldp     x19, x30, [sp], #16
    ret

// render_catch_up: replay the open log (ring entry 0's) and empty it, for a
// caller that renders on the main thread. Clobbers C but x19-x24.
render_catch_up:
    ADRG    x16, ring
    ldr     x1, [x16, #FS_LOG]
    adrp    x17, log_ptr
    ldr     x0, [x17, :lo12:log_ptr]
    str     x1, [x17, :lo12:log_ptr]
    b       render_apply

// pipeline_plan: before the effect is built, decide whether the frames will
// go to a render thread: output that is not paced on the real clock (and not
// the parity dump), at least two CPUs to run on, and no TTFX_ASM_THREADS=1
// (which keeps every run single-threaded, for testing). With one, visual
// changes are logged for the renderer (log_handles), which keeps a visual
// array of its own. Without, a visual change shows in the grid on the spot
// (handle_direct), and the renderer reads ch_handle itself: it only ever
// runs between frames. Clobbers C.
pipeline_plan:
    str     x30, [sp, #-16]!
    LDX     x9, ch_handle
    STX     x9, rs_handle
    LDX     x9, request
    ldr     x10, [x9, #RQ_DIRECT_EMIT]
    cbnz    x10, pipeline_plan.done     // borrowed grid is main-thread only
    LDB     w9, cfg_parity_dump
    cbnz    w9, pipeline_plan.done
    LDB     w9, clock_is_virtual
    cbnz    w9, pipeline_plan.unpaced
    LDX     x9, cfg_frame_rate
    cbnz    x9, pipeline_plan.done
pipeline_plan.unpaced:
    ADRG    x0, env_threads
    CCALL   getenv
    cbz     x9, pipeline_plan.cpus
    ldrh    w3, [x9]
    cmp     w3, #0x31                   // "1", NUL
    b.eq    pipeline_plan.done
pipeline_plan.cpus:
    // the CPUs this thread may run on: two or more (as far as it matters)
    sub     sp, sp, #128
    mov     x0, #0
    mov     x1, #128
    mov     x2, sp
    SYSCALL SYS_sched_getaffinity
    mov     w3, #0
    cmp     x9, #0
    b.le    pipeline_plan.counted
    lsr     x9, x9, #3
    mov     x2, #0
pipeline_plan.word:
    ldr     x4, [sp, x2, lsl #3]
    cbz     x4, pipeline_plan.skip
    add     w3, w3, #1
    sub     x5, x4, #1
    tst     x4, x5
    b.eq    pipeline_plan.skip
    add     w3, w3, #1                  // two or more in this word
pipeline_plan.skip:
    add     x2, x2, #1
    cmp     x2, x9
    b.lo    pipeline_plan.word
pipeline_plan.counted:
    add     sp, sp, #128
    cmp     w3, #2
    b.lo    pipeline_plan.done
    mov     w9, #1
    STB     w9, log_handles
    mov     x0, #(CHAR_LIMIT * 4)
    bl      reserve_small
    STX     x9, rs_handle
pipeline_plan.done:
    ldr     x30, [sp], #16
    ret

// pipeline_start: start the render thread planned for (pipeline_plan). Without
// it, frames are rendered on the main thread. Clobbers C.
pipeline_start:
    str     x30, [sp, #-16]!
    LDB     w9, log_handles
    cbz     w9, pipeline_start.done
    ADRG    x0, render_thread
    ADRG    x1, render_tid
    bl      thread_start
    cbnz    w9, pipeline_start.done     // no thread: render here
    mov     w9, #1
    STB     w9, pipe_running
pipeline_start.done:
    ldr     x30, [sp], #16
    ret

// pipeline_finish -> x9 = -errno of the first frame that failed to write, or
// 0. Every frame handed over is written (or, after a failure, dropped) and
// the render thread has ended. Safe to call when there is none. Clobbers C.
pipeline_finish:
    str     x30, [sp, #-16]!
    LDB     w9, pipe_running
    cbz     w9, pipeline_finish.done
    mov     w9, #1
    ADRG    x16, render_quit
    stlrb   w9, [x16]
    dmb     ish                         // the quit store before the flag's load
    ADRG    x3, render_sleeping
    R_XCHGB w9, x3, wzr
    cbz     w9, pipeline_finish.join
    ADRG    x0, render_seq
    R_ATOMIC_INC x0, w9
    bl      futex_wake
pipeline_finish.join:
    LDX     x0, render_tid
    bl      thread_join
    STB     wzr, pipe_running
pipeline_finish.done:
    LDX     x9, render_err
    ldr     x30, [sp], #16
    ret

// render_thread: the render thread's start routine (a C function). Renders
// the frames handed over, in order, until told to quit with none left. After
// a failed write it only drops frames (the run is ending).
render_thread:
    CENTRY
render_thread.loop:
    LDW     w9, r_completed
    ADRG    x17, r_submitted
    ldar    w2, [x17]
    cmp     w9, w2
    b.ne    render_thread.work
    ADRG    x16, render_quit
    ldarb   w2, [x16]
    cbnz    w2, render_thread.quitting
    mov     w19, #RENDER_SPIN
render_thread.spin:
    yield
    LDW     w9, r_completed
    ADRG    x17, r_submitted
    ldar    w2, [x17]
    cmp     w9, w2
    b.ne    render_thread.work
    ADRG    x16, render_quit
    ldarb   w2, [x16]
    cbnz    w2, render_thread.quitting
    subs    w19, w19, #1
    b.ne    render_thread.spin
    // sleep: the futex word first, then the flag, then a last look
    LDW     w20, render_seq
    mov     w9, #1
    ADRG    x3, render_sleeping
    R_XCHGB w2, x3, w9
    LDW     w9, r_completed
    ADRG    x17, r_submitted
    ldar    w2, [x17]
    cmp     w9, w2
    b.ne    render_thread.awake
    ADRG    x16, render_quit
    ldarb   w2, [x16]
    cbnz    w2, render_thread.awake
    ADRG    x0, render_seq
    mov     w1, w20
    bl      futex_wait
render_thread.awake:
    STB     wzr, render_sleeping
    b       render_thread.loop
render_thread.quitting:
    // quit comes after the last frame: one more look at the count
    LDW     w9, r_completed
    ADRG    x17, r_submitted
    ldar    w2, [x17]
    cmp     w9, w2
    b.ne    render_thread.work
    mov     x0, #0
    CEXIT
render_thread.work:
    and     w9, w9, #(FRAME_RING - 1)
    lsl     w9, w9, #FS_SHIFT
    ADRG    x0, ring
    add     x0, x0, x9
    LDX     x9, render_err
    cbnz    x9, render_thread.done
    bl      render_emit
    cbz     x9, render_thread.done
    STX     x9, render_err
render_thread.done:
    ADRG    x0, r_completed
    R_ATOMIC_INC x0, w9
    dmb     ish                         // the count before main_sleeping's load
    // a main thread waiting for room wakes when half the ring is free
    LDB     w9, main_sleeping
    cbz     w9, render_thread.loop
    LDW     w9, r_submitted
    LDW     w2, r_completed
    sub     w9, w9, w2
    cmp     w9, #(FRAME_RING / 2)
    b.hi    render_thread.loop
    ADRG    x3, main_sleeping
    R_XCHGB w9, x3, wzr
    cbz     w9, render_thread.loop
    ADRG    x0, main_seq
    R_ATOMIC_INC x0, w9
    bl      futex_wake
    b       render_thread.loop

    .section .rodata
env_threads:
    .asciz  "TTFX_ASM_THREADS"

    .text

    TSTATE
// Both threads read this block, which is fixed once render_init is done;
// it has its lines to itself, so neither thread's writes elsewhere evict it.
// (128: the cache line of the Apple cores.)
    .balign 128
grid_width:     .skip 8
grid_height:    .skip 8
grid_cells:     .skip 8
co_rbase:       .skip 8
co_rspan:       .skip 8
co_cbase:       .skip 8
co_cspan:       .skip 8
co_cell0:       .skip 8
handle_grid:    .skip 8
rs_link:        .skip 8
owner_grid:     .skip 8
rs_cell:        .skip 8
rs_handle:      .skip 8
cell_rec:       .skip 8
pending_cells:  .skip 8
out_base:       .skip 8
row_store:      .skip 8
row_stride:     .skip 8
row_offs:       .skip 8
offs_stride:    .skip 8
row_sel:        .skip 8
row_iov:        .skip 8
frame_iov:      .skip 8
iov_scratch:    .skip 8
dirty_chunks:   .skip 8
dirty_rows:     .skip 8
dirty_cells:    .skip 8
dirty_bits:     .skip 8
full_blocks:    .skip 8
row_blocks:     .skip 8
render_tid:     .skip 8
pipe_running:   .skip 1                 // frames go to the render thread
log_handles:    .skip 1                 // visual changes are logged for it
// what each thread writes as it goes, each on lines of its own
    .balign 128
log_ptr:        .skip 8                 // the main side's end of the open log
render_quit:    .skip 1
    .balign 128
frame_len:      .skip 8                 // the renderer's
render_err:     .skip 8                 // the renderer's first failed write
pending_count:  .skip 4
all_dirty:      .skip 1
    .balign 128
r_submitted:    .skip 4                 // frames handed over (main)
    .balign 128
r_completed:    .skip 4                 // frames done (renderer)
    .balign 128
render_seq:     .skip 4                 // the renderer's futex word
render_sleeping: .skip 1
    .balign 128
main_seq:       .skip 4                 // the main thread's futex word
main_sleeping:  .skip 1
    .balign 128
ring:           .skip FRAME_RING * FS_SIZE

    .text

.if (co_rbase - grid_width) != RB_CO_RBASE || (co_rspan - grid_width) != RB_CO_RSPAN
    .error "render block layout"
.endif
.if (co_cbase - grid_width) != RB_CO_CBASE || (co_cspan - grid_width) != RB_CO_CSPAN
    .error "render block layout"
.endif
.if (co_cell0 - grid_width) != RB_CO_CELL0
    .error "render block layout"
.endif
