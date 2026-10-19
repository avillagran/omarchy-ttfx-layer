// utils/color.s - Color arithmetic on the u64 color format of ttfx.inc:
// Animation::adjust_color_brightness (src/engine/animation.rs) and
// graphics::shift_color_towards / random_color (src/utils/graphics.rs).
// Inputs may carry an xterm code; results are plain RGB colors, as Rust's
// Color::from_rgb makes them.
//
// Every comparison, select and expression is in the oracle's order. The
// aarch64 oracle compiles f64::max/min to fmaxnm/fminnm, used here in the
// same operand order; float comparisons take the oracle's (Rust's) branch
// on NaN, though no input here makes one.

    .text

// adjust_color_brightness(x0=color, d0=brightness) -> x9 = color.
// RGB -> HLS, lightness scaled by brightness and clamped to [0, 1], back to
// RGB with hue_to_rgb, channels rounded half-even and truncated to u8.
// Clobbers x16, x17, d0-d15.
adjust_color_brightness:
    PUSH2   x19, x30
    LDD     d15, col_255                // 255.0 throughout
    ubfx    w9, w0, #16, #8
    scvtf   d10, w9
    fdiv    d10, d10, d15               // normalized_red
    ubfx    w9, w0, #8, #8
    scvtf   d2, w9
    fdiv    d2, d2, d15                 // normalized_green
    and     w9, w0, #0xff
    scvtf   d3, w9
    fdiv    d3, d3, d15                 // normalized_blue
    fmaxnm  d5, d10, d2
    fmaxnm  d5, d5, d3                  // max_val
    fminnm  d1, d10, d2
    fminnm  d1, d1, d3                  // min_val
    fadd    d4, d5, d1                  // max + min
    fmov    d7, #0.5
    fmul    d7, d7, d4                  // lightness
    fmul    d0, d0, d7                  // lightness * brightness
    fcmp    d5, d1
    b.ne    adjust_color_brightness.chroma
    // max == min: hue and saturation are 0, so the result is gray
    fmov    d2, #1.0
    fminnm  d0, d0, d2
    movi    d2, #0
    fmaxnm  d0, d0, d2
    b       adjust_color_brightness.gray
adjust_color_brightness.chroma:
    fsub    d6, d5, d1                  // diff
    fmov    d8, #2.0
    fsub    d8, d8, d5
    fsub    d8, d8, d1                  // 2 - max - min
    fmov    d11, #0.5
    fcmp    d7, d11
    fcsel   d8, d8, d4, gt              // lightness <= 0.5: max + min
    fdiv    d4, d6, d8                  // saturation
    fcmp    d5, d10
    b.ne    adjust_color_brightness.not_red
    // (green - blue) / diff + (green < blue ? 6 : 0)
    fsub    d9, d2, d3
    fdiv    d9, d9, d6
    movi    d11, #0
    fcmp    d2, d3
    b.pl    adjust_color_brightness.wrap
    fmov    d11, #6.0
adjust_color_brightness.wrap:
    fadd    d9, d9, d11
    b       adjust_color_brightness.hue
adjust_color_brightness.not_red:
    fcmp    d5, d2
    b.ne    adjust_color_brightness.blue_max
    fsub    d9, d3, d10
    fdiv    d9, d9, d6
    fmov    d11, #2.0
    fadd    d9, d9, d11
    b       adjust_color_brightness.hue
adjust_color_brightness.blue_max:
    fsub    d9, d10, d2
    fdiv    d9, d9, d6
    fmov    d11, #4.0
    fadd    d9, d9, d11
adjust_color_brightness.hue:
    fmov    d11, #6.0
    fdiv    d9, d9, d11                 // hue_value
    fmov    d2, #1.0
    fminnm  d0, d0, d2
    movi    d2, #0
    fmaxnm  d0, d0, d2                  // lightness, clamped
    fcmp    d4, #0.0
    b.ne    adjust_color_brightness.colored
adjust_color_brightness.gray:
    fmov    d12, d0
    fmov    d13, d0
    fmov    d14, d0
    b       adjust_color_brightness.channels
adjust_color_brightness.colored:
    fmov    d1, #0.5
    fcmp    d0, d1
    b.mi    adjust_color_brightness.dark
    fadd    d8, d0, d4
    fmul    d1, d4, d0
    fsub    d8, d8, d1                  // lightness + saturation - lightness * saturation
    b       adjust_color_brightness.intensity
adjust_color_brightness.dark:
    fmov    d8, #1.0
    fadd    d8, d4, d8
    fmul    d8, d8, d0                  // lightness * (1 + saturation)
adjust_color_brightness.intensity:
    fadd    d2, d0, d0
    fsub    d2, d2, d8                  // lightness_scaled = 2 * lightness - intensity
    LDD     d1, col_third
    fadd    d0, d9, d1
    bl      hue_to_rgb
    fmov    d12, d0                     // red
    fmov    d0, d9
    bl      hue_to_rgb
    fmov    d13, d0                     // green
    LDD     d1, col_neg_third
    fadd    d0, d9, d1
    bl      hue_to_rgb
    fmov    d14, d0                     // blue
adjust_color_brightness.channels:
    // round_half_even keeps every vector register (it clobbers x16, x17)
    fmul    d0, d12, d15
    bl      round_half_even
    ubfiz   w19, w9, #16, #8
    fmul    d0, d13, d15
    bl      round_half_even
    ubfiz   w9, w9, #8, #8
    orr     w19, w19, w9
    fmul    d0, d14, d15
    bl      round_half_even
    and     w9, w9, #0xff
    orr     w9, w9, w19
    POP2    x19, x30
    ret

// hue_to_rgb(d2=lightness_scaled, d8=color_intensity, d0=hue_value)
// -> d0. Clobbers d1, d3, x16.
hue_to_rgb:
    fcmp    d0, #0.0
    b.pl    hue_to_rgb.positive         // hue < 0 (ordered) adds 1
    fmov    d1, #1.0
    fadd    d0, d0, d1
hue_to_rgb.positive:
    fmov    d1, #1.0
    fcmp    d0, d1
    b.le    hue_to_rgb.unit             // hue > 1 (ordered) subtracts 1
    fmov    d1, #-1.0
    fadd    d0, d0, d1
hue_to_rgb.unit:
    LDD     d1, col_sixth
    fcmp    d0, d1
    b.pl    hue_to_rgb.second
    fsub    d1, d8, d2
    fmov    d3, #6.0
    fmul    d1, d1, d3
    fmul    d1, d1, d0
    fadd    d1, d1, d2                  // scaled + (intensity - scaled) * 6 * hue
    fmov    d0, d1
    ret
hue_to_rgb.second:
    fmov    d1, #0.5
    fcmp    d0, d1
    b.pl    hue_to_rgb.third
    fmov    d0, d8
    ret
hue_to_rgb.third:
    LDD     d1, col_two_thirds
    fcmp    d0, d1
    b.pl    hue_to_rgb.last
    fsub    d3, d1, d0
    fsub    d1, d8, d2
    fmul    d3, d3, d1
    fmov    d1, #6.0
    fmul    d3, d3, d1
    fadd    d3, d3, d2                  // scaled + (intensity - scaled) * (2/3 - hue) * 6
    fmov    d0, d3
    ret
hue_to_rgb.last:
    fmov    d0, d2
    ret

// shift_color_towards(x0=color, x1=target_color, d0=factor)
// -> x9 = color, w2 = 1. Each channel is start + (end - start) * factor
// on the [0, 1] scale, times 255, truncated. When a channel leaves 0..=255
// the result is w2 = 0 (x9 = 0): Rust then formats the channels as hex,
// where a negative channel makes Color::from_hex panic on the '-' and a
// wide channel gives either a 7-digit pseudo-color or the Err carried by
// msg_invalid_color_value. No effect calls this function, and with a
// factor in [0, 1] the channels always stay in range, so callers can treat
// w2 = 0 as unreachable.
// Clobbers x12, x13, x16, d0-d4.
shift_color_towards:
    fmov    d4, d0
    LDD     d3, col_255
    mov     w12, #0
    mov     w13, #16                    // channel shift: red, green, blue
shift_color_towards.channel:
    lsr     w9, w0, w13
    and     w9, w9, #0xff
    scvtf   d0, w9
    fdiv    d0, d0, d3                  // start
    lsr     w9, w1, w13
    and     w9, w9, #0xff
    scvtf   d1, w9
    fdiv    d1, d1, d3                  // end
    fsub    d1, d1, d0
    fmul    d1, d1, d4
    fadd    d0, d0, d1                  // start + (end - start) * factor
    fmul    d0, d0, d3
    F64_TO_I64
    cmp     x9, #255
    b.hi    shift_color_towards.outside
    lsl     w9, w9, w13
    orr     w12, w12, w9
    subs    w13, w13, #8
    b.pl    shift_color_towards.channel
    mov     w9, w12
    mov     w2, #1
    ret
shift_color_towards.outside:
    mov     x9, #0
    mov     x2, #0
    ret

// random_color -> x9 = a color from randint(0, 0xFFFFFF): one RNG draw
// (plus rejections), whose value is the RGB word itself.
random_color:
    mov     x0, #0
    mov     x1, #0xFFFFFF
    b       rng_randint

    .section .rodata
    .balign 8
col_255:        .double 255.0
col_half:       .double 0.5
col_one:        .double 1.0
col_neg_one:    .double -1.0
col_two:        .double 2.0
col_four:       .double 4.0
col_six:        .double 6.0
col_third:      .8byte 0x3fd5555555555555   // 1.0 / 3.0
col_neg_third:  .8byte 0xbfd5555555555555   // -(1.0 / 3.0), the oracle's hue - 1/3
col_sixth:      .8byte 0x3fc5555555555555   // 1.0 / 6.0
col_two_thirds: .8byte 0x3fe5555555555555   // 2.0 / 3.0
STRING msg_invalid_color_value, "Invalid color value. Color must be an XTerm-256 color code or an RGB hex color string. Example: 255 or 'ffffff' or '#ffffff'"
