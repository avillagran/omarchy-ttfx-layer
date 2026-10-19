// audio_export.s - current-frame audio for typed direct output, the aarch64
// port of asm/engine/audio_export.asm. Scene and visual caches keep authored
// colors. The synchronous consumer borrows a reusable export pool; source
// visuals, scene handles and renderer state are never modified. Inactive
// audio and --no-color borrow the original grid/pool byte-for-byte. The
// scalar audio_tint is shared with the terminal engine (engine/visual.s).

    .text

// visual_export_audio -> x3=borrowed grid, x4=borrowed pool.
// Preserves the callee-saved registers. No C calls and no per-frame mmap
// after the first active frame.
visual_export_audio:
    STX     xzr, aexp_active
    STX     xzr, aexp_unique
    STX     xzr, aexp_changed
    LDX     x3, handle_grid
    LDX     x4, pool_base
    LDB     w9, audio_color_on
    cbz     w9, visual_export_audio.raw
    LDB     w9, cfg_no_color
    cbnz    w9, visual_export_audio.raw
    mov     x9, #1
    STX     x9, aexp_active
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x9, aexp_pool
    cbnz    x9, visual_export_audio.ready
    // first active frame: reusable export grid, handle map and export pool
    LDX     x0, grid_cells
    lsl     x0, x0, #2
    add     x0, x0, #64                 // borrowed grid's readable slack
    bl      alloc
    STX     x9, aexp_grid
    // At most one mapping per occupied grid cell. Load <= 1/2 guarantees
    // bounded probing, including the reserved blank, without resize.
    LDX     x3, grid_cells
    add     x3, x3, #1
    add     x3, x3, x3
    mov     x0, #1
visual_export_audio.capacity:
    cmp     x0, x3
    b.hs    visual_export_audio.allocate_map
    lsl     x0, x0, #1
    b       visual_export_audio.capacity
visual_export_audio.allocate_map:
    sub     x9, x0, #1
    STX     x9, aexp_map_mask
    lsl     x0, x0, #4                  // epoch u64, source u32, export u32
    bl      alloc
    STX     x9, aexp_map
    MOV64   x0, POOL_RESERVE
    bl      reserve
    STX     x9, aexp_pool
visual_export_audio.ready:
    LDX     x9, aexp_epoch
    add     x9, x9, #1
    STX     x9, aexp_epoch
    // Preserve the canonical blank at offset 32 for bridge fast-skip. Its
    // absent fg/bg remains absent: audio cannot create a visible empty cell.
    LDX     x1, pool_base
    LDX     x0, aexp_pool
    mov     w3, #33
    REP_MOVSB
    mov     x19, #33                    // next export header offset
    LDX     x20, pool_base
    LDX     x21, handle_grid
    LDX     x22, aexp_grid
    mov     x23, #0                     // cell index
visual_export_audio.cell:
    LDX     x9, grid_cells
    cmp     x23, x9
    b.hs    visual_export_audio.done
    ldr     w24, [x21, x23, lsl #2]     // source handle
    cbz     w24, visual_export_audio.store
    LDW     w9, space_handle
    cmp     w24, w9
    b.eq    visual_export_audio.store
    movz    w5, #0x79b9
    movk    w5, #0x9e37, lsl #16
    mul     w5, w24, w5                 // probe hash
visual_export_audio.probe:
    LDX     x9, aexp_map_mask
    and     x5, x5, x9
    LDX     x10, aexp_map
    add     x10, x10, x5, lsl #4        // map entry
    LDX     x9, aexp_epoch
    ldr     x16, [x10]
    cmp     x16, x9
    b.ne    visual_export_audio.new
    ldr     w16, [x10, #8]
    cmp     w16, w24
    b.eq    visual_export_audio.found
    add     x5, x5, #1
    b       visual_export_audio.probe
visual_export_audio.new:
    str     x9, [x10]                   // epoch
    str     w24, [x10, #8]              // source handle
    lsr     w3, w24, #24                // bounded visual byte length
    add     x3, x3, #VISUAL_HEADER
    add     x9, x19, x3
    MOV64   x16, POOL_LIMIT
    cmp     x9, x16
    b.hs    visual_export_audio.full    // never publish an out-of-range handle
    and     w1, w24, #HANDLE_OFFSET_MASK
    add     x1, x20, x1
    sub     x1, x1, #VISUAL_HEADER      // source header
    LDX     x11, aexp_pool
    add     x11, x11, x19               // export header for this unique visual
    mov     x0, x11
    add     x19, x19, x3
    // 25-36 bytes; copy 36 fixed, the pool's documented 128-byte read slack
    // covers the source, the reservation covers the destination, and the
    // next visual's copy overwrites this one's slack. The byte tail of the
    // generic REP_MOVSB macro is the worst case for these short copies.
    ldr     q0, [x1]
    ldr     q1, [x1, #16]
    ldr     w16, [x1, #32]
    str     q0, [x0]
    str     q1, [x0, #16]
    str     w16, [x0, #32]              // preserves symbol, attrs, visual bytes
    // True-color direct output uses the RGB of xterm-authored colors too;
    // NONE bg stays absent. Default fg matches direct bridge's light gray.
    ldr     x0, [x11, #8]
    cmn     x0, #1                      // NONE
    b.eq    visual_export_audio.fg_default
    b       visual_export_audio.fg_tint
visual_export_audio.fg_default:
    movz    w0, #0xcccc
    movk    w0, #0x00cc, lsl #16        // 0xcccccc
visual_export_audio.fg_tint:
    mov     x2, x0
    bl      audio_tint
    cmp     w0, w2
    cset    w9, ne
    STB     w9, aexp_different          // STB's address scratch is x16
    str     x0, [x11, #8]
    ldr     x0, [x11, #16]
    cmn     x0, #1                      // NONE bg stays absent
    b.eq    visual_export_audio.mapped
    mov     x2, x0
    bl      audio_tint
    cmp     w0, w2
    cset    w9, ne
    LDB     w17, aexp_different
    orr     w9, w9, w17
    STB     w9, aexp_different
    str     x0, [x11, #16]
visual_export_audio.mapped:
    LDX     x9, aexp_unique
    add     x9, x9, #1
    STX     x9, aexp_unique
    LDB     w9, aexp_different
    LDX     x17, aexp_changed
    add     x17, x17, x9
    STX     x17, aexp_changed
    LDX     x9, aexp_pool
    sub     x9, x11, x9
    add     w9, w9, #VISUAL_HEADER
    and     w16, w24, #0xff000000
    orr     w9, w9, w16
    str     w9, [x10, #12]
    b       visual_export_audio.store
visual_export_audio.found:
    ldr     w9, [x10, #12]
visual_export_audio.store:
    str     w9, [x22, x23, lsl #2]
    add     x23, x23, #1
    b       visual_export_audio.cell
visual_export_audio.done:
    mov     x3, x22
    LDX     x4, aexp_pool
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
visual_export_audio.raw:
    ret
visual_export_audio.full:
    FAIL    msg_pool_full

// Read-only observation of the last export, only inside its synchronous
// callback. No audio samples or process memory inspection: three numeric
// counters at x0: active, unique visuals transformed, RGB headers changed.
EXPORT ttfx_asm_audio_export_stats
ttfx_asm_audio_export_stats:
    LDX     x9, aexp_active
    str     x9, [x0]
    LDX     x9, aexp_unique
    str     x9, [x0, #8]
    LDX     x9, aexp_changed
    str     x9, [x0, #16]
    ret

    TSTATE
    .balign 8
aexp_active:    .skip 8
aexp_unique:    .skip 8
aexp_changed:   .skip 8
aexp_different: .skip 1
    .balign 8
aexp_grid:      .skip 8
aexp_pool:      .skip 8
aexp_map:       .skip 8
aexp_map_mask:  .skip 8
aexp_epoch:     .skip 8
