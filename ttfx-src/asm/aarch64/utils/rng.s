// utils/rng.s - xoshiro256++ with the Python-shaped helpers of
// src/utils/rng.rs. Every helper consumes draws exactly as the Rust one does;
// that draw order is the parity contract.
//
// The generator runs ahead in batches with its state in registers, and draws
// are served from the batch. The raw sequence does not depend on how it is
// consumed, so this is exactly the Rust sequence without a store-to-load
// round trip through the state on every draw.
//
// aarch64 has the one tier: a batch is RNG_BATCH (512) consecutive draws of
// a single lane (RNG_LANES = 1, RNG_LANE = RNG_BATCH; defs.inc).
// dropped: rng_lanes (TIER 4 lane end states: no 8-lane generator here)
// dropped: rng_lanes_ok (TIER 4 only)
// dropped: rng_idx_lo (TIER 4 transpose indices)
// dropped: rng_idx_hi (TIER 4 transpose indices)
// dropped: rng_bits (TIER 4 jump-matrix bit masks)
// dropped: rng_jump (TIER 4 jump matrix, utils/rng_jump.inc)
// dropped: two_pow_minus_53 (rng_random scales with ucvtf #53, exactly the same product)
//
// The draw macros (RNG_OPEN, RNG_TAKE, RNG_TAKE53, RNG_CLOSE, RNG_BITS53)
// live in ttfx.inc.

    .text

// rng_seed(x0=seed): SplitMix64 expansion into the xoshiro state.
// Clobbers x0, x1, x3-x5, x9, x10.
rng_seed:
    MOV64   x1, 0x9E3779B97F4A7C15
    MOV64   x4, 0xBF58476D1CE4E5B9
    MOV64   x5, 0x94D049BB133111EB
    ADRG    x10, rng_state
    mov     w3, #0
rng_seed.next:
    add     x0, x0, x1
    mov     x9, x0
    eor     x9, x9, x9, lsr #30
    mul     x9, x9, x4
    eor     x9, x9, x9, lsr #27
    mul     x9, x9, x5
    eor     x9, x9, x9, lsr #31
    str     x9, [x10, x3, lsl #3]
    add     w3, w3, #1
    cmp     w3, #4
    b.lo    rng_seed.next
    mov     x9, #RNG_BATCH              // the first draw generates a batch
    STX     x9, rng_pos
    ret

// rng_load(x0=4 x u64 state): continue from a state handed in by Rust.
// Clobbers x9, x10.
rng_load:
    ldp     x9, x10, [x0]
    ADRG    x16, rng_state
    stp     x9, x10, [x16]
    ldp     x9, x10, [x0, #16]
    stp     x9, x10, [x16, #16]
    mov     x9, #RNG_BATCH
    STX     x9, rng_pos
    ret

// rng_store(x0=4 x u64 out): the logical state - as if every draw so far
// had been taken one at a time, which is where Rust's generator would be.
// Clobbers x1-x5, x9-x11 (x0 is preserved).
rng_store:
    LDX     x3, rng_pos
    cmp     x3, #RNG_BATCH
    b.lo    rng_store.partial
    ADRG    x1, rng_state
    b       rng_store.copy
rng_store.partial:
    // replay the consumed part of the batch from the batch's starting state
    ADRG    x9, rng_batch_start
    ldp     x4, x5, [x9]
    ldp     x10, x2, [x9, #16]
rng_store.step:
    cbz     x3, rng_store.stepped
    lsl     x11, x5, #17
    eor     x10, x10, x4
    eor     x2, x2, x5
    eor     x5, x5, x10
    eor     x4, x4, x2
    eor     x10, x10, x11
    ror     x2, x2, #19                 // rotl 45
    sub     x3, x3, #1
    b       rng_store.step
rng_store.stepped:
    ADRG    x1, rng_scratch
    stp     x4, x5, [x1]
    stp     x10, x2, [x1, #16]
rng_store.copy:
    ldp     x9, x10, [x1]
    stp     x9, x10, [x0]
    ldp     x9, x10, [x1, #16]
    stp     x9, x10, [x0, #16]
    ret

// rng_next -> x9: the next xoshiro256++ output.
// Clobbers x3 only (and x16, x17).
rng_next:
    LDX     x3, rng_pos
    cmp     x3, #RNG_BATCH
    b.hs    rng_next.refill
rng_next.take:
    ADRG    x9, rng_buf
    ldr     x9, [x9, x3, lsl #3]
    add     x3, x3, #1
    STX     x3, rng_pos
    ret
rng_next.refill:
    str     x30, [sp, #-16]!
    bl      rng_refill
    ldr     x30, [sp], #16
    mov     x3, #0
    b       rng_next.take

// rng_refill: generate the next batch into rng_buf (rng_pos is left to the
// caller, which starts the batch at 0). Preserves every register but x16,
// x17 and the flags, so the draw macros can call it from anywhere; touches
// no vector register. A leaf: the caller saved x30.
rng_refill:
    stp     x0, x1, [sp, #-48]!
    stp     x2, x3, [sp, #16]
    stp     x4, x5, [sp, #32]
    // x0-x3 = s0..s3
    ADRG    x4, rng_state
    ldp     x0, x1, [x4]
    ldp     x2, x3, [x4, #16]
    ADRG    x5, rng_batch_start
    stp     x0, x1, [x5]
    stp     x2, x3, [x5, #16]
    ADRG    x4, rng_buf
    add     x5, x4, #(RNG_LANE * 8)
rng_refill.generate:
    // four draws per pass (RNG_LANE is a multiple of 4). The state update
    // is two xors deep a step, the output a side chain off it.
    .irp i, 0, 1, 2, 3
    add     x16, x0, x3
    ror     x16, x16, #41               // rotl 23
    add     x16, x16, x0                // result
    str     x16, [x4, #(\i * 8)]
    lsl     x17, x1, #17                // t
    eor     x2, x2, x0                  // s2 ^= s0
    eor     x3, x3, x1                  // s3 ^= s1
    eor     x1, x1, x2                  // s1 ^= s2
    eor     x0, x0, x3                  // s0 ^= s3
    eor     x2, x2, x17                 // s2 ^= t
    ror     x3, x3, #19                 // s3 = rotl(s3, 45)
    .endr
    add     x4, x4, #32
    cmp     x4, x5
    b.lo    rng_refill.generate
    ADRG    x4, rng_state
    stp     x0, x1, [x4]
    stp     x2, x3, [x4, #16]
    ldp     x4, x5, [sp, #32]
    ldp     x2, x3, [sp, #16]
    ldp     x0, x1, [sp], #48
    ret

// rng_below(x0=n > 0) -> x9 in [0, n): bit-mask rejection sampling.
// n == 1 still draws (and may reject) exactly like Rust's randbelow.
// Clobbers x2-x5, x10 (and x16, x17); x0, x1, x11 are preserved.
rng_below:
    // shift = 64 - bits.max(1), bits = 64 - (n - 1).leading_zeros()
    sub     x9, x0, #1
    clz     x3, x9                      // 64 for n == 1
    mov     x10, #63
    cmp     x3, #63
    csel    x3, x3, x10, lo
    // the position stays in x2 through the rejection loop. Rejections are
    // unpredictable (up to half the draws), so four draws are tested at
    // once without branches and the first accepted one is taken; only four
    // rejections in a row loop.
    ADRG    x4, rng_buf
    LDX     x2, rng_pos
rng_below.again:
    cmp     x2, #(RNG_BATCH - 4)
    b.hi    rng_below.single
    add     x10, x4, x2, lsl #3
    mov     w5, #0                      // bit i: draw i is rejected
    .irp i, 3, 2, 1, 0
    ldr     x9, [x10, #(\i * 8)]
    lsr     x9, x9, x3
    cmp     x9, x0                      // C = rejected
    adc     w5, w5, w5
    .endr
    eor     w5, w5, #0xf                // bit i: draw i is accepted
    cbz     w5, rng_below.rejected
    rbit    w5, w5
    clz     w5, w5
    add     x2, x2, x5
    ldr     x9, [x4, x2, lsl #3]
    add     x2, x2, #1
    lsr     x9, x9, x3
    STX     x2, rng_pos
    ret
rng_below.rejected:
    add     x2, x2, #4
    b       rng_below.again
rng_below.single:
    // the batch's last draws, one at a time
    cmp     x2, #RNG_BATCH
    b.hs    rng_below.refill
    ldr     x9, [x4, x2, lsl #3]
    add     x2, x2, #1
    lsr     x9, x9, x3
    cmp     x9, x0
    b.hs    rng_below.again
    STX     x2, rng_pos
    ret
rng_below.refill:
    str     x30, [sp, #-16]!
    bl      rng_refill
    ldr     x30, [sp], #16
    mov     x2, #0
    b       rng_below.again

// rng_randint(x0=a, x1=b) -> x9 in [a, b].
// Clobbers x0, x1 and what rng_below clobbers.
rng_randint:
    PUSH2   x19, x30
    mov     x19, x0
    sub     x1, x1, x0
    add     x0, x1, #1
    bl      rng_below
    add     x9, x9, x19
    POP2    x19, x30
    ret

// rng_randrange(x0=a, x1=b) -> x9 in [a, b).
// Clobbers x0, x1 and what rng_below clobbers.
rng_randrange:
    PUSH2   x19, x30
    mov     x19, x0
    sub     x1, x1, x0
    mov     x0, x1
    bl      rng_below
    add     x9, x9, x19
    POP2    x19, x30
    ret

// rng_random -> d0 in [0, 1): (next >> 11) * 2^-53, Python random().
// ucvtf with 53 fraction bits is that product exactly (next >> 11 is below
// 2^53, so both the conversion and the scaling are exact).
// Clobbers x3, x9 (and x16, x17); no vector register but d0.
rng_random:
    str     x30, [sp, #-16]!
    bl      rng_next
    ldr     x30, [sp], #16
    lsr     x9, x9, #11
    ucvtf   d0, x9, #53
    ret

// rng_threshold(d0=c >= 0) -> x9: the T with random() < c exactly when
// RNG_BITS53 < T, i.e. ceil(c * 2^53) capped at 2^53. Clobbers d0, d1.
rng_threshold:
    LDD     d1, two_pow_53
    fmul    d0, d0, d1
    fcmp    d0, d1                      // minsd: NaN -> 2^53
    fcsel   d0, d0, d1, mi
    frintp  d0, d0                      // toward +inf
    fcvtzs  x9, d0
    ret

// rng_uniform(d0=a, d1=b) -> d0 = a + (b - a) * random().
rng_uniform:
    stp     d0, d1, [sp, #-32]!
    str     x30, [sp, #16]
    bl      rng_random
    ldr     x30, [sp, #16]
    ldp     d2, d1, [sp], #32           // d2 = a, d1 = b
    fsub    d1, d1, d2
    fmul    d0, d0, d1
    fadd    d0, d0, d2
    ret

// rng_shuffle32(x0=u32 array, x1=length) / rng_shuffle64(x0=u64 array,
// x1=length): Fisher-Yates from the top, CPython's loop
// (for i in reversed(range(1, n)): j = randbelow(i + 1); swap).
rng_shuffle32:
    PUSH2   x19, x21
    PUSH1   x30
    mov     x19, x0
    mov     x21, x1
rng_shuffle32.next:
    subs    x21, x21, #1
    b.le    rng_shuffle32.done
    add     x0, x21, #1
    bl      rng_below
    ldr     w3, [x19, x21, lsl #2]
    ldr     w2, [x19, x9, lsl #2]
    str     w2, [x19, x21, lsl #2]
    str     w3, [x19, x9, lsl #2]
    b       rng_shuffle32.next
rng_shuffle32.done:
    POP1    x30
    POP2    x19, x21
    ret

rng_shuffle64:
    PUSH2   x19, x21
    PUSH1   x30
    mov     x19, x0
    mov     x21, x1
rng_shuffle64.next:
    subs    x21, x21, #1
    b.le    rng_shuffle64.done
    add     x0, x21, #1
    bl      rng_below
    ldr     x3, [x19, x21, lsl #3]
    ldr     x2, [x19, x9, lsl #3]
    str     x2, [x19, x21, lsl #3]
    str     x3, [x19, x9, lsl #3]
    b       rng_shuffle64.next
rng_shuffle64.done:
    POP1    x30
    POP2    x19, x21
    ret

    .section .rodata
    .balign 8
two_pow_53:         .8byte 0x4340000000000000   // 1 << 53

    TSTATE
    .balign 64
rng_buf:            .skip 8 * RNG_BATCH
rng_state:          .skip 8 * 4
rng_batch_start:    .skip 8 * 4
rng_scratch:        .skip 8 * 4
rng_pos:            .skip 8
