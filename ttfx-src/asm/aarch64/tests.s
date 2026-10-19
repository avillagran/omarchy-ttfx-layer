// tests.s - C entry points for tests/asm_diff.rs, which compares asm
// functions against their Rust originals on large input sets. Every thunk
// is a CENTRY/CEXIT frame: the engine's internal convention clobbers
// d8-d15 (and the callee may use x19-x28 as it likes only if it saves
// them), which AAPCS64 callers expect preserved. Integer results move from
// x9 to x0 (a two-word result, x9/x2, to x0/x1); float results stay in d0.
// u32 arguments are zero-extended first: AAPCS64 leaves their upper halves
// unspecified.
//
// Name them ttfx_test_<module>_<function> and declare them with EXPORT on
// the line before the label; aarch64 has one tier, so these are the names
// the tests link against. Keep this file test-only glue: no logic beyond
// marshalling arguments.

    .text

// ttfx_test_rng_randint(x0=state[4] in/out, x1=a, x2=b) -> x0
EXPORT ttfx_test_rng_randint
ttfx_test_rng_randint:
    CENTRY
    mov     x19, x0
    mov     x21, x1
    mov     x22, x2
    bl      rng_load
    mov     x0, x21
    mov     x1, x22
    bl      rng_randint
    mov     x23, x9
    mov     x0, x19
    bl      rng_store
    mov     x0, x23
    CEXIT

// ttfx_test_rng_uniform(x0=state[4] in/out, d0=a, d1=b) -> d0
EXPORT ttfx_test_rng_uniform
ttfx_test_rng_uniform:
    CENTRY
    sub     sp, sp, #16
    mov     x19, x0
    stp     d0, d1, [sp]
    bl      rng_load
    ldp     d0, d1, [sp]
    bl      rng_uniform
    str     d0, [sp]
    mov     x0, x19
    bl      rng_store
    ldr     d0, [sp]
    add     sp, sp, #16
    CEXIT

// ttfx_test_rng_shuffle64(x0=state[4] in/out, x1=array, x2=length)
EXPORT ttfx_test_rng_shuffle64
ttfx_test_rng_shuffle64:
    CENTRY
    mov     x19, x0
    mov     x21, x1
    mov     x22, x2
    bl      rng_load
    mov     x0, x21
    mov     x1, x22
    bl      rng_shuffle64
    mov     x0, x19
    bl      rng_store
    CEXIT

// Easing utilities. Storage layouts are in easing.s.
// ttfx_test_ease(w0=id, d0=t) -> d0
EXPORT ttfx_test_ease
ttfx_test_ease:
    CENTRY
    mov     w0, w0
    bl      ease
    CEXIT

// ttfx_test_bezier_easing(x0=&[x1, y1, x2, y2], d0=t) -> d0
EXPORT ttfx_test_bezier_easing
ttfx_test_bezier_easing:
    CENTRY
    bl      bezier_easing
    CEXIT

// ttfx_test_easing_tracker_new(x0=out, w1=id, x2=total_steps, w3=clamp)
EXPORT ttfx_test_easing_tracker_new
ttfx_test_easing_tracker_new:
    CENTRY
    mov     w1, w1
    mov     w3, w3
    bl      easing_tracker_new
    CEXIT

// ttfx_test_easing_tracker_step(x0=tracker) -> d0
EXPORT ttfx_test_easing_tracker_step
ttfx_test_easing_tracker_step:
    CENTRY
    bl      easing_tracker_step
    CEXIT

// ttfx_test_easing_tracker_reset(x0=tracker)
EXPORT ttfx_test_easing_tracker_reset
ttfx_test_easing_tracker_reset:
    CENTRY
    bl      easing_tracker_reset
    CEXIT

// ttfx_test_easing_tracker_is_complete(x0=tracker) -> w0
EXPORT ttfx_test_easing_tracker_is_complete
ttfx_test_easing_tracker_is_complete:
    CENTRY
    bl      easing_tracker_is_complete
    mov     w0, w9
    CEXIT

// ttfx_test_sequence_easer_new(x0=out, x1=sequence, x2=len, w3=id,
// x4=steps)
EXPORT ttfx_test_sequence_easer_new
ttfx_test_sequence_easer_new:
    CENTRY
    mov     w3, w3
    bl      sequence_easer_new
    CEXIT

// ttfx_test_sequence_easer_step(x0=easer) -> x0 = step record
EXPORT ttfx_test_sequence_easer_step
ttfx_test_sequence_easer_step:
    CENTRY
    bl      sequence_easer_step
    mov     x0, x9
    CEXIT

// ttfx_test_sequence_easer_reset(x0=easer)
EXPORT ttfx_test_sequence_easer_reset
ttfx_test_sequence_easer_reset:
    CENTRY
    bl      sequence_easer_reset
    CEXIT

// ttfx_test_sequence_easer_is_complete(x0=easer) -> w0
EXPORT ttfx_test_sequence_easer_is_complete
ttfx_test_sequence_easer_is_complete:
    CENTRY
    bl      sequence_easer_is_complete
    mov     w0, w9
    CEXIT

// ttfx_test_arena_reset(): a fresh arena for the list-returning functions,
// releasing the previous one (tests never run an effect, which would do
// this itself).
EXPORT ttfx_test_arena_reset
ttfx_test_arena_reset:
    CENTRY
    bl      release_regions
    mov     x0, #(1 << 38)
    bl      reserve
    STX     x9, arena_ptr
    CEXIT

// pycompat, under the register map already.
// ttfx_test_pycompat_round_half_even(d0) -> x0
EXPORT ttfx_test_pycompat_round_half_even
ttfx_test_pycompat_round_half_even:
    CENTRY
    bl      round_half_even
    mov     x0, x9
    CEXIT

// ttfx_test_pycompat_f64_to_i64(d0) -> x0
EXPORT ttfx_test_pycompat_f64_to_i64
ttfx_test_pycompat_f64_to_i64:
    CENTRY
    bl      f64_to_i64
    mov     x0, x9
    CEXIT

// ttfx_test_pycompat_floor_div(x0, x1) -> x0
EXPORT ttfx_test_pycompat_floor_div
ttfx_test_pycompat_floor_div:
    CENTRY
    bl      floor_div
    mov     x0, x9
    CEXIT

// ttfx_test_pycompat_py_mod(x0, x1) -> x0
EXPORT ttfx_test_pycompat_py_mod
ttfx_test_pycompat_py_mod:
    CENTRY
    bl      py_mod
    mov     x0, x9
    CEXIT

// Lists come back as x9 = pointer, x2 = count: a two-word C struct,
// returned in x0/x1.
// ttfx_test_geometry_coords_on_circle(x0=origin, x1=radius, x2=limit,
// w3=unique)
EXPORT ttfx_test_geometry_coords_on_circle
ttfx_test_geometry_coords_on_circle:
    CENTRY
    mov     w3, w3
    bl      find_coords_on_circle
    mov     x0, x9
    mov     x1, x2
    CEXIT

// ttfx_test_geometry_coords_in_circle(x0=center, x1=diameter)
EXPORT ttfx_test_geometry_coords_in_circle
ttfx_test_geometry_coords_in_circle:
    CENTRY
    bl      find_coords_in_circle
    mov     x0, x9
    mov     x1, x2
    CEXIT

// ttfx_test_geometry_coords_in_rect(x0=origin, x1=distance)
EXPORT ttfx_test_geometry_coords_in_rect
ttfx_test_geometry_coords_in_rect:
    CENTRY
    bl      find_coords_in_rect
    mov     x0, x9
    mov     x1, x2
    CEXIT

// ttfx_test_geometry_coords_on_rect(x0=origin, x1=half_width,
// x2=half_height)
EXPORT ttfx_test_geometry_coords_on_rect
ttfx_test_geometry_coords_on_rect:
    CENTRY
    bl      find_coords_on_rect
    mov     x0, x9
    mov     x1, x2
    CEXIT

// ttfx_test_geometry_extrapolate_along_ray(x0=origin, x1=target,
// d0=offset) -> x0
EXPORT ttfx_test_geometry_extrapolate_along_ray
ttfx_test_geometry_extrapolate_along_ray:
    CENTRY
    bl      extrapolate_along_ray
    mov     x0, x9
    CEXIT

// ttfx_test_geometry_coord_on_bezier_curve(x0=start, x1=control, x2=count,
// x3=end, d0=t) -> x0
EXPORT ttfx_test_geometry_coord_on_bezier_curve
ttfx_test_geometry_coord_on_bezier_curve:
    CENTRY
    bl      find_coord_on_bezier_curve
    mov     x0, x9
    CEXIT

// ttfx_test_geometry_coord_on_line(x0=start, x1=end, d0=t) -> x0
EXPORT ttfx_test_geometry_coord_on_line
ttfx_test_geometry_coord_on_line:
    CENTRY
    bl      find_coord_on_line
    mov     x0, x9
    CEXIT

// ttfx_test_geometry_length_of_bezier_curve(x0=start, x1=control,
// x2=count, x3=end) -> d0
EXPORT ttfx_test_geometry_length_of_bezier_curve
ttfx_test_geometry_length_of_bezier_curve:
    CENTRY
    bl      find_length_of_bezier_curve
    CEXIT

// ttfx_test_geometry_length_of_line(x0=a, x1=b, w2=double) -> d0
EXPORT ttfx_test_geometry_length_of_line
ttfx_test_geometry_length_of_line:
    CENTRY
    mov     w2, w2
    bl      find_length_of_line
    CEXIT

// ttfx_test_geometry_circle_iter_init(x0=state, x1=center, x2=diameter)
EXPORT ttfx_test_geometry_circle_iter_init
ttfx_test_geometry_circle_iter_init:
    CENTRY
    bl      coords_in_circle_init
    CEXIT

// ttfx_test_geometry_circle_iter_next(x0=state, x1=out coord) -> w0 = 1
// while coordinates remain.
EXPORT ttfx_test_geometry_circle_iter_next
ttfx_test_geometry_circle_iter_next:
    CENTRY
    mov     x19, x1
    bl      coords_in_circle_next
    str     x9, [x19]
    mov     w0, w2
    CEXIT

// ttfx_test_geometry_normalized_distance(x0=bottom, x1=top, x2=left,
// x3=right, x4=coord, x5=out f64) -> w0 = 1 when inside.
EXPORT ttfx_test_geometry_normalized_distance
ttfx_test_geometry_normalized_distance:
    CENTRY
    mov     x19, x5
    bl      find_normalized_distance_from_center
    str     d0, [x19]
    mov     w0, w9
    CEXIT

// ttfx_test_color_adjust_color_brightness(x0=color, d0=brightness) -> x0
EXPORT ttfx_test_color_adjust_color_brightness
ttfx_test_color_adjust_color_brightness:
    CENTRY
    bl      adjust_color_brightness
    mov     x0, x9
    CEXIT

// ttfx_test_color_shift_color_towards(x0=color, x1=target, d0=factor)
// -> x0 = color, x1 = ok
EXPORT ttfx_test_color_shift_color_towards
ttfx_test_color_shift_color_towards:
    CENTRY
    bl      shift_color_towards
    mov     x0, x9
    mov     w1, w2
    CEXIT

// ttfx_test_color_random_color(x0=state[4] in/out) -> x0
EXPORT ttfx_test_color_random_color
ttfx_test_color_random_color:
    CENTRY
    mov     x19, x0
    bl      rng_load
    bl      random_color
    mov     x21, x9
    mov     x0, x19
    bl      rng_store
    mov     x0, x21
    CEXIT

// ttfx_test_rng_chance(x0=state[4] in/out, d0=c) -> x0 = 1 when
// random() < c, decided as RNG_BITS53 < rng_threshold(c).
EXPORT ttfx_test_rng_chance
ttfx_test_rng_chance:
    CENTRY
    sub     sp, sp, #16
    mov     x19, x0
    str     d0, [sp]
    bl      rng_load
    ldr     d0, [sp]
    bl      rng_threshold
    mov     x21, x9
    RNG_BITS53
    cmp     x9, x21
    cset    x22, lo
    mov     x0, x19
    bl      rng_store
    mov     x0, x22
    add     sp, sp, #16
    CEXIT

// ttfx_test_audio_tint(w0=color, x1=params[10] f32: brightness, then the
// row-major 3x3 matrix) -> w0 = retinted color: audio_tint with those
// parameters (audio_frame_poll's audio_params slot is loaded directly).
EXPORT ttfx_test_audio_tint
ttfx_test_audio_tint:
    CENTRY
    mov     w19, w0
    ADRG    x0, audio_params
    ldp     q0, q1, [x1]
    stp     q0, q1, [x0]
    ldr     d2, [x1, #32]
    str     d2, [x0, #32]
    mov     w0, w19
    bl      audio_tint
    CEXIT
