// utils/easing.s - src/utils/easing.rs, named easings and step trackers.
// The release oracle (aarch64 as on x86) uses pow for exponents 3/4/5, fmul
// for squares, fsqrt for roots and exp2 for powers of two. Preserve its
// operand order: InElastic negates the sine argument; InBack multiplies by
// negative c1; InOutSine and the high polynomial branches multiply by -0.5.
// No sincos call occurs in these arms.
//
// EASE_BEZIER_BASE, EASE_MEMO_BITS, EASE_CHEAP, BEZIER_RING, BEZIER_LIMIT
// and the EasingTracker / SequenceStep / SequenceEaser layouts are in
// defs.inc. Ids evaluated without the memo (EASE_CHEAP): linear, the quads,
// circs, InOutBack and the bounces (no libm calls).

    .text

// ease(w0=Easing id 0..30, d0=t) -> d0.
// All real inputs are accepted, including extrapolation. Ids from
// EASE_BEZIER_BASE up are the CubicBezier curves made by bezier_new.
// Easings that call libm (and the curves) go through a per-run memo keyed
// by (id, bits of t): an easing is a pure function of both, and the same
// (easing, step / steps) pairs recur for every character that shares a
// path length, so most calls skip pow/exp2/sin/cos. The rest are cheaper
// to evaluate than to look up.
// Clobbers all caller-saved registers (libm through CCALL).
ease:
    mov     w0, w0                      // id is a 32-bit argument
    cmp     w0, #32
    b.hs    ease.memo
    MOV64   x9, EASE_CHEAP
    lsr     x9, x9, x0
    tbnz    x9, #0, ease_eval
ease.memo:
    LDX     x1, ease_memo
    cbz     x1, ease.alloc
ease.lookup:
    fmov    x9, d0
    add     w3, w0, #1                  // tag; 0 marks an empty entry
    add     x2, x9, x3
    MOV64   x4, 0x9E3779B97F4A7C15
    mul     x2, x2, x4
    lsr     x2, x2, #(64 - EASE_MEMO_BITS)
    add     x1, x1, x2, lsl #5
    ldr     x10, [x1]
    cmp     x10, x9
    b.ne    ease.miss
    ldr     w10, [x1, #16]
    cmp     w10, w3
    b.ne    ease.miss
    ldr     d0, [x1, #8]
    ret
ease.miss:
    stp     x1, x3, [sp, #-32]!
    stp     x9, x30, [sp, #16]
    bl      ease_eval
    ldp     x9, x30, [sp, #16]
    ldp     x1, x3, [sp], #32
    str     x9, [x1]
    str     d0, [x1, #8]
    str     w3, [x1, #16]
    ret
ease.alloc:
    stp     x0, x30, [sp, #-32]!
    str     d0, [sp, #16]
    mov     x0, #((1 << EASE_MEMO_BITS) * 32)
    bl      alloc                       // zeroed
    ldr     d0, [sp, #16]
    ldp     x0, x30, [sp], #32
    STX     x9, ease_memo
    mov     x1, x9
    b       ease.lookup

// ease_eval(w0=id, d0=t) -> d0: ease() without the memo.
// Frame: [sp] and [sp, #16], [sp, #32] locals, x30 at [sp, #8].
// d5 holds t; d6 carries the constants x86 took from memory.
ease_eval:
    cmp     w0, #EASE_BEZIER_BASE
    b.hs    ease_bezier_id
    sub     sp, sp, #48
    str     x30, [sp, #8]
    fmov    d5, d0
    ADRG    x3, ease_table
    ldrsw   x9, [x3, w0, uxtw #2]
    add     x9, x9, x3
    br      x9
ease_eval.in_sine:
    LDD     d6, ease_pi
    fmul    d5, d5, d6
    fmov    d6, #0.5
    fmul    d5, d5, d6
    fmov    d0, d5
    CCALL   cos
    b       ease_eval.complement
ease_eval.in_out_elastic:
    movi    d2, #0
    fcmp    d5, d2
    b.eq    ease_eval.done
ease_eval.elastic_mid_one:
    fmov    d2, #1.0
    fcmp    d5, d2
    b.eq    ease_eval.done
ease_eval.elastic_mid:
    fmov    d1, #20.0
    fmul    d1, d1, d5
    LDD     d0, ease_neg_eleven_eighth
    str     d1, [sp, #16]
    fadd    d0, d0, d1
    LDD     d6, ease_elastic_c5
    fmul    d0, d0, d6
    str     d5, [sp]
    CCALL   sin
    str     d0, [sp, #32]
    fmov    d0, #0.5
    ldr     d6, [sp]
    fcmp    d0, d6
    b.le    ease_eval.elastic_high      // 0.5 <= t or unordered
    ldr     d0, [sp, #16]
    fmov    d6, #-10.0
    fadd    d0, d0, d6
    CCALL   exp2
    ldr     d6, [sp, #32]
    fmul    d2, d0, d6
    fmov    d6, #-0.5
    fmul    d2, d2, d6
    b       ease_eval.done
ease_eval.in_expo:
    movi    d2, #0
    fcmp    d5, d2
    b.eq    ease_eval.done
ease_eval.expo_in:
    fmov    d6, #10.0
    fmul    d5, d5, d6
    fmov    d6, #-10.0
    fadd    d5, d5, d6
    fmov    d0, d5
    CCALL   exp2
    b       ease_eval.leave
ease_eval.out_quint:
    fmov    d0, #1.0
    fsub    d0, d0, d5
    fmov    d1, #5.0
    b       ease_eval.power_complement
ease_eval.out_quart:
    fmov    d0, #1.0
    fsub    d0, d0, d5
    fmov    d1, #4.0
    b       ease_eval.power_complement
ease_eval.in_quad:
    fmul    d5, d5, d5
    b       ease_eval.linear
ease_eval.in_out_quart:
    fmov    d0, #0.5
    fcmp    d0, d5
    b.le    ease_eval.quart_high
    fmov    d1, #4.0
    fmov    d0, d5
    CCALL   pow
    fmov    d6, #8.0
    fmul    d2, d0, d6
    b       ease_eval.done
ease_eval.in_out_cubic:
    fmov    d0, #0.5
    fcmp    d0, d5
    b.le    ease_eval.cubic_high
    fmov    d1, #3.0
    fmov    d0, d5
    CCALL   pow
    fmov    d6, #4.0
    fmul    d2, d0, d6
    b       ease_eval.done
ease_eval.in_elastic:
    movi    d2, #0
    fcmp    d5, d2
    b.eq    ease_eval.done
ease_eval.elastic_in_one:
    fmov    d2, #1.0
    fcmp    d5, d2
    b.eq    ease_eval.done
ease_eval.elastic_in:
    fmov    d6, #10.0
    fmul    d5, d5, d6
    str     d5, [sp]
    fmov    d0, #-10.0
    fadd    d0, d0, d5
    CCALL   exp2
    str     d0, [sp, #16]
    ldr     d0, [sp]
    LDD     d6, ease_neg_ten_three_quarters
    fadd    d0, d0, d6
    LDD     d6, ease_neg_elastic_c4
    fmul    d0, d0, d6
    CCALL   sin
    ldr     d6, [sp, #16]
    fmul    d2, d0, d6
    b       ease_eval.done
ease_eval.in_out_back:
    fmov    d0, #0.5
    fcmp    d0, d5
    fadd    d2, d5, d5
    b.le    ease_eval.back_high
    fmul    d2, d2, d2
    LDD     d6, ease_back_twice_c2_plus_one
    fmul    d5, d5, d6
    LDD     d6, ease_neg_back_c2
    fadd    d5, d5, d6
    fmul    d5, d5, d2
    fmov    d6, #0.5
    fmul    d5, d5, d6
    b       ease_eval.linear
ease_eval.out_sine:
    LDD     d6, ease_pi
    fmul    d5, d5, d6
    fmov    d6, #0.5
    fmul    d5, d5, d6
    fmov    d0, d5
    CCALL   sin
    b       ease_eval.leave
ease_eval.in_out_quint:
    fmov    d0, #0.5
    fcmp    d0, d5
    b.le    ease_eval.quint_high
    fmov    d1, #5.0
    fmov    d0, d5
    CCALL   pow
    fmov    d6, #16.0
    fmul    d2, d0, d6
    b       ease_eval.done
ease_eval.in_out_sine:
    LDD     d6, ease_pi
    fmul    d5, d5, d6
    fmov    d0, d5
    CCALL   cos
    fmov    d6, #-1.0
    fadd    d2, d0, d6
    fmov    d6, #-0.5
    fmul    d2, d2, d6
    b       ease_eval.done
ease_eval.in_cubic:
    fmov    d1, #3.0
    b       ease_eval.power
ease_eval.out_expo:
    fmov    d2, #1.0
    fcmp    d5, d2
    b.eq    ease_eval.done
ease_eval.expo_out:
    fmov    d6, #-10.0
    fmul    d5, d5, d6
    fmov    d0, d5
    CCALL   exp2
    b       ease_eval.complement
ease_eval.out_circ:
    fmov    d6, #-1.0
    fadd    d5, d5, d6
    fmul    d5, d5, d5
    fmov    d0, #1.0
    fsub    d0, d0, d5
    fsqrt   d0, d0
    b       ease_eval.leave
ease_eval.out_quad:
    fmov    d2, #1.0
    fsub    d0, d2, d5
    fmul    d0, d0, d0
    fsub    d2, d2, d0
    b       ease_eval.done
ease_eval.in_out_circ:
    fmov    d0, #0.5
    fcmp    d0, d5
    fadd    d5, d5, d5
    b.le    ease_eval.circ_high
    fmul    d5, d5, d5
    fmov    d2, #1.0
    fsub    d0, d2, d5
    fsqrt   d0, d0
    b       ease_eval.complement_half
ease_eval.in_quint:
    fmov    d1, #5.0
    b       ease_eval.power
ease_eval.in_out_quad:
    fmov    d0, #0.5
    fcmp    d0, d5
    b.le    ease_eval.quad_high
    fmul    d5, d5, d5
    fadd    d5, d5, d5
    b       ease_eval.linear
ease_eval.in_out_expo:
    movi    d2, #0
    fcmp    d5, d2
    b.eq    ease_eval.done
ease_eval.expo_mid_one:
    fmov    d2, #1.0
    fcmp    d5, d2
    b.eq    ease_eval.done
ease_eval.expo_mid:
    fmov    d0, #0.5
    fcmp    d0, d5
    fmov    d6, #20.0
    fmul    d5, d5, d6
    b.le    ease_eval.expo_high
    fmov    d6, #-10.0
    fadd    d5, d5, d6
    fmov    d0, d5
    CCALL   exp2
    fmov    d2, d0
    b       ease_eval.half
ease_eval.in_quart:
    fmov    d1, #4.0
ease_eval.power:
    fmov    d0, d5
    CCALL   pow
    b       ease_eval.leave
ease_eval.out_cubic:
    fmov    d0, #1.0
    fsub    d0, d0, d5
    fmov    d1, #3.0
ease_eval.power_complement:
    CCALL   pow
ease_eval.complement:
    fmov    d2, #1.0
    fsub    d2, d2, d0
    b       ease_eval.done
ease_eval.in_out_bounce:
    fmov    d0, #0.5
    fcmp    d0, d5
    fadd    d5, d5, d5
    b.le    ease_eval.bounce_high
    fmov    d2, #1.0
    fsub    d0, d2, d5
    LDD     d1, ease_bounce_threshold_a
    fcmp    d1, d0
    b.le    ease_eval.bounce_low_second
    fmul    d0, d0, d0
    LDD     d6, ease_bounce_n
    fmul    d0, d0, d6
    b       ease_eval.complement_half
ease_eval.in_back:
    fmov    d1, #3.0
    fmov    d0, d5
    str     d5, [sp]
    CCALL   pow
    LDD     d6, ease_back_c3
    fmul    d0, d0, d6
    ldr     d2, [sp]
    fmul    d2, d2, d2
    LDD     d6, ease_neg_back_c1
    fmul    d2, d2, d6
    fadd    d2, d0, d2                  // pow term first: its NaN wins
    b       ease_eval.done
ease_eval.in_circ:
    fmul    d5, d5, d5
    fmov    d2, #1.0
    fsub    d0, d2, d5
    fsqrt   d0, d0
    fsub    d2, d2, d0
    b       ease_eval.done
ease_eval.out_back:
    fmov    d6, #-1.0
    fadd    d5, d5, d6
    str     d5, [sp]
    fmov    d1, #3.0
    fmov    d0, d5
    CCALL   pow
    LDD     d6, ease_back_c3
    fmul    d0, d0, d6
    fmov    d6, #1.0
    fadd    d0, d0, d6
    ldr     d2, [sp]
    fmul    d2, d2, d2
    LDD     d6, ease_back_c1
    fmul    d2, d2, d6
    fadd    d2, d2, d0
    b       ease_eval.done
ease_eval.in_bounce:
    fmov    d2, #1.0
    fsub    d0, d2, d5
    LDD     d1, ease_bounce_threshold_a
    fcmp    d1, d0
    b.le    ease_eval.bounce_in_second
    fmul    d0, d0, d0
    LDD     d6, ease_bounce_n
    fmul    d0, d0, d6
    fsub    d2, d2, d0
    b       ease_eval.done
ease_eval.out_bounce:
    LDD     d0, ease_bounce_threshold_a
    fcmp    d0, d5
    b.le    ease_eval.bounce_out_second
    fmul    d5, d5, d5
    LDD     d6, ease_bounce_n
    fmul    d5, d5, d6
    b       ease_eval.linear
ease_eval.out_elastic:
    movi    d2, #0
    fcmp    d5, d2
    b.eq    ease_eval.done
ease_eval.elastic_out_one:
    fmov    d2, #1.0
    fcmp    d5, d2
    b.eq    ease_eval.done
ease_eval.elastic_out:
    fmov    d0, #-10.0
    fmul    d0, d0, d5
    str     d5, [sp]
    CCALL   exp2
    str     d0, [sp, #16]
    ldr     d0, [sp]
    fmov    d6, #10.0
    fmul    d0, d0, d6
    fmov    d6, #-0.75
    fadd    d0, d0, d6
    LDD     d6, ease_elastic_c4
    fmul    d0, d0, d6
    CCALL   sin
    ldr     d6, [sp, #16]
    fmul    d2, d0, d6
    fmov    d6, #1.0
    fadd    d2, d2, d6
    b       ease_eval.done
ease_eval.quart_high:
    fadd    d5, d5, d5
    fmov    d0, #2.0
    fsub    d0, d0, d5
    fmov    d1, #4.0
    b       ease_eval.power_high
ease_eval.cubic_high:
    fadd    d5, d5, d5
    fmov    d0, #2.0
    fsub    d0, d0, d5
    fmov    d1, #3.0
    b       ease_eval.power_high
ease_eval.back_high:
    fmov    d6, #-2.0
    fadd    d2, d2, d6
    fmov    d0, d2
    LDD     d6, ease_back_c2_plus_one
    fmul    d2, d2, d6
    fmul    d0, d0, d0
    LDD     d6, ease_back_c2
    fadd    d2, d2, d6
    fmul    d2, d2, d0
    fmov    d6, #2.0
    fadd    d2, d2, d6
    b       ease_eval.half
ease_eval.quint_high:
    fadd    d5, d5, d5
    fmov    d0, #2.0
    fsub    d0, d0, d5
    fmov    d1, #5.0
ease_eval.power_high:
    CCALL   pow
    fmov    d2, d0
    b       ease_eval.negative_half_plus_one
ease_eval.circ_high:
    fmov    d0, #2.0
    fsub    d0, d0, d5
    fmul    d0, d0, d0
    fmov    d1, #1.0
    fsub    d2, d1, d0
    fsqrt   d2, d2
    fadd    d2, d2, d1
    b       ease_eval.half
ease_eval.quad_high:
    fadd    d5, d5, d5
    fmov    d2, #2.0
    fsub    d2, d2, d5
    fmul    d2, d2, d2
ease_eval.negative_half_plus_one:
    fmov    d6, #-0.5
    fmul    d2, d2, d6
    fmov    d6, #1.0
    fadd    d2, d2, d6
    b       ease_eval.done
ease_eval.bounce_high:
    fmov    d6, #-1.0
    fadd    d5, d5, d6
    LDD     d0, ease_bounce_threshold_a
    fcmp    d0, d5
    b.le    ease_eval.bounce_high_second
    fmul    d5, d5, d5
    LDD     d6, ease_bounce_n
    fmul    d5, d5, d6
    b       ease_eval.plus_one_half
ease_eval.bounce_in_second:
    LDD     d1, ease_bounce_threshold_b
    fcmp    d1, d0
    b.le    ease_eval.bounce_in_third
    LDD     d6, ease_bounce_shift_b
    fadd    d0, d0, d6
    fmul    d0, d0, d0
    LDD     d6, ease_bounce_n
    fmul    d0, d0, d6
    LDD     d6, ease_bounce_b
    fadd    d0, d0, d6
    fsub    d2, d2, d0
    b       ease_eval.done
ease_eval.bounce_out_second:
    LDD     d0, ease_bounce_threshold_b
    fcmp    d0, d5
    b.le    ease_eval.bounce_out_third
    LDD     d6, ease_bounce_shift_b
    fadd    d5, d5, d6
    fmul    d5, d5, d5
    LDD     d6, ease_bounce_n
    fmul    d5, d5, d6
    LDD     d6, ease_bounce_b
    fadd    d5, d5, d6
    b       ease_eval.linear
ease_eval.bounce_low_second:
    LDD     d1, ease_bounce_threshold_b
    fcmp    d1, d0
    b.le    ease_eval.bounce_low_third
    LDD     d6, ease_bounce_shift_b
    fadd    d0, d0, d6
    fmul    d0, d0, d0
    LDD     d6, ease_bounce_n
    fmul    d0, d0, d6
    LDD     d6, ease_bounce_b
    fadd    d0, d0, d6
    b       ease_eval.complement_half
ease_eval.bounce_high_second:
    LDD     d0, ease_bounce_threshold_b
    fcmp    d0, d5
    b.le    ease_eval.bounce_high_third
    LDD     d6, ease_bounce_shift_b
    fadd    d5, d5, d6
    fmul    d5, d5, d5
    LDD     d6, ease_bounce_n
    fmul    d5, d5, d6
    LDD     d6, ease_bounce_b
    fadd    d5, d5, d6
    b       ease_eval.plus_one_half
ease_eval.bounce_in_third:
    LDD     d1, ease_bounce_threshold_c
    fcmp    d1, d0
    b.le    ease_eval.bounce_in_last
    LDD     d6, ease_bounce_shift_c
    fadd    d0, d0, d6
    fmul    d0, d0, d0
    LDD     d6, ease_bounce_n
    fmul    d0, d0, d6
    LDD     d6, ease_bounce_c
    fadd    d0, d0, d6
    fsub    d2, d2, d0
    b       ease_eval.done
ease_eval.bounce_out_third:
    LDD     d0, ease_bounce_threshold_c
    fcmp    d0, d5
    b.le    ease_eval.bounce_out_last
    LDD     d6, ease_bounce_shift_c
    fadd    d5, d5, d6
    fmul    d5, d5, d5
    LDD     d6, ease_bounce_n
    fmul    d5, d5, d6
    LDD     d6, ease_bounce_c
    fadd    d5, d5, d6
    b       ease_eval.linear
ease_eval.bounce_low_third:
    LDD     d1, ease_bounce_threshold_c
    fcmp    d1, d0
    b.le    ease_eval.bounce_low_last
    LDD     d6, ease_bounce_shift_c
    fadd    d0, d0, d6
    fmul    d0, d0, d0
    LDD     d6, ease_bounce_n
    fmul    d0, d0, d6
    LDD     d6, ease_bounce_c
    fadd    d0, d0, d6
    b       ease_eval.complement_half
ease_eval.bounce_high_third:
    LDD     d0, ease_bounce_threshold_c
    fcmp    d0, d5
    b.le    ease_eval.bounce_high_last
    LDD     d6, ease_bounce_shift_c
    fadd    d5, d5, d6
    fmul    d5, d5, d5
    LDD     d6, ease_bounce_n
    fmul    d5, d5, d6
    LDD     d6, ease_bounce_c
    fadd    d5, d5, d6
    b       ease_eval.plus_one_half
ease_eval.bounce_in_last:
    LDD     d6, ease_bounce_shift_d
    fadd    d0, d0, d6
    fmul    d0, d0, d0
    LDD     d6, ease_bounce_n
    fmul    d0, d0, d6
    LDD     d6, ease_bounce_d
    fadd    d0, d0, d6
    fsub    d2, d2, d0
    b       ease_eval.done
ease_eval.bounce_out_last:
    LDD     d6, ease_bounce_shift_d
    fadd    d5, d5, d6
    fmul    d5, d5, d5
    LDD     d6, ease_bounce_n
    fmul    d5, d5, d6
    LDD     d6, ease_bounce_d
    fadd    d5, d5, d6
ease_eval.linear:
    fmov    d2, d5
    b       ease_eval.done
ease_eval.elastic_high:
    fmov    d0, #10.0
    ldr     d6, [sp, #16]
    fsub    d0, d0, d6
    CCALL   exp2
    ldr     d6, [sp, #32]
    fmul    d2, d0, d6
    fmov    d6, #0.5
    fmul    d2, d2, d6
    fmov    d6, #1.0
    fadd    d2, d2, d6
    b       ease_eval.done
ease_eval.expo_high:
    fmov    d0, #10.0
    fsub    d0, d0, d5
    CCALL   exp2
    fmov    d2, #2.0
    b       ease_eval.complement_half
ease_eval.bounce_low_last:
    LDD     d6, ease_bounce_shift_d
    fadd    d0, d0, d6
    fmul    d0, d0, d0
    LDD     d6, ease_bounce_n
    fmul    d0, d0, d6
    LDD     d6, ease_bounce_d
    fadd    d0, d0, d6
ease_eval.complement_half:
    fsub    d2, d2, d0
    b       ease_eval.half
ease_eval.bounce_high_last:
    LDD     d6, ease_bounce_shift_d
    fadd    d5, d5, d6
    fmul    d5, d5, d5
    LDD     d6, ease_bounce_n
    fmul    d5, d5, d6
    LDD     d6, ease_bounce_d
    fadd    d5, d5, d6
ease_eval.plus_one_half:
    fmov    d6, #1.0
    fadd    d2, d5, d6
ease_eval.half:
    fmov    d6, #0.5
    fmul    d2, d2, d6
ease_eval.done:
    fmov    d0, d2
ease_eval.leave:
    ldr     x30, [sp, #8]
    add     sp, sp, #48
    ret

    .section .rodata
    .balign 8
ease_table:
    .4byte ease_eval.linear - ease_table
    .4byte ease_eval.in_sine - ease_table
    .4byte ease_eval.out_sine - ease_table
    .4byte ease_eval.in_out_sine - ease_table
    .4byte ease_eval.in_quad - ease_table
    .4byte ease_eval.out_quad - ease_table
    .4byte ease_eval.in_out_quad - ease_table
    .4byte ease_eval.in_cubic - ease_table
    .4byte ease_eval.out_cubic - ease_table
    .4byte ease_eval.in_out_cubic - ease_table
    .4byte ease_eval.in_quart - ease_table
    .4byte ease_eval.out_quart - ease_table
    .4byte ease_eval.in_out_quart - ease_table
    .4byte ease_eval.in_quint - ease_table
    .4byte ease_eval.out_quint - ease_table
    .4byte ease_eval.in_out_quint - ease_table
    .4byte ease_eval.in_expo - ease_table
    .4byte ease_eval.out_expo - ease_table
    .4byte ease_eval.in_out_expo - ease_table
    .4byte ease_eval.in_circ - ease_table
    .4byte ease_eval.out_circ - ease_table
    .4byte ease_eval.in_out_circ - ease_table
    .4byte ease_eval.in_back - ease_table
    .4byte ease_eval.out_back - ease_table
    .4byte ease_eval.in_out_back - ease_table
    .4byte ease_eval.in_elastic - ease_table
    .4byte ease_eval.out_elastic - ease_table
    .4byte ease_eval.in_out_elastic - ease_table
    .4byte ease_eval.in_bounce - ease_table
    .4byte ease_eval.out_bounce - ease_table
    .4byte ease_eval.in_out_bounce - ease_table
    .balign 8
// The constants, as bit patterns (the ones fmov can encode are also used as
// immediates above; the values are the same).
ease_bounce_d: .8byte 0x3fef800000000000 // 0.984375
ease_neg_one: .8byte 0xbff0000000000000 // -1.0
ease_back_c2: .8byte 0x4004c25fe974a340 // 2.5949095
ease_four: .8byte 0x4010000000000000 // 4.0
ease_two: .8byte 0x4000000000000000 // 2.0
ease_back_twice_c2_plus_one: .8byte 0x401cc25fe974a340 // 7.189819
ease_bounce_threshold_c: .8byte 0x3fed1745d1745d17 // 0.9090909090909091
ease_bounce_shift_d: .8byte 0xbfee8ba2e8ba2e8c // -0.9545454545454546
ease_back_c3: .8byte 0x40059cd5f99c38b0 // 2.70158
ease_neg_ten: .8byte 0xc024000000000000 // -10.0
ease_bounce_shift_c: .8byte 0xbfea2e8ba2e8ba2f // -0.8181818181818182
ease_sixteen: .8byte 0x4030000000000000 // 16.0
ease_eight: .8byte 0x4020000000000000 // 8.0
ease_one: .8byte 0x3ff0000000000000 // 1.0
ease_neg_two: .8byte 0xc000000000000000 // -2.0
ease_bounce_threshold_a: .8byte 0x3fd745d1745d1746 // 0.36363636363636365
ease_bounce_b: .8byte 0x3fe8000000000000 // 0.75
ease_ten: .8byte 0x4024000000000000 // 10.0
ease_neg_half: .8byte 0xbfe0000000000000 // -0.5
ease_back_c1: .8byte 0x3ffb39abf3387161 // 1.70158
ease_five: .8byte 0x4014000000000000 // 5.0
ease_half: .8byte 0x3fe0000000000000 // 0.5
ease_elastic_c5: .8byte 0x3ff657184ae74487 // 1.3962634015954636
ease_neg_elastic_c4: .8byte 0xc000c152382d7365 // -2.0943951023931953
ease_bounce_threshold_b: .8byte 0x3fe745d1745d1746 // 0.7272727272727273
ease_back_c2_plus_one: .8byte 0x400cc25fe974a340 // 3.5949095
ease_three: .8byte 0x4008000000000000 // 3.0
ease_bounce_n: .8byte 0x401e400000000000 // 7.5625
ease_neg_three_quarters: .8byte 0xbfe8000000000000 // -0.75
ease_bounce_c: .8byte 0x3fee000000000000 // 0.9375
ease_neg_ten_three_quarters: .8byte 0xc025800000000000 // -10.75
ease_bounce_shift_b: .8byte 0xbfe1745d1745d174 // -0.5454545454545454
ease_elastic_c4: .8byte 0x4000c152382d7365 // 2.0943951023931953
ease_neg_back_c1: .8byte 0xbffb39abf3387161 // -1.70158
ease_twenty: .8byte 0x4034000000000000 // 20.0
ease_neg_eleven_eighth: .8byte 0xc026400000000000 // -11.125
ease_neg_back_c2: .8byte 0xc004c25fe974a340 // -2.5949095
ease_pi: .8byte 0x400921fb54442d18 // 3.141592653589793

// Caller-owned storage (alloc or embedded in an effect's state); the
// layouts are in defs.inc:
// EasingTracker: easing (id 0..30), total_steps (signed i64, including
//   zero/negative), clamp (0 or 1), current_step, then the f64 fields
//   progress_ratio, step_delta, eased_value, last_eased_value.
// SequenceStep: added (borrowed u64 slice: pointer, count), removed (always
//   in original sequence order), current (the current prefix).
// SequenceEaser: tracker, sequence (borrowed immutable u64 array), length,
//   result (a SequenceStep).

    .text

// easing_tracker_new(x0=storage, w1=id, x2=total_steps, w3=clamp)
// -> x9=storage. EasingTracker::new; clobbers x9, d0.
easing_tracker_new:
    mov     w9, w1
    str     x9, [x0, #EasingTracker.easing]
    str     x2, [x0, #EasingTracker.total_steps]
    mov     w9, w3
    str     x9, [x0, #EasingTracker.clamp]
    b       easing_tracker_reset

// easing_tracker_reset(x0=tracker) -> x9=tracker.
// EasingTracker::reset; clobbers x9, d0.
easing_tracker_reset:
    str     xzr, [x0, #EasingTracker.current_step]
    movi    d0, #0
    str     d0, [x0, #EasingTracker.progress_ratio]
    str     d0, [x0, #EasingTracker.step_delta]
    str     d0, [x0, #EasingTracker.eased_value]
    str     d0, [x0, #EasingTracker.last_eased_value]
    mov     x9, x0
    ret

// easing_tracker_is_complete(x0=tracker) -> w9=0/1.
// EasingTracker::is_complete; clobbers x9 (and x16).
easing_tracker_is_complete:
    ldr     x9, [x0, #EasingTracker.current_step]
    ldr     x16, [x0, #EasingTracker.total_steps]
    cmp     x9, x16
    cset    w9, ge
    ret

// easing_tracker_step(x0=tracker) -> d0=eased value.
// EasingTracker::step; clobbers all caller-saved registers.
// Completed trackers retain their last delta, ratio and value.
easing_tracker_step:
    PUSH2   x19, x30
    mov     x19, x0
    ldr     x9, [x19, #EasingTracker.current_step]
    ldr     x10, [x19, #EasingTracker.total_steps]
    cmp     x9, x10
    b.ge    easing_tracker_step.done
    add     x9, x9, #1
    str     x9, [x19, #EasingTracker.current_step]
    scvtf   d0, x9
    scvtf   d1, x10
    fdiv    d0, d0, d1
    str     d0, [x19, #EasingTracker.progress_ratio]
    ldr     w0, [x19, #EasingTracker.easing]
    bl      ease
    ldr     x9, [x19, #EasingTracker.clamp]
    cbz     x9, easing_tracker_step.value
    // min(1.0).max(0.0) as the aarch64 oracle compiles it: fminnm/fmaxnm
    // ignore a NaN operand, so NaN -> 1 as with minsd on x86
    fminnm  d0, d0, d0
    fmov    d1, #1.0
    fminnm  d0, d0, d1
    movi    d1, #0
    fmaxnm  d0, d0, d1
easing_tracker_step.value:
    str     d0, [x19, #EasingTracker.eased_value]
    ldr     d1, [x19, #EasingTracker.last_eased_value]
    fsub    d1, d0, d1
    str     d1, [x19, #EasingTracker.step_delta]
    str     d0, [x19, #EasingTracker.last_eased_value]
easing_tracker_step.done:
    ldr     d0, [x19, #EasingTracker.eased_value]
    POP2    x19, x30
    ret

// sequence_easer_new(x0=storage, x1=u64 array, x2=count, w3=id,
//                    x4=total_steps) -> x9=storage.
// SequenceEaser::new. The array must outlive the easer; no allocation/copy.
// Clobbers x9, x3, x2, x1, d0. Array/count may be replaced before reset
// (Sweep's second phase). Counts must describe a valid u64 array.
sequence_easer_new:
    str     x1, [x0, #SequenceEaser.sequence]
    str     x2, [x0, #SequenceEaser.length]
    mov     w1, w3
    mov     x2, x4
    mov     w3, #1
    str     x30, [sp, #-16]!
    bl      easing_tracker_new
    ldr     x30, [sp], #16
    b       sequence_easer_clear_result

// sequence_easer_reset(x0=easer) -> x9=easer.
// SequenceEaser::reset; clears the borrowed result, retaining array/config.
// Clobbers x9, x3, d0.
sequence_easer_reset:
    str     x30, [sp, #-16]!
    bl      easing_tracker_reset
    ldr     x30, [sp], #16
sequence_easer_clear_result:
    ldr     x3, [x0, #SequenceEaser.sequence]
    stp     x3, xzr, [x0, #(SequenceEaser.result + SequenceStep.added)]
    stp     x3, xzr, [x0, #(SequenceEaser.result + SequenceStep.removed)]
    stp     x3, xzr, [x0, #(SequenceEaser.result + SequenceStep.current)]
    ret

// sequence_easer_is_complete(x0=easer) -> w9=0/1; clobbers x9 (and x16).
sequence_easer_is_complete:
    b       easing_tracker_is_complete

// sequence_easer_step(x0=easer) -> x9=&easer.result (SequenceStep).
// SequenceEaser::step, plus the current prefix for effect consumers.
// Result is valid until the next step/reset. Slices borrow the input array;
// a zero count means empty (including when the array pointer is null).
// Clobbers all caller-saved registers. No per-frame allocation.
sequence_easer_step:
    stp     x19, x30, [sp, #-32]!
    mov     x19, x0
    ldr     d0, [x19, #EasingTracker.eased_value]
    str     d0, [sp, #16]
    bl      easing_tracker_step
    // The clamp keeps both products finite and in [0, length], and fcvtzs
    // is Rust's saturating cast anyway.
    ldr     x9, [x19, #SequenceEaser.length]
    ucvtf   d1, x9
    fmul    d0, d0, d1
    fcvtzs  x2, d0                      // new prefix length
    ldr     d6, [sp, #16]
    fmul    d1, d1, d6
    fcvtzs  x3, d1                      // previous prefix length
    ldr     x1, [x19, #SequenceEaser.sequence]
    add     x9, x19, #SequenceEaser.result
    stp     x1, x2, [x9, #SequenceStep.current]
    stp     x1, xzr, [x9, #SequenceStep.added]
    stp     x1, xzr, [x9, #SequenceStep.removed]
    cmp     x2, x3
    b.eq    sequence_easer_step.done
    b.lo    sequence_easer_step.removed
    add     x1, x1, x3, lsl #3
    sub     x2, x2, x3
    stp     x1, x2, [x9, #SequenceStep.added]
    b       sequence_easer_step.done
sequence_easer_step.removed:
    add     x1, x1, x2, lsl #3
    sub     x3, x3, x2
    stp     x1, x3, [x9, #SequenceStep.removed]
sequence_easer_step.done:
    ldp     x19, x30, [sp], #32
    ret

// ------------------------------------------------ Easing::CubicBezier curves
// A curve's four parameters live in a per-run table; its easing id is
// EASE_BEZIER_BASE + its index, so scenes and paths carry it like a named
// easing and ease() dispatches it. The table is a ring of BEZIER_RING
// entries (thunderstorm makes a curve per strike, forever): ids stay unique,
// so memos keyed by id never go stale, and an id stays valid while fewer
// than BEZIER_RING curves are made after it.

// bezier_new(d0=x1, d1=y1, d2=x2, d3=y2) -> w9 = easing id.
// Easing::CubicBezier(x1, y1, x2, y2). Clobbers the C caller-saved set.
bezier_new:
    stp     x19, x30, [sp, #-48]!
    stp     d0, d1, [sp, #16]
    stp     d2, d3, [sp, #32]
    LDX     x9, bezier_table
    cbnz    x9, bezier_new.have_table
    mov     x0, #(BEZIER_RING * 32)
    bl      reserve
    STX     x9, bezier_table
bezier_new.have_table:
    LDW     w19, bezier_count
    mov     w10, #BEZIER_LIMIT
    cmp     w19, w10
    b.hs    bezier_new.full
    add     w10, w19, #1
    STW     w10, bezier_count
    ubfiz   x9, x19, #5, #12            // (count & (BEZIER_RING - 1)) * 32
    LDX     x10, bezier_table
    add     x9, x9, x10
    ldp     x10, x11, [sp, #16]
    stp     x10, x11, [x9]
    ldp     x10, x11, [sp, #32]
    stp     x10, x11, [x9, #16]
    add     w9, w19, #EASE_BEZIER_BASE
    ldp     x19, x30, [sp], #48
    ret
bezier_new.full:
    ADRG    x0, msg_bezier_full
    mov     w1, #msg_bezier_full_len
    b       fatal

// ease_bezier_id(w0=id >= EASE_BEZIER_BASE, d0=t): ease() for a curve.
// The last (id, t) is memoized, like upstream's lru_cache: scenes started
// together (thunderstorm's flash on every text character) ask for the same
// point of the same curve one after another.
ease_bezier_id:
    fmov    x9, d0
    LDW     w10, bezier_memo_id
    cmp     w0, w10
    b.ne    ease_bezier_id.miss
    LDX     x10, bezier_memo_t
    cmp     x9, x10
    b.ne    ease_bezier_id.miss
    LDD     d0, bezier_memo_value
    ret
ease_bezier_id.miss:
    STW     w0, bezier_memo_id
    STX     x9, bezier_memo_t
    sub     w0, w0, #EASE_BEZIER_BASE
    and     w0, w0, #(BEZIER_RING - 1)
    LDX     x10, bezier_table
    add     x0, x10, x0, lsl #5
    str     x30, [sp, #-16]!
    bl      bezier_easing
    ldr     x30, [sp], #16
    STD     d0, bezier_memo_value
    ret

// bezier_easing(x0=&[x1, y1, x2, y2], d0=progress) -> d0.
// easing::bezier_easing: Newton-Raphson on x (20 iterations, 1e-5
// convergence, 1e-6 derivative bail), then y at the solved t. The oracle
// squares with fmul and cubes with pow(t, 3.0); products and sums keep the
// source's grouping. Clobbers the C caller-saved set.
bezier_easing:
    movi    d1, #0
    fcmp    d1, d0                      // progress <= 0 -> 0
    b.ge    bezier_easing.zero
    fmov    d1, #1.0
    fcmp    d0, d1                      // progress >= 1 -> 1
    b.ge    bezier_easing.one
    sub     sp, sp, #112
    stp     x19, x30, [sp, #96]
    // [sp] progress, +8 t, +16 1-t, +24 (1-t)^2, +32 t^2, +40 partial sum
    // +48 x1, +56 y1, +64 x2, +72 y2, +80 .powers' x30
    str     d0, [sp]
    str     d0, [sp, #8]
    ldp     x9, x10, [x0]
    stp     x9, x10, [sp, #48]
    ldp     x9, x10, [x0, #16]
    stp     x9, x10, [sp, #64]
    mov     w19, #20
bezier_easing.newton:
    bl      bezier_easing.powers        // d0 = t^3
    // x(t) = 3 x1 (1-t)^2 t + 3 x2 (1-t) t^2 + t^3
    fmov    d1, #3.0
    ldr     d6, [sp, #48]
    fmul    d1, d1, d6
    ldr     d6, [sp, #24]
    fmul    d1, d1, d6
    ldr     d6, [sp, #8]
    fmul    d1, d1, d6
    fmov    d2, #3.0
    ldr     d6, [sp, #64]
    fmul    d2, d2, d6
    ldr     d6, [sp, #16]
    fmul    d2, d2, d6
    ldr     d6, [sp, #32]
    fmul    d2, d2, d6
    fadd    d1, d1, d2
    fadd    d1, d1, d0
    ldr     d6, [sp]
    fsub    d1, d1, d6                  // dx
    fabs    d3, d1
    LDD     d2, bezier_x_epsilon
    fcmp    d2, d3
    b.gt    bezier_easing.solved
    // x'(t) = 3 (1-t)^2 x1 + 6 (1-t) t (x2 - x1) + 3 t^2 (1 - x2)
    fmov    d4, #3.0
    ldr     d6, [sp, #24]
    fmul    d4, d4, d6
    ldr     d6, [sp, #48]
    fmul    d4, d4, d6
    LDD     d5, bezier_six
    ldr     d6, [sp, #16]
    fmul    d5, d5, d6
    ldr     d6, [sp, #8]
    fmul    d5, d5, d6
    ldr     d2, [sp, #64]
    ldr     d6, [sp, #48]
    fsub    d2, d2, d6
    fmul    d5, d5, d2
    fadd    d4, d4, d5
    fmov    d5, #3.0
    ldr     d6, [sp, #32]
    fmul    d5, d5, d6
    fmov    d2, #1.0
    ldr     d6, [sp, #64]
    fsub    d2, d2, d6
    fmul    d5, d5, d2
    fadd    d4, d4, d5
    fabs    d3, d4
    LDD     d2, bezier_d_epsilon
    fcmp    d2, d3
    b.gt    bezier_easing.solved
    fdiv    d1, d1, d4
    ldr     d2, [sp, #8]
    fsub    d2, d2, d1
    str     d2, [sp, #8]
    subs    w19, w19, #1
    b.ne    bezier_easing.newton
    bl      bezier_easing.powers
bezier_easing.solved:
    // y(t) = 3 y1 (1-t)^2 t + 3 y2 (1-t) t^2 + t^3, d0 = t^3 at this t
    fmov    d1, #3.0
    ldr     d6, [sp, #56]
    fmul    d1, d1, d6
    ldr     d6, [sp, #24]
    fmul    d1, d1, d6
    ldr     d6, [sp, #8]
    fmul    d1, d1, d6
    fmov    d2, #3.0
    ldr     d6, [sp, #72]
    fmul    d2, d2, d6
    ldr     d6, [sp, #16]
    fmul    d2, d2, d6
    ldr     d6, [sp, #32]
    fmul    d2, d2, d6
    fadd    d1, d1, d2
    fadd    d1, d1, d0
    fmov    d0, d1
    ldp     x19, x30, [sp, #96]
    add     sp, sp, #112
    ret
bezier_easing.powers:
    // 1-t, (1-t)^2, t^2 into the caller's frame (sp is unchanged by bl),
    // t^3 in d0; this return address waits in the frame's slot at +80
    str     x30, [sp, #80]
    ldr     d0, [sp, #8]
    fmov    d1, #1.0
    fsub    d1, d1, d0
    str     d1, [sp, #16]
    fmul    d1, d1, d1
    str     d1, [sp, #24]
    fmul    d1, d0, d0
    str     d1, [sp, #32]
    fmov    d1, #3.0
    CCALL   pow
    ldr     x30, [sp, #80]
    ret
bezier_easing.zero:
    movi    d0, #0
    ret
bezier_easing.one:
    fmov    d0, #1.0
    ret

    .section .rodata
    .balign 16
bezier_abs_mask:    .8byte 0x7fffffffffffffff, 0x7fffffffffffffff  // (fabs here)
bezier_six:         .double 6.0
bezier_x_epsilon:   .double 1e-5
bezier_d_epsilon:   .double 1e-6
STRING msg_bezier_full, "ttfx: asm engine: bezier easing limit reached\n"

    TSTATE
    .balign 8
ease_memo:          .skip 8
bezier_table:       .skip 8
bezier_memo_t:      .skip 8
bezier_memo_value:  .skip 8
bezier_count:       .skip 4
bezier_memo_id:     .skip 4             // 0 (no curve) until the first call
