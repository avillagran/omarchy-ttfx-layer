// effects/rings.s - "Characters are dispersed and form into spinning rings"
// (src/effects/rings.rs).
//
// Config (src/asm/effects.rs, the Rings arm).
//
// Rust gives every ring character one single-waypoint path per ring
// coordinate ("0", "1", ...), chained in a loop. Here one engine path (RING)
// stands in for all of them: activating ring path k points its waypoint at
// the k-th rotated coordinate and swaps in path k's own total and origin
// distance (a path's total drifts by rounding across activations, so each
// keeps its history), then stores them back. The chain event becomes a
// callback that activates k + 1. Condense paths are ordinary engine paths.
//
// The "disperse" path is removed and recreated each cycle upstream; here it
// is emptied in place (path_reset), which is the same path afterwards.

.equ HAVE_rings, 1

.equ RINGS.ring_colors,         0       // *const u64
.equ RINGS.ring_color_count,    8
.equ RINGS.ring_gap,            16      // f64
.equ RINGS.spin_duration,       24
.equ RINGS.spin_speed_lo,       32      // f64
.equ RINGS.spin_speed_hi,       40      // f64
.equ RINGS.disperse_duration,   48
.equ RINGS.cycles,              56
.equ RINGS.final_stops,         64      // *const u64
.equ RINGS.final_stop_count,    72
.equ RINGS.final_steps,         80      // *const i64
.equ RINGS.final_step_count,    88
.equ RINGS.final_direction,     96
.equ RINGS_size,                104

// RingsIterator.Ring
.equ RINGS_RING.ccw,            0       // counter-clockwise coordinates
.equ RINGS_RING.cw,             8       // the same, reversed
.equ RINGS_RING.n,              16
.equ RINGS_RING.color,          24
.equ RINGS_RING.speed,          32      // f64 rotation_speed
.equ RINGS_RING_size,           40

// per ring character (ch_user0 points here)
.equ RINGCH.coords,             0       // the ring's coordinates in its direction
.equ RINGCH.n,                  8
.equ RINGCH.start,              16      // character_starting_index
.equ RINGCH.cur,                24      // the ring path RING stands for now
.equ RINGCH.last,               32      // character_last_ring_path: >= 0 a ring
                                        // path, < 0 the engine path ~last
.equ RINGCH.dist,               40      // n x (total_distance, origin distance)
.equ RINGCH.rpath,              48      // RING (u32)
.equ RINGCH.dpath,              52      // DISPERSE (u32)
.equ RINGCH.gscene,             56      // u32
.equ RINGCH.dscene,             60      // u32
.equ RINGCH.base_speed,         64      // f64, authored rotation speed: the
                                        // audio factor never multiplies itself
.equ RINGCH_size,               72

// ch_user1: the home path (low half); RINGS_EXTERNAL_BIT marks non-ring
// characters
.equ RINGS_EXTERNAL_BIT,        32

// scene names
.equ RINGS_SCN_GRADIENT,        NAME_LITERAL + 0
.equ RINGS_SCN_DISPERSE,        NAME_LITERAL + 1
// path names
.equ RINGS_PATH_HOME,           NAME_LITERAL + 0
.equ RINGS_PATH_EXTERNAL,       NAME_LITERAL + 1
.equ RINGS_PATH_RING,           NAME_LITERAL + 2
.equ RINGS_PATH_DISPERSE,       NAME_LITERAL + 3

.equ RINGS_EASE_OUT_SINE,       2
.equ RINGS_EASE_OUT_QUAD,       5
.equ RINGS_EASE_OUT_CUBIC,      8

.equ RINGS_PH_START,            0
.equ RINGS_PH_DISPERSE,         1
.equ RINGS_PH_SPIN,             2
.equ RINGS_PH_FINAL,            3
.equ RINGS_PH_COMPLETE,         4

    .text

// rings_build: Rings::build.
rings_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56]: the home path
    LDX     x19, effect_config
    LDD     d0, rings_audio_one
    STD     d0, rings_audio_speed
    // ring_gap = max(round(min(top, right) * ring_gap), 1)
    LDX     x9, canvas_top
    LDX     x10, canvas_right
    cmp     x9, x10
    csel    x9, x10, x9, gt
    scvtf   d0, x9
    ldr     d1, [x19, #RINGS.ring_gap]
    fmul    d0, d0, d1
    bl      round_half_even
    mov     x3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    STX     x9, ring_gap
    bl      rings_final_map
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    cset    w9, eq
    STB     w9, rings_dynamic
    // every input character: start scene, home path, visible
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9                     // becomes pending_chars
    mov     x22, x2
    mov     x23, #0
rings_build.start_char:
    cmp     x23, x22
    b.hs    rings_build.shuffle
    ldr     w24, [x21, x23, lsl #2]
    mov     w0, w24
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    mov     w0, w24
    bl      rings_final_colors          // x9 = fg, x2 = bg
    mov     x3, x9
    mov     x4, x2
    LDX     x1, ch_sym
    ldr     x1, [x1, x24, lsl #3]
    mov     w0, w20
    mov     w2, #1
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w24
    LDD     d0, rings_speed_home
    mov     w1, #RINGS_EASE_OUT_QUAD
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, RINGS_PATH_HOME
    bl      path_new
    LDX     x3, ch_user1
    mov     w9, w9
    str     x9, [x3, x24, lsl #3]       // home path, not external
    str     w9, [sp, #56]
    mov     w0, w24
    bl      char_input_coord
    mov     x1, x9
    ldr     w0, [sp, #56]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w24
    mov     w1, w20
    bl      scene_activate
    mov     w0, w24
    bl      set_visible
    add     x23, x23, #1
    b       rings_build.start_char
rings_build.shuffle:
    mov     x0, x21
    mov     x1, x22
    bl      rng_shuffle32
    bl      rings_make
    // assign characters to rings, alternating directions
    lsl     x0, x22, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, ring_list
    mov     x23, #0                     // pending index
    mov     x24, #0                     // ring index
rings_build.ring:
    LDX     x9, ring_count
    cmp     x24, x9
    b.hs    rings_build.external
    mov     x20, #0                     // position on the ring
rings_build.ring_slot:
    mov     x9, #RINGS_RING_size
    mul     x9, x24, x9
    LDX     x10, rings_array
    add     x9, x9, x10
    ldr     x10, [x9, #RINGS_RING.n]
    cmp     x20, x10
    b.hs    rings_build.next_ring
    cmp     x23, x22
    b.hs    rings_build.next_ring       // (the remaining pops find nothing)
    ldr     w0, [x21, x23, lsl #2]
    add     x23, x23, #1
    mov     x1, x9
    and     w2, w24, #1                 // clockwise on odd rings
    mov     x3, x20
    bl      ring_add_character
    add     x20, x20, #1
    b       rings_build.ring_slot
rings_build.next_ring:
    add     x24, x24, #1
    b       rings_build.ring
rings_build.external:
    // characters not in rings leave the canvas
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    lsl     x0, x22, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, rings_ext_list
    mov     x23, #0
rings_build.ext_char:
    cmp     x23, x22
    b.hs    rings_build.state
    ldr     w24, [x21, x23, lsl #2]
    add     x23, x23, #1
    LDX     x9, ch_user0
    ldr     x9, [x9, x24, lsl #3]
    cbnz    x9, rings_build.ext_char
    mov     w0, #1
    mov     w1, #0
    bl      canvas_random_coord
    mov     x20, x9
    mov     w0, w24
    LDD     d0, rings_speed_home
    mov     w1, #RINGS_EASE_OUT_SINE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, RINGS_PATH_EXTERNAL
    bl      path_new
    mov     w0, w9
    mov     x1, x20
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    LDX     x9, ch_user1
    ldr     x10, [x9, x24, lsl #3]
    orr     x10, x10, #(1 << RINGS_EXTERNAL_BIT)
    str     x10, [x9, x24, lsl #3]
    LDX     x9, rings_ext_list
    LDX     x3, rings_ext_count
    str     w24, [x9, x3, lsl #2]
    add     x3, x3, #1
    STX     x3, rings_ext_count
    mov     w0, w24
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    MOV64   w3, RINGS_PATH_EXTERNAL
    mov     w4, #ACT_CALLBACK
    ADRG    x5, rings_set_invisible
    mov     x6, #0
    bl      event_register
    b       rings_build.ext_char
rings_build.state:
    mov     w9, #RINGS_PH_START
    STB     w9, rings_phase
    STB     wzr, rings_initial_done
    ldr     x9, [x19, #RINGS.spin_duration]
    STX     x9, rings_spin_left
    ldr     x9, [x19, #RINGS.disperse_duration]
    STX     x9, rings_disperse_left
    ldr     x9, [x19, #RINGS.cycles]
    STX     x9, rings_cycles_left
    mov     x9, #100
    STX     x9, rings_initial_left
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// rings_make: rings from radius 1 outward by ring_gap until less than a
// quarter of a ring's coordinates are on the canvas. Ring.__init__ draws
// the rotation speed.
rings_make:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x23, canvas_right
    LDX     x9, canvas_top
    cmp     x23, x9
    csel    x23, x9, x23, lt            // radius limit
    // at most one ring per radius below the limit
    add     x0, x23, #1
    mov     x9, #RINGS_RING_size
    mul     x0, x0, x9
    bl      alloc
    STX     x9, rings_array
    mov     x24, #1                     // radius
rings_make.radius:
    cmp     x24, x23
    b.ge    rings_make.done
    LDX     x0, center_row
    lsl     x0, x0, #32
    LDW     w9, center_col
    orr     x0, x0, x9
    mov     x1, x24
    mov     x9, #7
    mul     x2, x24, x9
    mov     w3, #1
    bl      find_coords_on_circle
    mov     x21, x9
    mov     x22, x2
    mov     w19, #0                     // coordinates in the canvas
    mov     x20, #0
rings_make.count:
    cmp     x20, x22
    b.hs    rings_make.counted
    ldr     x1, [x21, x20, lsl #3]
    bl      coord_in_canvas
    add     w19, w19, w9
    add     x20, x20, #1
    b       rings_make.count
rings_make.counted:
    scvtf   d0, x19
    scvtf   d1, x22
    fdiv    d0, d0, d1
    LDD     d1, rings_quarter
    fcmp    d1, d0
    b.gt    rings_make.done             // ratio < 0.25 (NaN is not)
    LDX     x19, ring_count
    mov     x9, #RINGS_RING_size
    mul     x20, x19, x9
    LDX     x9, rings_array
    add     x20, x20, x9
    str     x21, [x20, #RINGS_RING.ccw]
    str     x22, [x20, #RINGS_RING.n]
    LDX     x3, effect_config
    ldr     x10, [x3, #RINGS.ring_color_count]
    udiv    x9, x19, x10
    msub    x2, x9, x10, x19
    ldr     x9, [x3, #RINGS.ring_colors]
    ldr     x9, [x9, x2, lsl #3]
    str     x9, [x20, #RINGS_RING.color]
    lsl     x0, x22, #3
    bl      alloc
    str     x9, [x20, #RINGS_RING.cw]
    add     x3, x21, x22, lsl #3
    sub     x3, x3, #8
    mov     x2, #0
rings_make.reverse:
    cmp     x2, x22
    b.hs    rings_make.speed
    ldr     x1, [x3]
    str     x1, [x9, x2, lsl #3]
    sub     x3, x3, #8
    add     x2, x2, #1
    b       rings_make.reverse
rings_make.speed:
    LDX     x3, effect_config
    ldr     d0, [x3, #RINGS.spin_speed_lo]
    ldr     d1, [x3, #RINGS.spin_speed_hi]
    bl      rng_uniform
    str     d0, [x20, #RINGS_RING.speed]
    LDX     x9, ring_count
    add     x9, x9, #1
    STX     x9, ring_count
    LDX     x9, ring_gap
    add     x24, x24, x9
    b       rings_make.radius
rings_make.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// ring_add_character(w0=slot, x1=ring, w2=clockwise, x3=starting index):
// Ring.add_character - gradient scene, the ring paths (RING) and their
// chain, disperse scene.
ring_add_character:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w19, w0
    mov     x21, x1
    mov     w22, w2
    mov     x23, x3
    mov     x0, #RINGCH_size
    bl      alloc
    mov     x20, x9
    LDX     x3, ch_user0
    str     x20, [x3, x19, lsl #3]
    ldr     x9, [x21, #RINGS_RING.ccw]
    ldr     x10, [x21, #RINGS_RING.cw]
    cmp     w22, #0
    csel    x9, x10, x9, ne
    str     x9, [x20, #RINGCH.coords]
    ldr     x9, [x21, #RINGS_RING.n]
    str     x9, [x20, #RINGCH.n]
    str     x23, [x20, #RINGCH.start]
    lsl     x0, x9, #4
    bl      alloc
    str     x9, [x20, #RINGCH.dist]
    // gradient: final color -> ring color, 3 ticks per color
    mov     w0, w19
    MOV64   w1, RINGS_SCN_GRADIENT
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     w9, [x20, #RINGCH.gscene]
    mov     w0, w9
    mov     w1, w19
    mov     x2, x21
    mov     w3, #0                      // final color first
    mov     w4, #3
    bl      ring_scene_frames
    // the ring paths: RING, first at rotated[0]
    mov     w0, w19
    ldr     d0, [x21, #RINGS_RING.speed]
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, RINGS_PATH_RING
    bl      path_new
    str     w9, [x20, #RINGCH.rpath]
    ldr     d0, [x21, #RINGS_RING.speed]
    str     d0, [x20, #RINGCH.base_speed]
    mov     w0, w9
    ldr     x1, [x20, #RINGCH.coords]
    ldr     x1, [x1, x23, lsl #3]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // disperse: ring color -> final color, 10 ticks per color
    mov     w0, w19
    MOV64   w1, RINGS_SCN_DISPERSE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     w9, [x20, #RINGCH.dscene]
    mov     w0, w9
    mov     w1, w19
    mov     x2, x21
    mov     w3, #1                      // ring color first
    mov     w4, #10
    bl      ring_scene_frames
    // the disperse path, filled by make_disperse_waypoints
    mov     w0, w19
    LDD     d0, rings_speed_disperse
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #1
    MOV64   w5, RINGS_PATH_DISPERSE
    bl      path_new
    str     w9, [x20, #RINGCH.dpath]
    // chain_paths(ring_paths, loop): each completion activates the next
    ldr     x9, [x20, #RINGCH.n]
    cmp     x9, #2
    b.lo    ring_add_character.chained
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    MOV64   w3, RINGS_PATH_RING
    mov     w4, #ACT_CALLBACK
    ADRG    x5, ring_advance
    mov     x6, #0
    bl      event_register
ring_add_character.chained:
    LDX     x9, ring_list
    LDX     x3, ring_list_count
    str     w19, [x9, x3, lsl #2]
    add     x3, x3, #1
    STX     x3, ring_list_count
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// ring_scene_frames(w0=scene, w1=slot, x2=ring, w3=ring color first,
// w4=duration): the input colors for one tick when dynamic, otherwise the
// 8-step gradient between the final color and the ring color.
ring_scene_frames:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    mov     w19, w0
    mov     w21, w1
    mov     x22, x2
    mov     w23, w3
    mov     w24, w4
    mov     w0, w1
    bl      rings_final_colors
    LDX     x1, ch_sym
    ldr     x1, [x1, x21, lsl #3]
    LDB     w10, rings_dynamic
    cbz     w10, ring_scene_frames.gradient
    mov     x3, x9
    mov     x4, x2
    mov     w0, w19
    mov     w2, #1
    mov     w5, #0
    bl      scene_add_frame
    b       ring_scene_frames.done
ring_scene_frames.gradient:
    STX     x1, rings_sym
    ldr     x3, [x22, #RINGS_RING.color]
    cbz     w23, ring_scene_frames.order
    mov     x10, x9
    mov     x9, x3
    mov     x3, x10
ring_scene_frames.order:
    ADRG    x0, rings_pair_stops
    stp     x9, x3, [x0]
    mov     w1, #2
    ADRG    x2, rings_eight_steps
    mov     w3, #1
    ADRG    x4, rings_pair_spectrum
    bl      gradient_new
    mov     w0, w19
    ADRG    x1, rings_sym
    mov     x2, #1
    mov     w3, w24
    ADRG    x4, rings_pair_spectrum
    mov     w5, w9
    mov     x6, #0
    mov     x7, #0
    bl      scene_apply_gradient
ring_scene_frames.done:
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// final colors(w0=slot) -> x9 = fg, x2 = bg: character_final_color_map -
// the input colors when dynamic, else the final gradient at the input
// coordinate over no background. Clobbers x3, x10, x16.
rings_final_colors:
    LDB     w9, rings_dynamic
    cbz     w9, rings_final_colors.mapped
    LDX     x9, ch_fg
    ldr     x9, [x9, w0, uxtw #3]
    LDX     x2, ch_bg
    ldr     x2, [x2, w0, uxtw #3]
    ret
rings_final_colors.mapped:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, w0, uxtw #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, rings_final_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, w0, uxtw #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, rings_final_map_ptr
    ldr     x9, [x3, x9, lsl #3]
    mov     x2, #NONE
    ret

// rings_final_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
rings_final_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #RINGS.final_steps]
    ldr     x3, [x19, #RINGS.final_step_count]
    ldr     x1, [x19, #RINGS.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, rings_final_spectrum
    ldr     x0, [x19, #RINGS.final_stops]
    ldr     x1, [x19, #RINGS.final_stop_count]
    ldr     x2, [x19, #RINGS.final_steps]
    ldr     x3, [x19, #RINGS.final_step_count]
    LDX     x4, rings_final_spectrum
    bl      gradient_new
    LDX     x0, rings_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, rings_final_map_width
    ldr     x6, [x19, #RINGS.final_direction]
    bl      gradient_map
    STX     x9, rings_final_map_ptr
    POP2    x19, x30
    ret

// ------------------------------------------------------------ ring paths

// ring_activate(w0=slot, x1=k): activate_path(ring path "k") through RING:
// its waypoint becomes rotated[k] and it carries path k's distances.
ring_activate:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     w19, w0
    LDX     x21, ch_user0
    ldr     x21, [x21, x19, lsl #3]
    str     x1, [x21, #RINGCH.cur]
    lsl     x22, x1, #4
    ldr     x9, [x21, #RINGCH.dist]
    add     x22, x22, x9                // (total, origin distance) of path k
    ldr     x2, [x21, #RINGCH.start]
    add     x2, x2, x1                  // start, k < n: (start + k) % n
    ldr     x10, [x21, #RINGCH.n]
    subs    x9, x2, x10
    csel    x2, x9, x2, hs
    ldr     x9, [x21, #RINGCH.coords]
    ldr     x3, [x9, x2, lsl #3]
    ldr     w1, [x21, #RINGCH.rpath]
    PATH_PTR x9, x1
    ldr     x2, [x9, #PA_WPS]
    str     x3, [x2, #WP_COORD]
    ldr     d0, [x22]
    str     d0, [x9, #PA_TOTAL]
    ldr     d0, [x22, #8]
    str     d0, [x9, #PA_ORIGIN_DIST]
    // Each rotation segment samples the current audio factor on activation.
    // path_activate derives PA_MAX from this speed, so there is no mid-path
    // jump, stale easing table, or alteration of the distance history.
    ldr     d0, [x21, #RINGCH.base_speed]
    LDD     d1, rings_audio_speed
    fmul    d0, d0, d1
    str     d0, [x9, #PA_SPEED]
    mov     w0, w19
    bl      path_activate
    ldr     w1, [x21, #RINGCH.rpath]
    PATH_PTR x9, x1
    ldr     d0, [x9, #PA_TOTAL]
    str     d0, [x22]
    ldr     d0, [x9, #PA_ORIGIN_DIST]
    str     d0, [x22, #8]
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// ring_advance(w0=slot): the chain event - ring path k completed, k + 1
// (wrapping) activates.
ring_advance:
    LDX     x9, ch_user0
    ldr     x9, [x9, w0, uxtw #3]
    ldr     x1, [x9, #RINGCH.cur]
    add     x1, x1, #1
    ldr     x10, [x9, #RINGCH.n]
    cmp     x1, x10
    csel    x1, xzr, x1, hs
    b       ring_activate

// rings_set_invisible(w0=slot): CB_SET_INVISIBLE.
rings_set_invisible:
    mov     w1, #0
    b       set_visibility

// make_disperse_waypoints(w0=slot, x1=origin): five random coordinates of
// find_coords_in_rect(origin, ring_gap), then the disperse path is made
// afresh with them. Locals: [sp, #0..39] the five coordinates.
ring_make_disperse:
    sub     sp, sp, #112
    stp     x19, x20, [sp, #48]
    stp     x21, x22, [sp, #64]
    stp     x23, x24, [sp, #80]
    str     x30, [sp, #96]
    mov     w19, w0
    mov     x21, x1
    LDX     x22, ring_gap
    lsl     x23, x22, #1
    add     x23, x23, #1                // side
    mov     x24, #0
ring_make_disperse.draw:
    mov     x0, #0
    mul     x1, x23, x23
    bl      rng_randrange
    sdiv    x10, x9, x23
    msub    x2, x10, x23, x9            // row index
    mov     x9, x10                     // column index
    sxtw    x3, w21
    sub     x3, x3, x22
    add     x9, x9, x3                  // column
    asr     x3, x21, #32
    sub     x3, x3, x22
    add     x2, x2, x3                  // row
    lsl     x2, x2, #32
    mov     w9, w9
    orr     x9, x9, x2
    str     x9, [sp, x24, lsl #3]
    add     w24, w24, #1
    cmp     w24, #5
    b.lo    ring_make_disperse.draw
    LDX     x9, ch_user0
    ldr     x9, [x9, x19, lsl #3]
    ldr     w20, [x9, #RINGCH.dpath]
    mov     w0, w20
    bl      path_reset
    mov     x24, #0
ring_make_disperse.waypoint:
    mov     w0, w20
    ldr     x1, [sp, x24, lsl #3]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    add     w24, w24, #1
    cmp     w24, #5
    b.lo    ring_make_disperse.waypoint
    ldr     x30, [sp, #96]
    ldp     x23, x24, [sp, #80]
    ldp     x21, x22, [sp, #64]
    ldp     x19, x20, [sp, #48]
    add     sp, sp, #112
    ret

// ------------------------------------------------------------ phases

// initial disperse: every ring character heads (eased) to the first
// waypoint of a fresh disperse path around its ring start, which then loops;
// the other characters leave the canvas.
rings_initial_disperse:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    stp     x23, x30, [sp, #32]
    mov     x21, #0
rings_initial_disperse.ring_char:
    LDX     x9, ring_list_count
    cmp     x21, x9
    b.hs    rings_initial_disperse.external
    LDX     x9, ring_list
    ldr     w19, [x9, x21, lsl #2]
    add     x21, x21, #1
    LDX     x20, ch_user0
    ldr     x20, [x20, x19, lsl #3]
    ldr     x9, [x20, #RINGCH.coords]
    ldr     x3, [x20, #RINGCH.start]
    ldr     x1, [x9, x3, lsl #3]        // ring path "0"'s waypoint
    mov     w0, w19
    bl      ring_make_disperse
    ldr     w1, [x20, #RINGCH.dpath]
    PATH_PTR x9, x1
    ldr     x9, [x9, #PA_WPS]
    ldr     x22, [x9, #WP_COORD]
    mov     w0, w19
    LDD     d0, rings_speed_initial
    mov     w1, #RINGS_EASE_OUT_CUBIC
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w23, w9
    mov     w0, w9
    mov     x1, x22
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    PATH_PTR x9, x23
    ldr     w3, [x9, #PA_NAME]
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w4, #ACT_ACTIVATE_PATH
    MOV64   w5, RINGS_PATH_DISPERSE
    mov     x6, #0
    bl      event_register
    mov     w0, w19
    ldr     w1, [x20, #RINGCH.dscene]
    bl      scene_activate
    mov     w0, w19
    mov     w1, w23
    bl      path_activate
    mov     w0, w19
    bl      active_insert
    b       rings_initial_disperse.ring_char
rings_initial_disperse.external:
    mov     x21, #0
rings_initial_disperse.ext_char:
    LDX     x9, rings_ext_count
    cmp     x21, x9
    b.hs    rings_initial_disperse.done
    LDX     x9, rings_ext_list
    ldr     w19, [x9, x21, lsl #2]
    add     x21, x21, #1
    mov     w0, w19
    MOV64   w1, RINGS_PATH_EXTERNAL
    bl      path_activate_name
    mov     w0, w19
    bl      active_insert
    b       rings_initial_disperse.ext_char
rings_initial_disperse.done:
    ldp     x23, x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// rings_spin: Ring.spin for every ring - a condense path back to the first
// waypoint of the character's last ring path, which resumes on arrival.
rings_spin:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     x21, #0
rings_spin.char:
    LDX     x9, ring_list_count
    cmp     x21, x9
    b.hs    rings_spin.done
    LDX     x9, ring_list
    ldr     w19, [x9, x21, lsl #2]
    add     x21, x21, #1
    LDX     x20, ch_user0
    ldr     x20, [x20, x19, lsl #3]
    ldr     x24, [x20, #RINGCH.last]
    tbnz    x24, #63, rings_spin.engine_path
    ldr     x2, [x20, #RINGCH.start]
    add     x2, x2, x24                 // start, last < n: (start + last) % n
    ldr     x10, [x20, #RINGCH.n]
    subs    x9, x2, x10
    csel    x2, x9, x2, hs
    ldr     x9, [x20, #RINGCH.coords]
    ldr     x22, [x9, x2, lsl #3]
    b       rings_spin.condense
rings_spin.engine_path:
    mvn     x9, x24
    PATH_PTR x3, x9
    ldr     x9, [x3, #PA_WPS]
    ldr     x22, [x9, #WP_COORD]
rings_spin.condense:
    mov     w0, w19
    LDD     d0, rings_speed_condense
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w23, w9
    mov     w0, w9
    mov     x1, x22
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    PATH_PTR x9, x23
    ldr     w3, [x9, #PA_NAME]
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    tbnz    x24, #63, rings_spin.to_engine_path
    mov     x6, x24
    mov     w4, #ACT_CALLBACK
    ADRG    x5, ring_activate
    b       rings_spin.register
rings_spin.to_engine_path:
    mov     x6, #0
    mvn     x9, x24
    PATH_PTR x5, x9
    ldr     w5, [x5, #PA_NAME]
    mov     w4, #ACT_ACTIVATE_PATH
rings_spin.register:
    bl      event_register
    mov     w0, w19
    mov     w1, w23
    bl      path_activate
    mov     w0, w19
    ldr     w1, [x20, #RINGCH.gscene]
    bl      scene_activate
    b       rings_spin.char
rings_spin.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// rings_disperse: Ring.disperse for every ring - remember the active ring
// path, then loop around a fresh disperse path.
rings_disperse:
    stp     x19, x20, [sp, #-32]!
    stp     x21, x30, [sp, #16]
    mov     x21, #0
rings_disperse.char:
    LDX     x9, ring_list_count
    cmp     x21, x9
    b.hs    rings_disperse.done
    LDX     x9, ring_list
    ldr     w19, [x9, x21, lsl #2]
    add     x21, x21, #1
    LDX     x20, ch_user0
    ldr     x20, [x20, x19, lsl #3]
    LDX     x9, ch_path
    ldr     w9, [x9, x19, lsl #2]
    mov     x3, #0                      // no active path: "0"
    cmn     w9, #1                      // NONE
    b.eq    rings_disperse.last
    ldr     x3, [x20, #RINGCH.cur]
    ldr     w10, [x20, #RINGCH.rpath]
    cmp     w9, w10
    b.eq    rings_disperse.last
    mvn     x3, x9                      // another path (a condense path)
rings_disperse.last:
    str     x3, [x20, #RINGCH.last]
    mov     w0, w19
    bl      char_coord
    mov     x1, x9
    mov     w0, w19
    bl      ring_make_disperse
    mov     w0, w19
    ldr     w1, [x20, #RINGCH.dpath]
    bl      path_activate
    mov     w0, w19
    ldr     w1, [x20, #RINGCH.dscene]
    bl      scene_activate
    b       rings_disperse.char
rings_disperse.done:
    ldp     x21, x30, [sp, #16]
    ldp     x19, x20, [sp], #32
    ret

// rings_final: everyone visible and home; ring characters fade back.
rings_final:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
rings_final.char:
    cbz     x22, rings_final.done
    ldr     w19, [x21], #4
    sub     x22, x22, #1
    mov     w0, w19
    bl      set_visible
    LDX     x9, ch_user1
    add     x9, x9, x19, lsl #3
    ldr     w1, [x9]
    mov     w0, w19
    bl      path_activate
    mov     w0, w19
    bl      active_insert
    LDX     x9, ch_user1
    ldr     x9, [x9, x19, lsl #3]
    tbnz    x9, #RINGS_EXTERNAL_BIT, rings_final.char
    LDX     x9, ch_user0
    ldr     x9, [x9, x19, lsl #3]
    ldr     w1, [x9, #RINGCH.dscene]
    mov     w0, w19
    bl      scene_activate
    b       rings_final.char
rings_final.done:
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// rings_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
rings_next_frame:
    PUSH2   x19, x30
    // Bass scales rotation by up to +50%; the short beat flag adds +25%.
    // Home, disperse and condense paths retain their authored speeds. The
    // factor is recomputed from the baseline, never multiplied into itself.
    LDX     x9, audio_word
    ubfx    w10, w9, #8, #8             // bass
    ucvtf   d0, w10
    LDD     d1, rings_audio_510
    fdiv    d0, d0, d1
    LDD     d1, rings_audio_one
    fadd    d0, d0, d1
    ubfx    w10, w9, #16, #8            // beat
    cbz     w10, rings_next_frame.audio_speed_ready
    LDD     d1, rings_quarter
    fadd    d0, d0, d1
rings_next_frame.audio_speed_ready:
    STD     d0, rings_audio_speed
    LDX     x19, effect_config
    LDB     w9, rings_phase
    cmp     w9, #RINGS_PH_START
    b.eq    rings_next_frame.start
    cmp     w9, #RINGS_PH_DISPERSE
    b.eq    rings_next_frame.disperse
    cmp     w9, #RINGS_PH_SPIN
    b.eq    rings_next_frame.spin
    cmp     w9, #RINGS_PH_FINAL
    b.eq    rings_next_frame.final
    mov     w9, #0                      // complete
    POP2    x19, x30
    ret
rings_next_frame.start:
    LDX     x9, rings_initial_left
    cbnz    x9, rings_next_frame.start_wait
    mov     w9, #RINGS_PH_DISPERSE
    STB     w9, rings_phase
    b       rings_next_frame.update
rings_next_frame.start_wait:
    sub     x9, x9, #1
    STX     x9, rings_initial_left
    b       rings_next_frame.update
rings_next_frame.disperse:
    LDB     w9, rings_initial_done
    cbnz    w9, rings_next_frame.dispersed
    mov     w9, #1
    STB     w9, rings_initial_done
    bl      rings_initial_disperse
    b       rings_next_frame.update
rings_next_frame.dispersed:
    LDX     x9, rings_disperse_left
    cbnz    x9, rings_next_frame.disperse_wait
    mov     w9, #RINGS_PH_SPIN
    STB     w9, rings_phase
    LDX     x9, rings_cycles_left
    sub     x9, x9, #1
    STX     x9, rings_cycles_left
    ldr     x9, [x19, #RINGS.spin_duration]
    STX     x9, rings_spin_left
    bl      rings_spin
    b       rings_next_frame.update
rings_next_frame.disperse_wait:
    sub     x9, x9, #1
    STX     x9, rings_disperse_left
    b       rings_next_frame.update
rings_next_frame.spin:
    LDX     x9, rings_spin_left
    cbnz    x9, rings_next_frame.spin_wait
    LDX     x9, rings_cycles_left
    cbnz    x9, rings_next_frame.again
    mov     w9, #RINGS_PH_FINAL
    STB     w9, rings_phase
    bl      rings_final
    b       rings_next_frame.update
rings_next_frame.again:
    ldr     x9, [x19, #RINGS.disperse_duration]
    STX     x9, rings_disperse_left
    bl      rings_disperse
    mov     w9, #RINGS_PH_DISPERSE
    STB     w9, rings_phase
    b       rings_next_frame.update
rings_next_frame.spin_wait:
    sub     x9, x9, #1
    STX     x9, rings_spin_left
    b       rings_next_frame.update
rings_next_frame.final:
    bl      active_empty
    cbz     w9, rings_next_frame.update
    mov     w9, #RINGS_PH_COMPLETE
    STB     w9, rings_phase
rings_next_frame.update:
    bl      update
    mov     w9, #1
    POP2    x19, x30
    ret

    .section .rodata
    .balign 8
rings_eight_steps:      .quad 8
rings_quarter:          .double 0.25
rings_audio_one:        .double 1.0
rings_audio_510:        .double 510.0
rings_speed_home:       .double 0.8
rings_speed_disperse:   .double 0.14
rings_speed_initial:    .double 0.3
rings_speed_condense:   .double 0.1

    TSTATE
    .balign 8
ring_gap:               .skip 8
rings_array:            .skip 8
ring_count:             .skip 8
ring_list:              .skip 8         // ring characters, ring by ring
ring_list_count:        .skip 8
rings_ext_list:         .skip 8         // non_ring_chars
rings_ext_count:        .skip 8
rings_spin_left:        .skip 8
rings_disperse_left:    .skip 8
rings_cycles_left:      .skip 8
rings_initial_left:     .skip 8
rings_sym:              .skip 8
rings_final_spectrum:   .skip 8
rings_final_map_ptr:    .skip 8
rings_final_map_width:  .skip 8
rings_audio_speed:      .skip 8         // this frame's rotation speed factor
rings_pair_stops:       .skip 8 * 2
rings_pair_spectrum:    .skip 8 * 16
rings_dynamic:          .skip 1
rings_phase:            .skip 1
rings_initial_done:     .skip 1

    .text
