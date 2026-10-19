// engine/visual.s - the visual pool (plan §7.5).
//
// A CharacterVisual is formatted once - SGR prefix, symbol, reset - and
// interned into a de-duplicated pool. A visual is then a u32 handle: the
// offset of its bytes (low 24 bits) and their length (high 8). The renderer
// never formats, allocates or refcounts; a cell is one bounded copy.
//
// Each pooled visual is preceded by a 32-byte header holding what the visual
// *is*, for effects that read it back (CharacterVisual.symbol, .colors):
//   -32 symbol (packed)   -24 fg color   -16 bg color   -8 attribute bits
// The colors are the logical ones even under --no-color, where the bytes
// carry none. The header is the interning key: it determines the bytes.
//
// Every pooled visual is readable for 128 bytes from its start (the pool keeps
// that much slack), so copies may overrun their length without leaving it.
//
// VISUAL_MAX, VISUAL_HEADER, POOL_RESERVE, VISUAL_TABLE_INITIAL, VH_* and
// ATTR_* are in defs.inc (dim is stored upstream but never emitted, so it
// has no bit).

    .text

.equ VIS_SPACE_SYMBOL, (1 << 32) | 0x20     // ' ', one byte

// visual_init: reserve the pool and table, and make the blank cell.
visual_init:
    PUSH1   x30
    MOV64   x0, POOL_RESERVE
    bl      reserve
    STX     x9, pool_base
    mov     w3, #VISUAL_TABLE_INITIAL
    bl      visual_table_alloc
    mov     x0, #NONE
    mov     x1, #NONE
    MOV64   x2, VIS_SPACE_SYMBOL
    mov     w3, #0
    bl      visual_make
    STW     w9, space_handle
    POP1    x30
    ret

// visual_table_alloc(w3=capacity): fresh zeroed table of w3 entries. An
// entry is the handle in the low half and its hash in the high half (0 =
// empty; handles are never 0), so probes and growth rarely touch the pool.
visual_table_alloc:
    PUSH2   x3, x30
    ubfiz   x0, x3, #3, #32
    bl      alloc
    POP2    x3, x30
    STX     x9, table_base
    sub     w3, w3, #1
    STW     w3, table_mask
    ret

// VISUAL_HASH sym, fg, bg, attrs, out, tmp: a 32-bit hash of a visual's
// header (four 64-bit registers, left intact) into the 64-bit register out.
// Only the table's probe order depends on it, never a handle, so any mix
// that is consistent within one build will do.
.macro VISUAL_HASH sym, fg, bg, attrs, out, tmp
    MOV64   \tmp, 0x9e3779b97f4a7c15
    mul     \out, \fg, \tmp
    eor     \out, \out, \sym
    ror     \out, \out, #29
    add     \out, \out, \bg
    mul     \out, \out, \tmp
    eor     \out, \out, \attrs
    ror     \out, \out, #31
    mul     \out, \out, \tmp
    lsr     \out, \out, #32
.endm

// visual_make(x0=fg color or NONE, x1=bg color or NONE, x2=packed symbol,
//             w3=ATTR_* bits) -> w9 = handle.
// CharacterVisual::new + format_symbol_into: bold, italic, underline, blink,
// reverse, hidden, strike, fg, bg, symbol, then a reset only if anything
// preceded it. Colors are dropped from the bytes under --no-color
// (resolve_color_code) but kept in the header. Under --xterm-colors a color
// renders as its own code when it has one, else the nearest by hex_to_xterm.
//
// The bytes are a function of the header (the color flags are fixed for a
// run), so the header alone is the interning key: a visual seen before is
// found without formatting it, and a new one is formatted straight into
// the pool. Handles are pool offsets in first-seen order, as before.
// Clobbers x9, x3, x2, x1, x0, x4, x5, x10, x11 (and what hex_to_xterm
// does when it runs).
visual_make:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     x21, x0
    mov     x22, x1
    mov     x23, x2
    mov     w24, w3
    // Direct frames apply the current transform at export, after all scene
    // caches have resolved. Keep their interned metadata authored, so a quiet
    // frame also restores colors instead of retaining an old audio transform.
    LDX     x9, request
    cbz     x9, visual_make.audio_color   // utility tests may have no run request
    ldr     x10, [x9, #RQ_DIRECT_EMIT]
    cbnz    x10, visual_make.tinted
visual_make.audio_color:
    // The frame's audio color transform retints new visuals when the params
    // callback flagged it active (never under --no-color, where colors emit
    // no bytes). The retinted color is the interning key too, so a changing
    // transform makes new visuals and a steady one keeps deduplicating.
    LDB     w9, audio_color_on
    cbz     w9, visual_make.tinted
    LDB     w9, cfg_no_color
    cbnz    w9, visual_make.tinted
    mov     x0, x21
    bl      audio_tint_maybe
    mov     x21, x0
    mov     x0, x22
    bl      audio_tint_maybe
    mov     x22, x0
visual_make.tinted:
    VISUAL_HASH x23, x21, x22, x24, x20, x9
    LDX     x10, table_base
    LDX     x11, pool_base
    LDW     w5, table_mask
    lsl     x4, x20, #32                // the entry's tag
    mov     w2, w20                     // probe index
visual_make.next:
    and     w2, w2, w5
    ldr     x9, [x10, x2, lsl #3]
    cbz     x9, visual_make.new
    eor     x3, x9, x4
    lsr     x3, x3, #32
    cbnz    x3, visual_make.skip        // another hash
    and     x3, x9, #HANDLE_OFFSET_MASK
    add     x3, x11, x3
    ldur    x16, [x3, #VH_SYMBOL]
    cmp     x23, x16
    b.ne    visual_make.skip
    ldur    x16, [x3, #VH_FG]
    cmp     x21, x16
    b.ne    visual_make.skip
    ldur    x16, [x3, #VH_BG]
    cmp     x22, x16
    b.ne    visual_make.skip
    ldur    x16, [x3, #VH_ATTRS]
    cmp     x24, x16
    b.eq    visual_make.found
visual_make.skip:
    add     w2, w2, #1
    b       visual_make.next
visual_make.found:
    mov     w9, w9
    b       visual_make.done
visual_make.new:
    lsl     x20, x20, #32
    orr     x20, x20, x2                // hash << 32 | slot
    // format at the pool's end: the header, then the bytes. POOL_RESERVE's
    // slack past POOL_LIMIT covers a visual written before the limit check.
    LDW     w3, pool_len
    add     x19, x11, x3
    stp     x23, x21, [x19]
    stp     x22, x24, [x19, #16]
    add     x19, x19, #VISUAL_HEADER    // write cursor
    LDB     w9, cfg_no_color
    cbz     w9, visual_make.attrs
    mov     x21, #NONE
    mov     x22, #NONE
visual_make.attrs:
    cbz     w24, visual_make.colors
    ADRG    x4, sgr_attr_codes
    mov     w5, #0
    movz    w6, #0x5b1b                 // "\033[0m", digit patched below
    movk    w6, #0x6d30, lsl #16
visual_make.attr:
    lsr     w7, w24, w5
    tbz     w7, #0, visual_make.attr_next
    str     w6, [x19]
    ldrb    w9, [x4, x5]
    strb    w9, [x19, #2]
    add     x19, x19, #4
visual_make.attr_next:
    add     w5, w5, #1
    cmp     w5, #7
    b.lo    visual_make.attr
visual_make.colors:
    cmn     x21, #1
    b.eq    visual_make.no_fg
    mov     x0, x21
    mov     w1, #0x33                   // '3'
    bl      sgr_color
visual_make.no_fg:
    cmn     x22, #1
    b.eq    visual_make.no_bg
    mov     x0, x22
    mov     w1, #0x34                   // '4'
    bl      sgr_color
visual_make.no_bg:
    ubfx    x3, x23, #32, #8            // symbol byte length
    str     w23, [x19]
    LDW     w2, pool_len
    LDX     x9, pool_base
    add     x2, x2, x9
    add     x2, x2, #VISUAL_HEADER      // the bytes' start
    sub     x0, x19, x2                 // prefix length
    add     x19, x19, x3
    cbz     x0, visual_make.plain
    movz    w9, #0x5b1b                 // "\033[0m"
    movk    w9, #0x6d30, lsl #16
    str     w9, [x19]
    add     x19, x19, #4
visual_make.plain:
    str     wzr, [x19]                  // clear symbol bytes past its length
    sub     x1, x19, x2                 // byte length
    cmp     w1, #VISUAL_MAX
    b.hi    visual_make.too_long
    LDW     w3, pool_len
    add     w9, w3, #VISUAL_HEADER
    add     w3, w3, w1
    add     w3, w3, #VISUAL_HEADER
    MOV64   x12, POOL_LIMIT
    cmp     w3, w12
    b.hs    visual_make.full
    STW     w3, pool_len
    lsl     w1, w1, #HANDLE_LEN_SHIFT
    orr     w9, w9, w1
    lsr     x3, x20, #32
    lsl     x3, x3, #32
    orr     x3, x3, x9
    LDX     x2, table_base
    mov     w1, w20
    str     x3, [x2, x1, lsl #3]
    LDW     w3, table_count
    add     w3, w3, #1
    STW     w3, table_count
    add     w3, w3, w3
    LDW     w1, table_mask
    cmp     w3, w1
    b.ls    visual_make.done
    PUSH1   x9
    bl      visual_table_grow
    POP1    x9
visual_make.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret
visual_make.too_long:
    ADRG    x0, msg_visual_long
    mov     w1, #msg_visual_long_len
    b       fatal
visual_make.full:
    ADRG    x0, msg_pool_full
    mov     w1, #msg_pool_full_len
    b       fatal

// visual_table_grow: quadruple the table and reinsert every entry at its
// stored hash (four times, not two: effects that make many visuals make
// hundreds of thousands, and each growth rehashes them all).
// Clobbers x9, x3, x2, x1, x0, x4, x5, x10, x11.
visual_table_grow:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    str     x30, [sp, #32]
    LDX     x21, table_base
    LDW     w22, table_mask
    add     w22, w22, #1                // old capacity
    lsl     w3, w22, #2
    bl      visual_table_alloc
    LDX     x23, table_base
    LDW     w10, table_mask
    mov     w19, #0
visual_table_grow.each:
    cmp     w19, w22
    b.hs    visual_table_grow.finish
    ldr     x11, [x21, x19, lsl #3]
    cbz     x11, visual_table_grow.skip
    lsr     x9, x11, #32
visual_table_grow.probe:
    and     w9, w9, w10
    ldr     x16, [x23, x9, lsl #3]
    cbz     x16, visual_table_grow.put
    add     w9, w9, #1
    b       visual_table_grow.probe
visual_table_grow.put:
    str     x11, [x23, x9, lsl #3]
visual_table_grow.skip:
    add     w19, w19, #1
    b       visual_table_grow.each
visual_table_grow.finish:
    ldr     x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// visual_run(x0=symbol, x1=fg colors, x2=count, x3=bg color or NONE,
//            x4=key) -> x9 = pointer to count u32 handles, w2 = count:
// visual_make of the symbol in each fg color over bg, no attributes - the
// frames of a per-character gradient. Memoized by (symbol, key, bg): callers
// give equal keys only for equal color lists (typically the color or colors
// the list was made from), so characters sharing a symbol and colors skip
// visual_make, and with visual_run_find even making the list. The handles
// stay valid for the run. Clobbers x9, x3, x2, x1, x0, x4, x5, x10, x11 (and
// what visual_make does).
// VFRAMES_MAX (frames visual_frames appends at once) and VRUN_INITIAL
// (32-byte entries: symbol, key, bg, handles) are in defs.inc.
visual_run:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56]: the entry, then the handles
    mov     x21, x0                     // symbol
    mov     x22, x1                     // colors
    mov     x23, x2                     // count
    mov     x24, x3                     // bg
    mov     x20, x4                     // key
    mov     x1, x4
    mov     x2, x3
    bl      visual_run_find
    cbnz    x9, visual_run.done
    // make the handles (after their count) and keep them
    str     x3, [sp, #56]               // the empty entry
    lsl     x0, x23, #2
    add     x0, x0, #4
    bl      alloc
    str     w23, [x9]
    add     x9, x9, #4
    ldr     x2, [sp, #56]
    str     x9, [sp, #56]
    stp     x21, x20, [x2]
    stp     x24, x9, [x2, #16]
    mov     x19, #0
visual_run.each:
    cmp     x19, x23
    b.hs    visual_run.made
    ldr     x0, [x22, x19, lsl #3]
    mov     x1, x24
    mov     x2, x21
    mov     w3, #0
    bl      visual_make
    ldr     x3, [sp, #56]
    str     w9, [x3, x19, lsl #2]
    add     x19, x19, #1
    b       visual_run.each
visual_run.made:
    // at half load, double the table
    LDW     w9, vrun_count
    add     w9, w9, #1
    STW     w9, vrun_count
    add     w9, w9, w9
    LDW     w3, vrun_mask
    cmp     w9, w3
    b.ls    visual_run.kept
    bl      visual_run_grow
visual_run.kept:
    ldr     x9, [sp, #56]
    mov     w2, w23
visual_run.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// visual_frames(w0=scene, x1=u32 handles, w2=count, w3=duration): a
// frame of each visual - scene_add_frame_visual for each, appended in one
// go when nothing needs checking (a positive duration, no preexisting
// colors to apply) - since scene_add_frame_visual would only append.
// Clobbers C except x19-x24.
visual_frames:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    sub     sp, sp, #(VFRAMES_MAX * FRAME_SIZE)
    mov     w21, w0
    mov     x22, x1
    mov     w23, w2
    mov     w24, w3
    cmp     w3, #1
    b.lt    visual_frames.each
    cmp     w2, #VFRAMES_MAX
    b.hi    visual_frames.each
    SCENE_PTR x9, x0
    ldr     w9, [x9, #SC_FLAGS]
    tst     w9, #(SCF_PREEXISTING | SCF_PRE_BOLD)
    b.ne    visual_frames.each
    // the frame records: handle, duration
    lsl     x2, x24, #32
    mov     w3, #0
visual_frames.record:
    cmp     w3, w23
    b.hs    visual_frames.append
    ldr     w9, [x22, x3, lsl #2]
    orr     x9, x9, x2
    str     x9, [sp, x3, lsl #3]
    add     w3, w3, #1
    b       visual_frames.record
visual_frames.append:
    mov     w0, w21
    mov     x1, sp
    mov     w2, w23
    bl      scene_append_frames
    b       visual_frames.done
visual_frames.each:
    mov     w19, #0
visual_frames.frame:
    cmp     w19, w23
    b.hs    visual_frames.done
    mov     w0, w21
    ldr     w1, [x22, x19, lsl #2]
    mov     w2, w24
    bl      scene_add_frame_visual
    add     w19, w19, #1
    b       visual_frames.frame
visual_frames.done:
    add     sp, sp, #(VFRAMES_MAX * FRAME_SIZE)
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// VRUN_HASH symbol, key, bg, out, tmp: visual_run's table index (before the
// mask) in the 32-bit register out's 64-bit form.
.macro VRUN_HASH symbol, key, bg, out, tmp
    MOV64   \tmp, 0x9e3779b97f4a7c15
    mul     \out, \key, \tmp
    eor     \out, \out, \symbol
    ror     \out, \out, #29
    add     \out, \out, \bg
    mul     \out, \out, \tmp
    lsr     \out, \out, #32
.endm

// visual_run_find(x0=symbol, x1=key, x2=bg) -> x9 = visual_run's handles
// for these, w2 = their count; or x9 = 0 and x3 = the empty memo entry.
// Clobbers x9, x3, x2, x0, x4, x5, x10, x11.
visual_run_find:
    LDX     x4, vrun_table
    cbnz    x4, visual_run_find.hash
    stp     x0, x1, [sp, #-32]!
    stp     x2, x30, [sp, #16]
    mov     x0, #(VRUN_INITIAL * 32)
    bl      alloc
    STX     x9, vrun_table
    mov     w3, #(VRUN_INITIAL - 1)
    STW     w3, vrun_mask
    mov     x4, x9
    ldp     x2, x30, [sp, #16]
    ldp     x0, x1, [sp], #32
visual_run_find.hash:
    VRUN_HASH x0, x1, x2, x3, x5
    LDW     w5, vrun_mask
visual_run_find.probe:
    and     w3, w3, w5
    add     x10, x4, x3, lsl #5         // the entry
    ldr     x9, [x10, #24]
    cbz     x9, visual_run_find.miss
    ldr     x16, [x10]
    cmp     x16, x0
    b.ne    visual_run_find.next
    ldr     x16, [x10, #8]
    cmp     x16, x1
    b.ne    visual_run_find.next
    ldr     x16, [x10, #16]
    cmp     x16, x2
    b.ne    visual_run_find.next
    ldur    w2, [x9, #-4]
    ret
visual_run_find.next:
    add     w3, w3, #1
    b       visual_run_find.probe
visual_run_find.miss:
    mov     x3, x10
    ret

// visual_run_keep(x3=the empty entry visual_run_find gave, x0=symbol,
// x1=key, x2=bg, x4=pointer): keep a caller's own pointer (after a u32
// count, as visual_run's handles) under the key, for effects memoizing
// something else by (symbol, key, bg) - a template scene, say. Keys must
// not collide with the effect's visual_run keys. Clobbers x9, x3, x2, x1,
// x0, x4, x5, x10, x11 and v0-v1.
visual_run_keep:
    stp     x0, x1, [x3]
    stp     x2, x4, [x3, #16]
    LDW     w9, vrun_count
    add     w9, w9, #1
    STW     w9, vrun_count
    add     w9, w9, w9
    LDW     w3, vrun_mask
    cmp     w9, w3
    b.ls    visual_run_keep.kept
    b       visual_run_grow
visual_run_keep.kept:
    ret

// visual_run_grow: double visual_run's table. Clobbers x9, x3, x2, x1, x0,
// x4, x5, x10, x11, v0-v1.
visual_run_grow:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    LDX     x21, vrun_table
    LDW     w22, vrun_mask
    add     w22, w22, #1                // old capacity
    lsl     x0, x22, #6                 // twice the entries, 32 bytes each
    bl      alloc
    STX     x9, vrun_table
    lsl     w3, w22, #1
    sub     w3, w3, #1
    STW     w3, vrun_mask
    mov     x4, x9
    mov     w5, w3
    mov     w19, #0
visual_run_grow.each:
    cmp     w19, w22
    b.hs    visual_run_grow.done
    add     x1, x21, x19, lsl #5
    ldr     x16, [x1, #24]
    cbz     x16, visual_run_grow.skip
    ldp     x12, x13, [x1]
    ldr     x14, [x1, #16]
    VRUN_HASH x12, x13, x14, x3, x10
visual_run_grow.probe:
    and     w3, w3, w5
    add     x0, x4, x3, lsl #5
    ldr     x16, [x0, #24]
    cbz     x16, visual_run_grow.put
    add     w3, w3, #1
    b       visual_run_grow.probe
visual_run_grow.put:
    ldp     q0, q1, [x1]
    stp     q0, q1, [x0]
visual_run_grow.skip:
    add     w19, w19, #1
    b       visual_run_grow.each
visual_run_grow.done:
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// visual_meta(w9=handle) -> x9 = pointer to the visual's bytes; the header
// fields are at negative offsets (VH_*). Clobbers x16 besides.
visual_meta:
    and     w9, w9, #HANDLE_OFFSET_MASK
    LDX     x16, pool_base
    add     x9, x9, x16
    ret

// audio_tint_maybe(x0=color or NONE) -> x0: audio_tint unless there is no
// color, or the color carries an xterm code of its own (it then renders as
// that code, which the fx engine's recolor leaves alone too).
audio_tint_maybe:
    cmn     x0, #1                      // NONE
    b.eq    audio_tint_maybe.keep
    tbz     x0, #COLOR_XTERM_BIT, audio_tint
audio_tint_maybe.keep:
    ret

// audio_tint(x0=color) -> w0 = tinted RGB (upper 32 bits zeroed): this
// frame's audio color transform, (matrix . rgb) * brightness in f32 with
// each channel clamped to 0-255 and truncated - the fx engine's
// utils/ansi.rs recolor. The parameters were sampled into audio_params by
// audio_frame_poll. Same left-to-right f32 operations as the x86 SSE2
// audio_tint, no fused operations; fmaxnm(value, 0) selects 0 for NaN like
// maxss(value, 0). The three matrix rows are multiplied as NEON vectors —
// lane-wise fmul on the same operands is bit-identical to the scalar form —
// while every addition, clamp, and conversion keeps the scalar order of the
// reference. Clobbers x3-x5, x12-x16, s0-s7, s16, v1-v6 (the x86 port's
// rax/rcx/rdx/r8: map entries and export headers in x10/x11, and the
// original color in x2, stay live across this call, as r10/r11/rsi do on
// x86).
audio_tint:
    ubfx    w12, w0, #16, #8
    ubfx    w13, w0, #8, #8
    and     w14, w0, #0xff
    ucvtf   s0, w12                     // v0 = {r, 0, 0, 0}
    ucvtf   s1, w13                     // g
    ucvtf   s2, w14                     // b
    mov     v0.s[1], v1.s[0]
    mov     v0.s[2], v2.s[0]            // v0 = {r, g, b, 0}
    ADRG    x16, audio_matrix
    ldr     q1, [x16]                   // row 0 (+ first coeff of row 1)
    add     x16, x16, #12
    ldr     q2, [x16]                   // row 1 (+ first coeff of row 2)
    add     x16, x16, #12
    ldr     q3, [x16]                   // row 2 (+ audio_bright)
    fmul    v1.4s, v1.4s, v0.4s
    fmul    v2.4s, v2.4s, v0.4s
    fmul    v3.4s, v3.4s, v0.4s         // lane 3 is coeff * 0: unused
    ADRG    x16, audio_bright
    ldr     s16, [x16]
    fmov    s6, wzr                     // 0.0
    movz    w12, #0x437f, lsl #16       // 255.0f32
    fmov    s7, w12
    // each row: exact reference order ((m0*r + m1*g) + m2*b) * bright
    mov     s4, v1.s[0]
    mov     s5, v1.s[1]
    fadd    s4, s4, s5
    mov     s5, v1.s[2]
    fadd    s4, s4, s5
    fmul    s4, s4, s16
    fmaxnm  s4, s4, s6
    fminnm  s4, s4, s7
    fcvtzu  w3, s4
    mov     s4, v2.s[0]
    mov     s5, v2.s[1]
    fadd    s4, s4, s5
    mov     s5, v2.s[2]
    fadd    s4, s4, s5
    fmul    s4, s4, s16
    fmaxnm  s4, s4, s6
    fminnm  s4, s4, s7
    fcvtzu  w4, s4
    mov     s4, v3.s[0]
    mov     s5, v3.s[1]
    fadd    s4, s4, s5
    mov     s5, v3.s[2]
    fadd    s4, s4, s5
    fmul    s4, s4, s16
    fmaxnm  s4, s4, s6
    fminnm  s4, s4, s7
    fcvtzu  w5, s4
    lsl     w0, w3, #16
    orr     w0, w0, w4, lsl #8
    orr     w0, w0, w5
    ret

// SGR_CHANNEL: append the decimal digits of w3 (0-255) at x19, from the
// table at x4. Clobbers x9.
.macro SGR_CHANNEL
    ldr     w9, [x4, x3, lsl #2]
    str     w9, [x19]
    lsr     w9, w9, #24
    add     x19, x19, x9
.endm

// sgr_color(x0=color, w1='3' fg or '4' bg): append the SGR sequence at x19:
// "\033[38;2;R;G;Bm", or "\033[38;5;Nm" under --xterm-colors.
sgr_color:
    LDB     w9, cfg_xterm_colors
    cbz     w9, sgr_rgb
    stp     x1, x30, [sp, #-32]!
    tbz     x0, #COLOR_XTERM_BIT, sgr_color.nearest
    ubfx    x9, x0, #32, #8
    b       sgr_color.code
sgr_color.nearest:
    str     x19, [sp, #16]
    bl      hex_to_xterm
    ldr     x19, [sp, #16]
sgr_color.code:
    ldp     x1, x30, [sp], #32
    movz    w16, #0x5b1b                // "\033["
    strh    w16, [x19]
    strb    w1, [x19, #2]
    movz    w16, #0x3b38                // "8;5;"
    movk    w16, #0x3b35, lsl #16
    stur    w16, [x19, #3]
    add     x19, x19, #7
    ADRG    x4, dec3_table
    mov     w3, w9
    SGR_CHANNEL
    mov     w16, #0x6d                  // 'm'
    strb    w16, [x19]
    add     x19, x19, #1
    ret

// sgr_rgb(w0=rgb, w1='3' fg or '4' bg): append "\033[38;2;R;G;Bm" at x19.
// Clobbers x9, x3, x4.
sgr_rgb:
    movz    w16, #0x5b1b                // "\033["
    strh    w16, [x19]
    strb    w1, [x19, #2]
    movz    w16, #0x3b38                // "8;2;"
    movk    w16, #0x3b32, lsl #16
    stur    w16, [x19, #3]
    add     x19, x19, #7
    ADRG    x4, dec3_table
    ubfx    w3, w0, #16, #8
    SGR_CHANNEL
    mov     w16, #0x3b                  // ';'
    strb    w16, [x19]
    add     x19, x19, #1
    ubfx    w3, w0, #8, #8
    SGR_CHANNEL
    mov     w16, #0x3b                  // ';'
    strb    w16, [x19]
    add     x19, x19, #1
    and     w3, w0, #0xff
    SGR_CHANNEL
    mov     w16, #0x6d                  // 'm'
    strb    w16, [x19]
    add     x19, x19, #1
    ret

    .section .rodata
sgr_attr_codes:
    .ascii  "1345789"

// dec3_table[n]: the decimal digits of n (1-3 bytes) with the count in byte 3.
    .balign 4
dec3_table:
    .set    vis_dec_n, 0
    .rept   256
    .if vis_dec_n >= 100
    .byte   48 + vis_dec_n / 100, 48 + (vis_dec_n / 10) % 10, 48 + vis_dec_n % 10, 3
    .elseif vis_dec_n >= 10
    .byte   48 + vis_dec_n / 10, 48 + vis_dec_n % 10, 0, 2
    .else
    .byte   48 + vis_dec_n, 0, 0, 1
    .endif
    .set    vis_dec_n, vis_dec_n + 1
    .endr

STRING msg_pool_full, "ttfx: asm engine: visual pool exhausted\n"
STRING msg_visual_long, "ttfx: asm engine: visual exceeds 128 bytes\n"

    TSTATE
    .balign 8
pool_base:      .skip 8
pool_len:       .skip 4
space_handle:   .skip 4
table_base:     .skip 8
table_mask:     .skip 4
table_count:    .skip 4
vrun_table:     .skip 8             // visual_run's memo
vrun_count:     .skip 4
vrun_mask:      .skip 4
