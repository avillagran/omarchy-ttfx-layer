// utils/geometry.s - Coord and geometry math (src/utils/geometry.rs).
//
// Coordinates are packed u64 values (ttfx.inc): the column as a signed i32
// in the low half, the row in the high half. Results are the low 32 bits of
// Rust's i64 values. Functions that return a list allocate it from the arena
// and return x9 = pointer, x2 = count, in Rust's push order; an empty
// list is a zero-length allocation.
//
// Float lowering follows the compiled oracle, not the source: powf(x, 2.0)
// is x * x, powf(x, 0.5) is fsqrt with LLVM's fabs and -inf fix-ups, the
// sin/cos pair is one sincos call, and lengths are glibc hypot. Every
// round() is round_half_even (pycompat.s) and every `as i64` saturates.

// The streaming ellipse (coords_in_circle_init / _next) keeps its state in
// caller-provided memory of CIRCLE_ITER_size bytes (the CIRCLE_ITER.* offsets
// in defs.inc):
//   .x          next column to open
//   .x_end
//   .y          next row in the open column
//   .y_end
//   .h          center column
//   .k          center row
//   .a_squared
//   .b_squared
//   .column     the open column

// GEO_ROUND_PAIR vd, vs, vtmp, xtmp: vd.2s (the low 64 bits of vd) = both
// f64 lanes of vs rounded half-even to i64 (round_half_even), each
// truncated to its low 32 bits: a packed coordinate. fcvtns is
// round_half_even for every input but +inf, which Rust's release build
// turns into i64::MIN (low word 0): those lanes are cleared.
.macro GEO_ROUND_PAIR vd, vs, vtmp, xtmp
    fcvtns  \vd\().2d, \vs\().2d
    mov     \xtmp, #0x7ff0000000000000
    dup     \vtmp\().2d, \xtmp
    cmeq    \vtmp\().2d, \vs\().2d, \vtmp\().2d
    bic     \vd\().16b, \vd\().16b, \vtmp\().16b
    xtn     \vd\().2s, \vd\().2d
.endm

// GEO_COORD_F64 vd, xsrc: vd.2d = (column, row) of packed coordinate xsrc
// as f64 (the cvtdq2pd of the x86 engine).
.macro GEO_COORD_F64 vd, xsrc
    fmov    d\vd, \xsrc
    sxtl    v\vd\().2d, v\vd\().2s
    scvtf   v\vd\().2d, v\vd\().2d
.endm

    .text

// find_coords_on_circle(x0=origin, x1=radius, x2=coords_limit,
//                       w3=unique) -> x9 = list, x2 = count.
// coords_limit 0 means round(2 * pi * radius); the x offset from the origin
// is doubled for the cell aspect; every point is rounded half-even. With
// unique set, a point equal to an earlier one is skipped.
find_coords_on_circle:
    sub     sp, sp, #128
    stp     x19, x20, [sp, #64]
    stp     x21, x22, [sp, #80]
    stp     x23, x24, [sp, #96]
    str     x30, [sp, #112]
    // [sp] origin column, [sp+8] origin row, [sp+16] radius (all f64),
    // [sp+24] angle_step, [sp+32] sin, [sp+40] cos, [sp+48] rounded x,
    // [sp+56] coords_limit
    mov     x23, #0                     // count
    cbz     x1, find_coords_on_circle.empty
    mov     x21, x0
    mov     w22, w3
    mov     x19, x2
    scvtf   d2, x1
    str     d2, [sp, #16]
    cbnz    x19, find_coords_on_circle.limit
    LDD     d0, geo_two_pi
    fmul    d0, d0, d2
    ROUND_HALF_EVEN
    mov     x19, x9
find_coords_on_circle.limit:
    cmp     x19, #0
    b.le    find_coords_on_circle.empty
    str     x19, [sp, #56]
    scvtf   d1, x19
    LDD     d0, geo_two_pi
    fdiv    d0, d0, d1
    str     d0, [sp, #24]               // angle_step
    sxtw    x9, w21
    scvtf   d0, x9
    str     d0, [sp]
    asr     x9, x21, #32
    scvtf   d0, x9
    str     d0, [sp, #8]
    lsl     x0, x19, #3
    bl      alloc
    mov     x24, x9                     // the list
    mov     x20, #0                     // the seen set, only when unique
    cbz     w22, find_coords_on_circle.points
    mov     x0, x19
    bl      coordset_new
    mov     x20, x9
find_coords_on_circle.points:
    mov     x22, #0                     // i
find_coords_on_circle.point:
    ldr     x9, [sp, #56]
    cmp     x22, x9
    b.ge    find_coords_on_circle.done
    scvtf   d0, x22
    ldr     d1, [sp, #24]
    fmul    d0, d0, d1                  // angle
    add     x0, sp, #32
    add     x1, sp, #40
    CCALL   sincos
    // x = origin.column + radius * cos; x += x - origin.column
    ldr     d0, [sp, #40]
    ldr     d1, [sp, #16]
    fmul    d0, d0, d1
    ldr     d1, [sp]
    fadd    d0, d0, d1
    fsub    d2, d0, d1
    fadd    d0, d0, d2
    ROUND_HALF_EVEN
    str     x9, [sp, #48]
    // y = origin.row + radius * sin
    ldr     d0, [sp, #32]
    ldr     d1, [sp, #16]
    fmul    d0, d0, d1
    ldr     d1, [sp, #8]
    fadd    d0, d0, d1
    ROUND_HALF_EVEN
    ldr     w3, [sp, #48]
    orr     x21, x3, x9, lsl #32        // the point
    cbz     x20, find_coords_on_circle.push
    mov     x0, x20
    mov     x1, x21
    bl      coordset_insert
    cbz     w9, find_coords_on_circle.next
find_coords_on_circle.push:
    str     x21, [x24, x23, lsl #3]
    add     x23, x23, #1
find_coords_on_circle.next:
    add     x22, x22, #1
    b       find_coords_on_circle.point
find_coords_on_circle.empty:
    mov     x0, #0
    bl      alloc
    mov     x24, x9
find_coords_on_circle.done:
    mov     x9, x24
    mov     x2, x23
    ldp     x19, x20, [sp, #64]
    ldp     x21, x22, [sp, #80]
    ldp     x23, x24, [sp, #96]
    ldr     x30, [sp, #112]
    add     sp, sp, #128
    ret

// coordset_new(x0=at most this many keys) -> x9 = an open-addressing set
// of packed coordinates: a mask, then 16-byte (key, occupied) entries at
// +16. Capacity is at least twice the key count, so probing terminates.
coordset_new:
    stp     x19, x30, [sp, #-16]!
    lsl     x9, x0, #1
    cmp     x9, #16
    b.hs    coordset_new.size
    mov     x9, #16
coordset_new.size:
    sub     x9, x9, #1
    clz     x3, x9
    mov     x9, #64
    sub     x9, x9, x3
    mov     x19, #1
    lsl     x19, x19, x9                // capacity: the next power of two
    lsl     x0, x19, #4
    add     x0, x0, #16
    bl      alloc
    sub     x19, x19, #1
    str     x19, [x9]                   // mask
    ldp     x19, x30, [sp], #16
    ret

// coordset_insert(x0=set, x1=key) -> w9 = 1 when the key was not there.
// Clobbers x3, x2, x4.
coordset_insert:
    MOV64   x3, 0x9E3779B97F4A7C15
    mul     x9, x1, x3
    ldr     x2, [x0]                    // mask
    clz     x3, x2                      // the mask is at least 15
    lsr     x9, x9, x3                  // the top bits index the table
coordset_insert.probe:
    add     x4, x0, x9, lsl #4
    ldr     x3, [x4, #24]
    cbz     x3, coordset_insert.insert
    ldr     x3, [x4, #16]
    cmp     x3, x1
    b.eq    coordset_insert.found
    add     x9, x9, #1
    and     x9, x9, x2
    b       coordset_insert.probe
coordset_insert.insert:
    mov     x3, #1
    stp     x1, x3, [x4, #16]
    mov     w9, #1
    ret
coordset_insert.found:
    mov     w9, #0
    ret

// coords_in_circle_init(x0=state, x1=center, x2=diameter): start the
// streaming ellipse of geometry::coords_in_circle (a = diameter,
// b = diameter / 2). A zero diameter streams nothing.
// Clobbers x9, x3, d0, d1.
coords_in_circle_init:
    sxtw    x9, w1
    str     x9, [x0, #CIRCLE_ITER.h]
    asr     x3, x1, #32
    str     x3, [x0, #CIRCLE_ITER.k]
    str     xzr, [x0, #CIRCLE_ITER.y]
    mov     x3, #-1
    str     x3, [x0, #CIRCLE_ITER.y_end]
    cbz     x2, coords_in_circle_init.none
    sub     x3, x9, x2
    str     x3, [x0, #CIRCLE_ITER.x]
    add     x9, x9, x2
    str     x9, [x0, #CIRCLE_ITER.x_end]
    scvtf   d0, x2
    fmov    d1, #0.5
    fmul    d1, d0, d1                  // diameter / 2.0
    fmul    d0, d0, d0                  // diameter.powf(2.0)
    str     d0, [x0, #CIRCLE_ITER.a_squared]
    fmul    d1, d1, d1                  // (diameter / 2.0).powf(2.0)
    str     d1, [x0, #CIRCLE_ITER.b_squared]
    ret
coords_in_circle_init.none:
    mov     x3, #1
    str     x3, [x0, #CIRCLE_ITER.x]
    str     xzr, [x0, #CIRCLE_ITER.x_end]
    ret

// coords_in_circle_next(x0=state) -> x9 = the next coordinate, w2 = 1;
// w2 = 0 when the ellipse is exhausted. Column-major, rows ascending.
// Clobbers x3, x1, x0, d0-d3.
coords_in_circle_next:
    stp     x19, x30, [sp, #-16]!
    mov     x19, x0
coords_in_circle_next.again:
    ldr     x9, [x19, #CIRCLE_ITER.y]
    ldr     x3, [x19, #CIRCLE_ITER.y_end]
    cmp     x9, x3
    b.gt    coords_in_circle_next.column
    add     x3, x9, #1
    str     x3, [x19, #CIRCLE_ITER.y]
    ldr     w3, [x19, #CIRCLE_ITER.column]
    orr     x9, x3, x9, lsl #32
    mov     w2, #1
    ldp     x19, x30, [sp], #16
    ret
coords_in_circle_next.column:
    ldr     x0, [x19, #CIRCLE_ITER.x]
    ldr     x3, [x19, #CIRCLE_ITER.x_end]
    cmp     x0, x3
    b.gt    coords_in_circle_next.end
    add     x9, x0, #1
    str     x9, [x19, #CIRCLE_ITER.x]
    str     x0, [x19, #CIRCLE_ITER.column]
    ldr     x1, [x19, #CIRCLE_ITER.h]
    ldr     x2, [x19, #CIRCLE_ITER.k]
    ldr     d2, [x19, #CIRCLE_ITER.a_squared]
    ldr     d3, [x19, #CIRCLE_ITER.b_squared]
    bl      circle_column_range
    str     x9, [x19, #CIRCLE_ITER.y]
    str     x2, [x19, #CIRCLE_ITER.y_end]
    b       coords_in_circle_next.again
coords_in_circle_next.end:
    mov     x9, #0
    mov     w2, #0
    ldp     x19, x30, [sp], #16
    ret

// circle_column_range(x0=x, x1=h, x2=k, d2=a_squared, d3=b_squared)
// -> x9 = first row, x2 = last row (inclusive; empty when x9 > x2).
// circle_column_y_range: the y offset is (b^2 * (1 - (x - h)^2 / a^2)) ^ 0.5
// truncated. The oracle lowered that powf to sqrt, plus fabs (so -0 gives
// +0) and +inf for a -inf argument, before the saturating cast.
// Clobbers x3, x0, d0, d1.
circle_column_range:
    sub     x0, x0, x1
    scvtf   d0, x0
    fmul    d0, d0, d0
    fdiv    d0, d0, d2                  // x_component
    fmov    d1, #1.0
    fsub    d1, d1, d0
    fmul    d1, d1, d3                  // the powf argument
    fsqrt   d0, d1
    fabs    d0, d0
    fcvtzs  x9, d0
    fmov    x3, d1
    mov     x0, #0xfff0000000000000     // -inf: one bit pattern
    cmp     x3, x0
    mov     x0, #0x7fffffffffffffff     // (-inf).powf(0.5) is +inf
    csel    x9, x0, x9, eq
    mov     x3, x9
    sub     x9, x2, x3                  // k - max_y_offset
    add     x2, x2, x3                  // k + max_y_offset
    ret

// find_coords_in_circle(x0=center, x1=diameter) -> x9 = list, x2 = count.
// The streamed ellipse as a list: one pass to count, one to fill.
find_coords_in_circle:
    sub     sp, sp, #128
    stp     x19, x21, [sp, #80]
    stp     x22, x23, [sp, #96]
    str     x30, [sp, #112]
    // [sp] the CIRCLE_ITER state
    mov     x21, x0
    mov     x22, x1
    mov     x0, sp
    mov     x2, x1
    mov     x1, x21
    bl      coords_in_circle_init
    mov     x19, #0
find_coords_in_circle.count:
    mov     x0, sp
    bl      coords_in_circle_next
    cbz     w2, find_coords_in_circle.counted
    add     x19, x19, #1
    b       find_coords_in_circle.count
find_coords_in_circle.counted:
    lsl     x0, x19, #3
    bl      alloc
    mov     x23, x9
    mov     x0, sp
    mov     x1, x21
    mov     x2, x22
    bl      coords_in_circle_init
    mov     x19, #0
find_coords_in_circle.fill:
    mov     x0, sp
    bl      coords_in_circle_next
    cbz     w2, find_coords_in_circle.done
    str     x9, [x23, x19, lsl #3]
    add     x19, x19, #1
    b       find_coords_in_circle.fill
find_coords_in_circle.done:
    mov     x9, x23
    mov     x2, x19
    ldp     x19, x21, [sp, #80]
    ldp     x22, x23, [sp, #96]
    ldr     x30, [sp, #112]
    add     sp, sp, #128
    ret

// find_coords_in_rect(x0=origin, x1=distance) -> x9 = list, x2 = count.
// The full (2d + 1)^2 block, column-major; empty for distance <= 0.
find_coords_in_rect:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    str     x30, [sp, #32]
    cmp     x1, #0
    b.le    find_coords_in_rect.empty
    sxtw    x21, w0                     // column
    asr     x22, x0, #32                // row
    mov     x23, x1
    lsl     x9, x1, #1
    add     x9, x9, #1
    mul     x0, x9, x9
    lsl     x0, x0, #3
    bl      alloc
    mov     x19, x9
    mov     x4, #0                      // count
    sub     x3, x21, x23                // column
    add     x5, x21, x23                // last column
    add     x10, x22, x23               // last row
find_coords_in_rect.column:
    cmp     x3, x5
    b.gt    find_coords_in_rect.done
    sub     x2, x22, x23                // row
find_coords_in_rect.row:
    cmp     x2, x10
    b.gt    find_coords_in_rect.next
    mov     w9, w3
    orr     x9, x9, x2, lsl #32
    str     x9, [x19, x4, lsl #3]
    add     x4, x4, #1
    add     x2, x2, #1
    b       find_coords_in_rect.row
find_coords_in_rect.next:
    add     x3, x3, #1
    b       find_coords_in_rect.column
find_coords_in_rect.empty:
    mov     x0, #0
    bl      alloc
    mov     x19, x9
    mov     x4, #0
find_coords_in_rect.done:
    mov     x9, x19
    mov     x2, x4
    ldp     x22, x23, [sp, #16]
    ldr     x30, [sp, #32]
    ldp     x19, x21, [sp], #48
    ret

// find_coords_on_rect(x0=origin, x1=half_width, x2=half_height)
// -> x9 = list, x2 = count. The perimeter: the first and last columns in
// full, two rows for every column between; empty when either half is 0.
// A negative half_width has no columns; a negative half_height keeps the
// middle columns' two rows and empties the edge columns, as Rust's ranges do.
find_coords_on_rect:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    cbz     x1, find_coords_on_rect.empty
    cbz     x2, find_coords_on_rect.empty
    tbnz    x1, #63, find_coords_on_rect.empty
    sxtw    x21, w0                     // column
    asr     x22, x0, #32                // row
    mov     x23, x1
    mov     x24, x2
    // capacity: 2 * edge column rows + 2 * (2 * half_width - 1)
    lsl     x9, x24, #1
    add     x9, x9, #1
    cmp     x24, #0
    csel    x9, xzr, x9, lt
    lsl     x0, x23, #1
    sub     x0, x0, #1
    add     x0, x0, x9
    lsl     x0, x0, #4
    bl      alloc
    mov     x19, x9
    mov     x4, #0                      // count
    sub     x1, x21, x23                // first column
    mov     x3, x1                      // column
    add     x5, x21, x23                // last column
    sub     x10, x22, x24               // first row
    add     x11, x22, x24               // last row
find_coords_on_rect.column:
    cmp     x3, x5
    b.gt    find_coords_on_rect.done
    cmp     x3, x1
    b.eq    find_coords_on_rect.full
    cmp     x3, x5
    b.eq    find_coords_on_rect.full
    mov     w9, w3
    orr     x9, x9, x10, lsl #32
    str     x9, [x19, x4, lsl #3]
    add     x4, x4, #1
    mov     w9, w3
    orr     x9, x9, x11, lsl #32
    str     x9, [x19, x4, lsl #3]
    add     x4, x4, #1
    b       find_coords_on_rect.next
find_coords_on_rect.full:
    mov     x2, x10
find_coords_on_rect.row:
    cmp     x2, x11
    b.gt    find_coords_on_rect.next
    mov     w9, w3
    orr     x9, x9, x2, lsl #32
    str     x9, [x19, x4, lsl #3]
    add     x4, x4, #1
    add     x2, x2, #1
    b       find_coords_on_rect.row
find_coords_on_rect.next:
    add     x3, x3, #1
    b       find_coords_on_rect.column
find_coords_on_rect.empty:
    mov     x0, #0
    bl      alloc
    mov     x19, x9
    mov     x4, #0
find_coords_on_rect.done:
    mov     x9, x19
    mov     x2, x4
    ldp     x22, x23, [sp, #16]
    ldp     x24, x30, [sp, #32]
    ldp     x19, x21, [sp], #48
    ret

// find_length_of_line(x0=coord1, x1=coord2, w2=double_row_diff) -> d0.
// glibc hypot of the deltas, the row delta doubled when asked (as an
// addition, which is what the oracle emits for 2.0 * x). hypot is a tail
// call. Clobbers everything a C call does.
find_length_of_line:
    sxtw    x9, w1
    sxtw    x3, w0
    sub     x9, x9, x3
    scvtf   d0, x9
    asr     x9, x1, #32
    asr     x3, x0, #32
    sub     x9, x9, x3
    scvtf   d1, x9
    cbz     w2, find_length_of_line.hypot
    fadd    d1, d1, d1
find_length_of_line.hypot:
    b       hypot

// extrapolate_along_ray(x0=origin, x1=target, d0=offset_from_target)
// -> x9 = coord. Lerp past the target by offset along the (non-doubled)
// line, rounded; the target itself when the total distance is 0 or the
// points coincide.
extrapolate_along_ray:
    sub     sp, sp, #48
    stp     x19, x21, [sp, #16]
    str     x30, [sp, #32]
    mov     x19, x0
    mov     x21, x1
    str     d0, [sp]
    mov     w2, #0
    bl      find_length_of_line
    str     d0, [sp, #8]                // base
    ldr     d1, [sp]
    fadd    d1, d1, d0                  // total_distance
    fcmp    d1, #0.0
    b.eq    extrapolate_along_ray.target    // ordered equal only
    cmp     x19, x21
    b.eq    extrapolate_along_ray.target
    ldr     d2, [sp, #8]
    fdiv    d1, d1, d2                  // t
    fmov    d2, #1.0
    fsub    d2, d2, d1                  // 1 - t
    sxtw    x9, w19
    scvtf   d3, x9
    fmul    d3, d3, d2
    sxtw    x9, w21
    scvtf   d0, x9
    fmul    d0, d0, d1
    fadd    d0, d0, d3
    ROUND_HALF_EVEN
    mov     w12, w9                     // column, low 32 bits
    asr     x9, x19, #32
    scvtf   d3, x9
    fmul    d3, d3, d2
    asr     x9, x21, #32
    scvtf   d0, x9
    fmul    d0, d0, d1
    fadd    d0, d0, d3
    ROUND_HALF_EVEN
    orr     x9, x12, x9, lsl #32
    b       extrapolate_along_ray.done
extrapolate_along_ray.target:
    mov     x9, x21
extrapolate_along_ray.done:
    ldp     x19, x21, [sp, #16]
    ldr     x30, [sp, #32]
    add     sp, sp, #48
    ret

// find_coord_on_line(x0=start, x1=end, d0=t) -> x9 = coord:
// (1 - t) * start + t * end per axis, rounded half-even.
// Both axes at once: the same multiplies and adds per lane, and fcvtns
// rounds each lane half to even like round_half_even (GEO_ROUND_PAIR).
// Clobbers x3, x2, v0-v4.
find_coord_on_line:
    GEO_COORD_F64 3, x0                 // start column, row
    GEO_COORD_F64 4, x1                 // end column, row
    fmov    d1, #1.0
    fsub    d1, d1, d0                  // 1 - t
    fmul    v3.2d, v3.2d, v1.d[0]
    fmul    v4.2d, v4.2d, v0.d[0]
    fadd    v3.2d, v3.2d, v4.2d
    GEO_ROUND_PAIR v4, v3, v2, x3
    fmov    x9, d4
    ret

// find_coord_on_bezier_curve(x0=start, x1=control points, x2=control
// count, x3=end, d0=t) -> x9 = coord. De Casteljau with float
// intermediates, rounded once at the end. No control points is the line;
// one (every production path) stays in registers; more use the stack.
// Clobbers x1-x4, x12-x14, v0-v6.
find_coord_on_bezier_curve:
    cbnz    x2, find_coord_on_bezier_curve.curve
    mov     x1, x3
    b       find_coord_on_line
find_coord_on_bezier_curve.curve:
    cmp     x2, #1
    b.ne    find_coord_on_bezier_curve.many
    // one control point: both axes at once, as in find_coord_on_line
    GEO_COORD_F64 1, x0                 // start
    ldr     x2, [x1]
    GEO_COORD_F64 2, x2                 // control
    GEO_COORD_F64 3, x3                 // end
    fmov    d5, #1.0
    fsub    d5, d5, d0                  // 1 - t
    fmul    v1.2d, v1.2d, v5.d[0]
    fmul    v6.2d, v2.2d, v0.d[0]
    fadd    v1.2d, v1.2d, v6.2d         // start.interpolate(control, t)
    fmul    v2.2d, v2.2d, v5.d[0]
    fmul    v3.2d, v3.2d, v0.d[0]
    fadd    v2.2d, v2.2d, v3.2d         // control.interpolate(end, t)
    fmul    v1.2d, v1.2d, v5.d[0]
    fmul    v2.2d, v2.2d, v0.d[0]
    fadd    v1.2d, v1.2d, v2.2d         // the point between them
    GEO_ROUND_PAIR v0, v1, v2, x3
    fmov    x9, d0
    ret
find_coord_on_bezier_curve.many:
    fmov    d4, d0                      // t
    fmov    d5, #1.0
    fsub    d5, d5, d0                  // 1 - t
    // points[0..n+2] as (column, row) f64 pairs on the stack
    mov     x12, sp
    add     x9, x2, #2
    lsl     x9, x9, #4
    sub     sp, sp, x9
    mov     x13, x2                     // remaining - 2
    sxtw    x9, w0
    scvtf   d0, x9
    str     d0, [sp]
    asr     x9, x0, #32
    scvtf   d0, x9
    str     d0, [sp, #8]
    mov     x14, #0
    add     x4, sp, #16                 // points[1..]
find_coord_on_bezier_curve.load:
    ldr     x9, [x1, x14, lsl #3]
    sxtw    x2, w9
    scvtf   d0, x2
    str     d0, [x4]
    asr     x9, x9, #32
    scvtf   d0, x9
    str     d0, [x4, #8]
    add     x4, x4, #16
    add     x14, x14, #1
    cmp     x14, x13
    b.lo    find_coord_on_bezier_curve.load
    sxtw    x9, w3
    scvtf   d0, x9
    str     d0, [x4]
    asr     x9, x3, #32
    scvtf   d0, x9
    str     d0, [x4, #8]
    add     x13, x13, #2                // remaining
find_coord_on_bezier_curve.level:
    cmp     x13, #1
    b.ls    find_coord_on_bezier_curve.collapsed
    mov     x14, #0
    mov     x2, sp
find_coord_on_bezier_curve.pair:
    add     x9, x14, #1
    cmp     x9, x13
    b.hs    find_coord_on_bezier_curve.next_level
    // points[i] = points[i].interpolate(points[i + 1], t)
    ldr     d0, [x2]
    fmul    d0, d0, d5
    ldr     d1, [x2, #16]
    fmul    d1, d1, d4
    fadd    d0, d0, d1
    str     d0, [x2]
    ldr     d0, [x2, #8]
    fmul    d0, d0, d5
    ldr     d1, [x2, #24]
    fmul    d1, d1, d4
    fadd    d0, d0, d1
    str     d0, [x2, #8]
    add     x2, x2, #16
    add     x14, x14, #1
    b       find_coord_on_bezier_curve.pair
find_coord_on_bezier_curve.next_level:
    sub     x13, x13, #1
    b       find_coord_on_bezier_curve.level
find_coord_on_bezier_curve.collapsed:
    ldr     d0, [sp]
    ldr     d1, [sp, #8]
    mov     sp, x12
    ROUND_HALF_EVEN
    mov     w12, w9
    fmov    d0, d1
    ROUND_HALF_EVEN
    orr     x9, x12, x9, lsl #32
    ret

// dropped: find_coord_on_bezier_curve.wide find_coord_on_bezier_curve.general find_coord_on_bezier_curve.round (GEO_ROUND_PAIR handles every lane; no i32 fallback)

// find_length_of_bezier_curve(x0=start, x1=control points, x2=control
// count, x3=end) -> d0. The 10-sample polyline that stops at t = 0.9:
// the final span is omitted, faithfully (plan.md §5.4). Segment lengths use
// the doubled row delta.
find_length_of_bezier_curve:
    sub     sp, sp, #64
    stp     x19, x20, [sp]
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    // [sp+56] length
    mov     x19, x0
    mov     x21, x1
    mov     x22, x2
    mov     x23, x3
    mov     x24, x0                     // prev_coord
    str     xzr, [sp, #56]              // length = 0.0
    mov     w20, #1
find_length_of_bezier_curve.segment:
    scvtf   d0, w20
    fmov    d1, #10.0
    fdiv    d0, d0, d1                  // t
    mov     x0, x19
    mov     x1, x21
    mov     x2, x22
    mov     x3, x23
    bl      find_coord_on_bezier_curve
    mov     x0, x24
    mov     x1, x9
    mov     x24, x9
    mov     w2, #1
    bl      find_length_of_line
    ldr     d1, [sp, #56]
    fadd    d0, d0, d1
    str     d0, [sp, #56]
    add     w20, w20, #1
    cmp     w20, #10
    b.lo    find_length_of_bezier_curve.segment
    ldr     d0, [sp, #56]
    ldp     x19, x20, [sp]
    ldp     x21, x22, [sp, #16]
    ldp     x23, x24, [sp, #32]
    ldr     x30, [sp, #48]
    add     sp, sp, #64
    ret

// find_normalized_distance_from_center(x0=bottom, x1=top, x2=left,
//   x3=right, x4=coord) -> w9 = 1 and d0 = the distance in [0, 1], or
// w9 = 0 when the coordinate is outside the rectangle (Rust's Err; the
// message is msg_not_in_rectangle). The math is graphics.s's
// normalized_distance_from_center. Clobbers x3, x2, x1, x0, x4, x5, x10, d0-d4.
find_normalized_distance_from_center:
    // column - x_offset in 1..=right - x_offset, row likewise
    sub     x5, x2, #1
    sxtw    x9, w4
    sub     x9, x9, x5
    sub     x10, x3, x5
    cmp     x9, #1
    b.lt    find_normalized_distance_from_center.outside
    cmp     x9, x10
    b.gt    find_normalized_distance_from_center.outside
    sub     x5, x0, #1
    asr     x9, x4, #32
    sub     x9, x9, x5
    sub     x10, x1, x5
    cmp     x9, #1
    b.lt    find_normalized_distance_from_center.outside
    cmp     x9, x10
    b.gt    find_normalized_distance_from_center.outside
    asr     x5, x4, #32
    sxtw    x4, w4
    str     x30, [sp, #-16]!
    bl      normalized_distance_from_center
    ldr     x30, [sp], #16
    mov     w9, #1
    ret
find_normalized_distance_from_center.outside:
    mov     w9, #0
    ret

// normalized_distance_from_center(x0=bottom, x1=top, x2=left, x3=right,
//   x4=column, x5=row) -> d0. geometry::find_normalized_distance_from_center
// for a coordinate known to lie inside the rectangle. The oracle's compiler
// lowered powf(x, 2.0) to x * x and powf(x, 0.5) to sqrt (its arguments here
// are sums of squares, so the fabs / -inf fix-ups never apply); so does this.
// Clobbers x9, x10, x1, x3-x5, d0-d4.
normalized_distance_from_center:
    sub     x9, x0, #1                  // y_offset
    sub     x10, x2, #1                 // x_offset
    sub     x3, x3, x10                 // right
    sub     x1, x1, x9                  // top
    fmov    d1, #0.5
    scvtf   d2, x3
    fmul    d2, d2, d1                  // center_x
    scvtf   d3, x1
    fmul    d3, d3, d1                  // center_y  (n / 2.0 == n * 0.5 exactly)
    sub     x4, x4, x10                 // column
    sub     x5, x5, x9                  // row
    // max_distance = sqrt(right^2 + (top * 2)^2)
    scvtf   d0, x3
    fmul    d0, d0, d0
    lsl     x9, x1, #1
    scvtf   d1, x9
    fmul    d1, d1, d1
    fadd    d0, d0, d1
    fsqrt   d4, d0
    // distance = sqrt((column - cx)^2 + ((row - cy) * 2)^2)
    scvtf   d0, x4
    fsub    d0, d0, d2
    fmul    d0, d0, d0
    scvtf   d1, x5
    fsub    d1, d1, d3
    fadd    d1, d1, d1
    fmul    d1, d1, d1
    fadd    d0, d0, d1
    fsqrt   d0, d0
    // distance / (max_distance / 2.0)
    fmov    d1, #0.5
    fmul    d4, d4, d1
    fdiv    d0, d0, d4
    ret

// utf8_pack(w0=codepoint) -> x9 = packed symbol (bytes | length << 32).
// Clobbers x3.
utf8_pack:
    cmp     w0, #0x80
    b.lo    utf8_pack.one
    cmp     w0, #0x800
    b.lo    utf8_pack.two
    cmp     w0, #0x10000
    b.lo    utf8_pack.three
    lsr     w9, w0, #18
    orr     w9, w9, #0xf0
    ubfx    w3, w0, #12, #6
    orr     w3, w3, #0x80
    orr     w9, w9, w3, lsl #8
    ubfx    w3, w0, #6, #6
    orr     w3, w3, #0x80
    orr     w9, w9, w3, lsl #16
    and     w3, w0, #0x3f
    orr     w3, w3, #0x80
    orr     w9, w9, w3, lsl #24
    orr     x9, x9, #(4 << 32)
    ret
utf8_pack.three:
    lsr     w9, w0, #12
    orr     w9, w9, #0xe0
    ubfx    w3, w0, #6, #6
    orr     w3, w3, #0x80
    orr     w9, w9, w3, lsl #8
    and     w3, w0, #0x3f
    orr     w3, w3, #0x80
    orr     w9, w9, w3, lsl #16
    orr     x9, x9, #(3 << 32)
    ret
utf8_pack.two:
    lsr     w9, w0, #6
    orr     w9, w9, #0xc0
    and     w3, w0, #0x3f
    orr     w3, w3, #0x80
    orr     w9, w9, w3, lsl #8
    orr     x9, x9, #(2 << 32)
    ret
utf8_pack.one:
    mov     w9, w0
    orr     x9, x9, #(1 << 32)
    ret

    .section .rodata
    .balign 8
geo_two_pi:     .8byte 0x401921fb54442d18   // 2.0 * PI, as the oracle folded it
STRING msg_not_in_rectangle, "Coordinate is not within the rectangle."

// dropped: geo_abs_mask geo_int_min geo_one geo_half geo_ten geo_neg_inf (fabs, fmov immediates and an integer -inf compare instead)
