#![cfg(all(ttfx_asm, any(target_arch = "x86_64", target_arch = "aarch64")))]

//! Differential gate for the engine's audio color transform: the assembly
//! audio_tint (SSE2 on x86-64, scalar f32 on aarch64) must match the fx
//! engine's utils/ansi.rs recolor bit for bit over colors x parameter sets,
//! including saturation, negative coefficients and non-finite values.

extern "C" {
    /// asm thunk: loads the ten f32 into the engine's audio_params slot and
    /// runs audio_tint on the packed 0xRRGGBB color.
    fn ttfx_test_audio_tint(color: u64, params: *const f32) -> u32;
}

/// The reference math, in the same left-to-right f32 operation order as
/// utils/ansi.rs recolor: (matrix . rgb) * brightness, clamp 0-255, truncate.
fn oracle(color: u64, params: &[f32; 10]) -> u32 {
    let r = ((color >> 16) & 255) as f32;
    let g = ((color >> 8) & 255) as f32;
    let b = (color & 255) as f32;
    let bright = params[0];
    let m = &params[1..];
    let rr = (m[0] * r + m[1] * g + m[2] * b) * bright;
    let gg = (m[3] * r + m[4] * g + m[5] * b) * bright;
    let bb = (m[6] * r + m[7] * g + m[8] * b) * bright;
    (((rr.clamp(0.0, 255.0) as u32) << 16)
        | ((gg.clamp(0.0, 255.0) as u32) << 8)
        | (bb.clamp(0.0, 255.0) as u32)) as u32
}

const COLORS: [u64; 12] = [
    0x000000, 0xffffff, 0xff0000, 0x00ff00, 0x0000ff, 0xcccccc, 0x010203, 0x7f8081, 0x123456,
    0xdeadbe, 0xfedcba, 0x808080,
];

fn parameter_sets() -> Vec<[f32; 10]> {
    let identity = [
        1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let mut sets = vec![
        identity,
        [0.5, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        [1.5, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        [3.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        // A hue rotation like the live bridge computes (all nine coefficients
        // nontrivial), at two brightness levels.
        [
            1.275, 0.9587, 0.0203, 0.0210, 0.0210, 0.9587, 0.0203, 0.0203, 0.0210, 0.9587,
        ],
        [
            0.85, 0.5, 0.25, 0.25, 0.25, 0.5, 0.25, 0.25, 0.25, 0.5,
        ],
        // Negative and mixed-sign coefficients exercise the 0.0 clamp.
        [-0.25, 1.0, -0.5, 0.0, 0.0, 1.0, -0.25, 0.5, -1.0, 0.0],
        // Huge magnitudes saturate at 255.
        [1.0e30, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        // Non-finite brightness and coefficients: NaN clamps to 0 like the
        // SSE2 maxss(value, 0), infinities saturate.
        [f32::NAN, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        [f32::INFINITY, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        [f32::NEG_INFINITY, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        [
            1.0, f32::NAN, 0.0, 0.0, 0.0, f32::INFINITY, 0.0, 0.0, 0.0, f32::NEG_INFINITY,
        ],
    ];
    // 60-degree hue rotations at fractional brightness, from the same formula
    // the embedding bridge uses (cos/sin at two decimal places is enough to
    // mix channels nontrivially).
    for (bright, hue_deg) in [(1.1f32, 30.0f32), (1.35, 120.0), (0.7, 240.0)] {
        let (sin, cos) = hue_deg.to_radians().sin_cos();
        let t = (1.0 - cos) / 3.0;
        let v = 0.57735026 * sin;
        sets.push([
            bright,
            cos + t,
            t - v,
            t + v,
            t + v,
            cos + t,
            t - v,
            t - v,
            t + v,
            cos + t,
        ]);
    }
    let _ = identity;
    sets
}

#[test]
fn assembly_audio_tint_matches_the_rust_recolor_bit_for_bit() {
    assert!(ttfx::asm::tier() > 0, "must execute the real assembly engine");
    let mut cases = 0usize;
    for params in parameter_sets() {
        for &color in &COLORS {
            let expected = oracle(color, &params);
            // SAFETY: params points to ten readable f32; the thunk only reads
            // them and returns the tinted color. No engine run is active.
            let actual = unsafe { ttfx_test_audio_tint(color, params.as_ptr()) };
            assert_eq!(
                actual, expected,
                "audio_tint mismatch for color {color:#08x} params {params:?}: \
                 asm {actual:#08x} vs oracle {expected:#08x}"
            );
            cases += 1;
        }
    }
    assert!(cases >= 12 * 15, "the corpus must stay meaningful: {cases}");
}
