// utils/graphics.s - Gradient generation, fraction lookup and coordinate
// mappings (src/utils/graphics.rs). Colors use the u64 format of ttfx.inc;
// generated colors are plain RGB, stops keep their xterm codes.
//
// Gradients are NOT float lerps: channel deltas use Python floor division and
// the exact end stop is appended per pair (plan.md §5.2).

    .text

// gradient_new(x0=stops (u64 colors), w1=stop count, x2=steps (i64),
//              w3=step count, x4=spectrum out) -> w9 = spectrum length.
// Gradient::new with do_loop = false. Step values reaching here are >= 1
// (the CLI validates them), so the per-pair error path cannot trigger.
// Clobbers x0-x5, x10-x13.
gradient_new:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    mov     x21, x0
    mov     w22, w1
    mov     x23, x2
    mov     w24, w3
    mov     x20, x4
    mov     w19, #0                     // spectrum length
    cmp     w22, #1
    b.ne    gradient_new.pairs
    // one stop: steps[0] copies of it
    ldr     x3, [x23]
    ldr     x9, [x21]
gradient_new.single:
    cmp     x3, #0
    b.le    gradient_new.done
    str     x9, [x20, x19, lsl #3]
    add     w19, w19, #1
    sub     x3, x3, #1
    b       gradient_new.single
gradient_new.pairs:
    mov     w5, #0                      // pair index
gradient_new.pair:
    sub     w9, w22, #1
    cmp     w5, w9
    b.hs    gradient_new.done
    // step count: steps[min(pair, count - 1)]
    sub     w9, w24, #1
    cmp     w5, w9
    csel    w9, w5, w9, lo
    ldr     x10, [x23, x9, lsl #3]      // step_count
    add     x11, x21, x5, lsl #3
    ldr     w9, [x11]                   // start (RGB bits)
    ldr     x11, [x11, #8]              // end (the whole color)
    // per-channel start at [sp + 8k] and floor-divided delta at
    // [sp + 24 + 8k], k = 0, 1, 2 for shifts 0, 8, 16
    sub     sp, sp, #48
    mov     w3, #0
gradient_new.channel:
    lsl     w12, w3, #3
    lsr     w2, w9, w12
    and     w2, w2, #0xff               // start channel
    str     x2, [sp, x3, lsl #3]
    and     w0, w11, #0xffffff
    lsr     w0, w0, w12
    and     w0, w0, #0xff
    sub     x0, x0, x2                  // end - start
    sdiv    x12, x0, x10
    // floor: adjust when the remainder is nonzero and signs differ
    msub    x13, x12, x10, x0
    cbz     x13, gradient_new.exact
    eor     x13, x13, x10
    tbz     x13, #63, gradient_new.exact
    sub     x12, x12, #1
gradient_new.exact:
    add     x13, sp, #24
    str     x12, [x13, x3, lsl #3]
    add     w3, w3, #1
    cmp     w3, #3
    b.lo    gradient_new.channel
    // i from (spectrum non-empty ? 1 : 0) up to step_count - 1
    cmp     w19, #0
    cset    x1, ne
    mov     x4, #255
gradient_new.step:
    cmp     x1, x10
    b.ge    gradient_new.pair_end
    // channel k: clamp(start + delta * i, 0, 255) << 8k
    ldr     x9, [sp, #40]
    mul     x9, x9, x1
    ldr     x2, [sp, #16]
    add     x9, x9, x2
    cmp     x9, #0
    csel    x9, xzr, x9, lt
    cmp     x9, x4
    csel    x9, x4, x9, gt
    mov     w0, w9
    ldr     x9, [sp, #32]
    mul     x9, x9, x1
    ldr     x2, [sp, #8]
    add     x9, x9, x2
    cmp     x9, #0
    csel    x9, xzr, x9, lt
    cmp     x9, x4
    csel    x9, x4, x9, gt
    orr     w0, w9, w0, lsl #8
    ldr     x9, [sp, #24]
    mul     x9, x9, x1
    ldr     x2, [sp]
    add     x9, x9, x2
    cmp     x9, #0
    csel    x9, xzr, x9, lt
    cmp     x9, x4
    csel    x9, x4, x9, gt
    orr     w0, w9, w0, lsl #8
    str     x0, [x20, x19, lsl #3]
    add     w19, w19, #1
    add     x1, x1, #1
    b       gradient_new.step
gradient_new.pair_end:
    add     sp, sp, #48
    str     x11, [x20, x19, lsl #3]     // the exact end stop
    add     w19, w19, #1
    add     w5, w5, #1
    b       gradient_new.pair
gradient_new.done:
    mov     w9, w19
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// gradient_capacity(x0=steps, x3=step count, x1=stop count) -> x9 = an
// upper bound on the spectrum length of Gradient::new(stops, steps).
// Clobbers x1, x2, x4, x16.
gradient_capacity:
    mov     x9, #1
    cmp     x1, #1
    b.ls    gradient_capacity.single
    sub     x1, x1, #1                  // pairs
    mov     x2, #0
gradient_capacity.pair:
    cmp     x2, x1
    b.hs    gradient_capacity.done
    sub     x4, x3, #1
    cmp     x2, x4
    csel    x4, x2, x4, lo
    ldr     x4, [x0, x4, lsl #3]
    add     x9, x9, x4
    add     x9, x9, #1
    add     x2, x2, #1
    b       gradient_capacity.pair
gradient_capacity.single:
    ldr     x16, [x0]
    add     x9, x9, x16
gradient_capacity.done:
    ret

// gradient_at_fraction(x0=spectrum, w1=length, d0=fraction) -> x9 = color.
// get_color_at_fraction: index = min(fraction * len as usize, len - 1), then
// stepped down while fraction <= index/len and up while fraction >
// (index + 1)/len - the exact float boundaries of the Python division.
// NaN or negative fractions give index 0. Clobbers x1-x3, d1-d2.
gradient_at_fraction:
    mov     w1, w1
    sub     x2, x1, #1                  // len - 1 (len 0 reads spectrum[-1])
    cbz     w1, gradient_at_fraction.pick
    scvtf   d1, x1                      // len
    fmul    d2, d0, d1
    fcvtzs  x3, d2                      // NaN gives 0, overflow saturates
    cmp     x3, #0
    csel    x3, xzr, x3, lt
    cmp     x3, x2
    csel    x3, x2, x3, hi
    mov     x2, x3
gradient_at_fraction.down:
    cbz     x2, gradient_at_fraction.up
    scvtf   d2, x2
    fdiv    d2, d2, d1
    fcmp    d0, d2
    b.hi    gradient_at_fraction.up     // fraction > index/len, or NaN: stop
    sub     x2, x2, #1
    b       gradient_at_fraction.down
gradient_at_fraction.up:
    add     x3, x2, #1
    cmp     x3, x1
    b.hs    gradient_at_fraction.pick
    scvtf   d2, x3
    fdiv    d2, d2, d1
    fcmp    d0, d2
    b.le    gradient_at_fraction.pick   // also NaN
    mov     x2, x3
    b       gradient_at_fraction.up
gradient_at_fraction.pick:
    ldr     x9, [x0, x2, lsl #3]
    ret

// gradient_map(x0=spectrum, w1=length, x2=min_row, x3=max_row,
//              x4=min_column, x5=max_column, x6=direction) -> x9 =
// a dense map: map[(row - min_row) * width + (column - min_column)] = color.
// build_coordinate_color_mapping; direction in GradientDirection order
// (vertical, horizontal, radial, diagonal). Callers validate the bounds.
// Locals: [sp + 64] spectrum, 72 length, 80 min_row, 88 max_row,
// 96 min_column, 104 max_column, 112 direction.
gradient_map:
    stp     x19, x20, [sp, #-128]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    stp     x0, x1, [sp, #64]
    stp     x2, x3, [sp, #80]
    stp     x4, x5, [sp, #96]
    str     x6, [sp, #112]
    sub     x20, x5, x4
    add     x20, x20, #1                // width
    sub     x9, x3, x2
    add     x9, x9, #1
    mul     x9, x9, x20
    lsl     x0, x9, #3
    bl      alloc
    mov     x24, x9                     // the map
    ldr     x21, [sp, #80]              // row
gradient_map.row:
    ldr     x9, [sp, #88]
    cmp     x21, x9
    b.gt    gradient_map.done
    ldr     x22, [sp, #96]              // column
gradient_map.column:
    ldr     x9, [sp, #104]
    cmp     x22, x9
    b.gt    gradient_map.next_row
    ldr     x9, [sp, #112]
    cmp     x9, #1
    b.eq    gradient_map.horizontal
    cmp     x9, #2
    b.eq    gradient_map.radial
    cmp     x9, #3
    b.eq    gradient_map.diagonal
    // vertical: (row - row_offset) / (max_row - row_offset)
    ldr     x9, [sp, #80]
    sub     x9, x9, #1                  // row_offset
    sub     x3, x21, x9
    ldr     x2, [sp, #88]
    sub     x2, x2, x9
    b       gradient_map.ratio
gradient_map.horizontal:
    ldr     x9, [sp, #96]
    sub     x9, x9, #1
    sub     x3, x22, x9
    ldr     x2, [sp, #104]
    sub     x2, x2, x9
    b       gradient_map.ratio
gradient_map.diagonal:
    // ((row - ro) * 2 + (column - co)) / ((max_row - ro) * 2 + (max_column - co))
    ldr     x9, [sp, #80]
    sub     x9, x9, #1
    sub     x3, x21, x9
    add     x3, x3, x3
    ldr     x2, [sp, #88]
    sub     x2, x2, x9
    add     x2, x2, x2
    ldr     x9, [sp, #96]
    sub     x9, x9, #1
    sub     x4, x22, x9
    add     x3, x3, x4
    ldr     x4, [sp, #104]
    sub     x4, x4, x9
    add     x2, x2, x4
gradient_map.ratio:
    scvtf   d0, x3
    scvtf   d1, x2
    fdiv    d0, d0, d1
    b       gradient_map.color
gradient_map.radial:
    ldp     x0, x1, [sp, #80]
    ldp     x2, x3, [sp, #96]
    lsl     x4, x21, #32
    mov     w9, w22
    orr     x4, x4, x9
    bl      find_normalized_distance_from_center
gradient_map.color:
    ldp     x0, x1, [sp, #64]
    bl      gradient_at_fraction
    ldr     x3, [sp, #80]
    sub     x3, x21, x3
    mul     x3, x3, x20
    add     x3, x3, x22
    ldr     x2, [sp, #96]
    sub     x3, x3, x2
    str     x9, [x24, x3, lsl #3]
    add     x22, x22, #1
    b       gradient_map.column
gradient_map.next_row:
    add     x21, x21, #1
    b       gradient_map.row
gradient_map.done:
    mov     x9, x24
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #128
    ret
