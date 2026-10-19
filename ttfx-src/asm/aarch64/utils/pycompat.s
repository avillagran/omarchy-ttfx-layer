// utils/pycompat.s - Python semantics where they differ from Rust's
// (src/utils/pycompat.rs): round(), // and %. Also Rust's own saturating
// float-to-integer cast. On aarch64 fcvtzs is exactly Rust's `as i64` and
// fcvtns is round-half-even with the same saturation, so the inline
// macros (F64_TO_I64, ROUND_HALF_EVEN) live in ttfx.inc and need no slow
// path; these functions are for callers that want a call.

    .text

// f64_to_i64(d0) -> x9 = d0 as i64 with Rust semantics: truncation,
// saturation at both ends, NaN -> 0.
f64_to_i64:
    fcvtzs  x9, d0
    ret

// round_half_even(d0) -> x9: Python round() to i64 (pycompat.rs). Finite
// values round to nearest even and saturate like `as i64`; NaN -> 0,
// -inf -> i64::MIN, and +inf -> i64::MAX + 1, which wraps to i64::MIN
// exactly as the oracle's release build does. Clobbers x16, x17.
round_half_even:
    ROUND_HALF_EVEN
    ret

// floor_div(x0=a, x1=b) -> x9 = a // b (Python floor division).
// b must be nonzero. Clobbers x2.
floor_div:
    sdiv    x9, x0, x1
    msub    x2, x9, x1, x0              // remainder
    cbz     x2, floor_div.done
    eor     x2, x2, x1                  // remainder and divisor differ in sign
    tbz     x2, #63, floor_div.done
    sub     x9, x9, #1
floor_div.done:
    ret

// py_mod(x0=a, x1=b) -> x9 = a % b with the divisor's sign (Python %).
// b must be nonzero. Clobbers x2, x3.
py_mod:
    sdiv    x2, x0, x1
    msub    x9, x2, x1, x0
    cbz     x9, py_mod.done
    eor     x3, x9, x1
    tbz     x3, #63, py_mod.done
    add     x9, x9, x1
py_mod.done:
    ret

// dropped: pc_below_2p63 (fcvtzs saturates by itself)
