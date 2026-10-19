// effects/swarm.s - "Characters are grouped into swarms and move around the
// terminal before settling into position" (src/effects/swarm.rs).
//
// Config (src/asm/effects.rs, the Swarm arm).
//
// Path names: swarm area a ("{a}_swarm_area") is NAME_LITERAL + a; the inner
// paths are Rust's decimal ids (the path count when made: 3a + 1, 3a + 2) and
// the landing path is the auto id 3k. So "contains swarm_area" is "name >=
// NAME_LITERAL", and int(s[0]) is the leading decimal digit of a.
// Scene names: the flash scene is auto id "0", the landing scene "1".
//
// Upstream's find_coords_on_circle is lru_cached and swarm shuffles the list
// it returns in place, so a later call with the same focus coordinate sees
// the shuffled list. swm_cache keeps one list per coordinate for the run, the
// same as Rust's effect-level circle_cache. find_coords_in_circle is pure, so
// it is cached alongside.

.equ HAVE_swarm, 1

.equ SWARM.base_colors,         0       // *const u64
.equ SWARM.base_count,          8
.equ SWARM.flash_color,         16
.equ SWARM.swarm_size,          24      // f64
.equ SWARM.coordination,        32      // f64
.equ SWARM.area_lo,             40
.equ SWARM.area_hi,             48
.equ SWARM.final_stops,         56      // *const u64
.equ SWARM.final_stop_count,    64
.equ SWARM.final_steps,         72      // *const i64
.equ SWARM.final_step_count,    80
.equ SWARM.final_direction,     88
.equ SWARM_size,                96

// circle cache entry (32 bytes); an empty slot has on_count 0
.equ SWM_CE_KEY,            0
.equ SWM_CE_ON,             8           // find_coords_on_circle list
.equ SWM_CE_ON_N,           16          // u32
.equ SWM_CE_IN_N,           20          // u32
.equ SWM_CE_IN,             24          // find_coords_in_circle list

// a swarm's area map entry (32 bytes): key coordinate, in-circle list, count
.equ SWM_AR_KEY,            0
.equ SWM_AR_IN,             8
.equ SWM_AR_IN_N,           16

.equ SWM_FLASH_SCENE,       0
.equ SWM_LAND_SCENE,        1

.equ SWM_EASE_OUT_SINE,     2
.equ SWM_EASE_IN_OUT_SINE,  3
.equ SWM_EASE_IN_OUT_QUAD,  6

.equ SWM_HASH_MUL,          0x9e3779b97f4a7c15

// SWM_EVENT slot, event, caller path name, action, arg0 (no arg1)
.macro SWM_EVENT slot, ev, name, act, arg0
    mov     w0, \slot
    mov     w1, #\ev
    mov     w2, #CALLER_PATH
    mov     w3, \name
    mov     w4, #\act
    mov     x5, #\arg0
    mov     x6, #0
    bl      event_register
.endm

    .text

// swarm_build: Swarm::build.
// [sp, #56]: the next focus coordinate.
swarm_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    cset    w9, eq
    STB     w9, swm_dynamic
    // swarm_size = max(round(len(characters) * swarm_size), 1)
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    scvtf   d0, x2
    ldr     d1, [x19, #SWARM.swarm_size]
    fmul    d0, d0, d1
    bl      round_half_even
    mov     x3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    STX     x9, swm_size
    bl      swm_make_swarms
    bl      swm_make_final_map
    // radius = max(min(right, top) // 2, 1); area diameter max(.. // 6, 1) * 2
    LDX     x21, canvas_right
    LDX     x9, canvas_top
    cmp     x21, x9
    csel    x21, x9, x21, gt
    mov     x0, x21
    mov     x1, #2
    bl      floor_div
    mov     x3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    STX     x9, swm_radius
    mov     x0, x21
    mov     x1, #6
    bl      floor_div
    mov     x3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    add     x9, x9, x9
    STX     x9, swm_diameter
    // the circle cache
    mov     x9, #63
    STX     x9, swm_cache_mask
    mov     x0, #(64 * 32)
    bl      alloc
    STX     x9, swm_cache
    // a swarm's area keys are distinct coordinates in [0, right + 1] x
    // [0, top + 1] (spawns sit one outside the canvas), and at most area_hi
    LDX     x9, canvas_right
    add     x9, x9, #2
    LDX     x3, canvas_top
    add     x3, x3, #2
    mul     x9, x9, x3
    ldr     x3, [x19, #SWARM.area_hi]
    cmp     x9, x3
    csel    x9, x3, x9, gt
    mov     x21, x9
    lsl     x0, x9, #5
    bl      alloc
    STX     x9, swm_areas
    add     x0, x21, x21, lsl #1
    add     x0, x0, #1
    lsl     x0, x0, #2
    bl      alloc
    STX     x9, swm_names
    mov     x21, #0                     // swarm index
swarm_build.swarm:
    LDX     x9, swm_nswarms
    cmp     x21, x9
    b.hs    swarm_build.built
    // swarm gradient: base -> flash in 7 steps, mirrored around 10 flashes
    ldr     x0, [x19, #SWARM.base_count]
    bl      rng_below
    ldr     x3, [x19, #SWARM.base_colors]
    ldr     x9, [x3, x9, lsl #3]
    ADRG    x0, swm_pair
    str     x9, [x0]
    STX     x9, swm_base
    ldr     x9, [x19, #SWARM.flash_color]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, swm_seven
    mov     w3, #1
    ADRG    x4, swm_spectrum
    bl      gradient_new
    ADRG    x1, swm_spectrum
    ADRG    x0, swm_mirror
    mov     w3, #0
swarm_build.up:
    ldr     x2, [x1, x3, lsl #3]
    str     x2, [x0], #8
    add     w3, w3, #1
    cmp     w3, w9
    b.lo    swarm_build.up
    ldr     x2, [x19, #SWARM.flash_color]
    mov     w3, #10
swarm_build.flash:
    str     x2, [x0], #8
    subs    w3, w3, #1
    b.ne    swarm_build.flash
    mov     w3, w9
swarm_build.down:
    sub     w3, w3, #1
    ldr     x2, [x1, x3, lsl #3]
    str     x2, [x0], #8
    cbnz    w3, swarm_build.down
    lsl     w9, w9, #1
    add     w9, w9, #10
    STX     x9, swm_mirror_len
    // spawn and the swarm areas
    mov     w0, #1
    mov     w1, #0
    bl      canvas_random_coord
    STX     x9, swm_spawn
    ldr     x0, [x19, #SWARM.area_lo]
    ldr     x1, [x19, #SWARM.area_hi]
    bl      rng_randint
    mov     x22, x9                     // swarm_area_count
    mov     x23, #0                     // len(swarm_areas)
    LDX     x24, swm_spawn              // last_focus_coord
    STX     xzr, swm_k
swarm_build.area:
    cmp     x23, x22
    b.ge    swarm_build.areas_done
    mov     x0, x24
    bl      swm_cache_get
    mov     x20, x9
    ldr     x0, [x20, #SWM_CE_ON]
    ldr     w1, [x20, #SWM_CE_ON_N]
    bl      rng_shuffle64
    // the first shuffled coordinate on the canvas, else a random one
    ldr     x3, [x20, #SWM_CE_ON]
    ldr     w2, [x20, #SWM_CE_ON_N]
swarm_build.scan:
    cbz     w2, swarm_build.random
    ldr     x1, [x3]
    bl      coord_in_canvas
    cbnz    w9, swarm_build.next_focus
    add     x3, x3, #8
    sub     w2, w2, #1
    b       swarm_build.scan
swarm_build.random:
    mov     w0, #0
    mov     w1, #0
    bl      canvas_random_coord
    mov     x1, x9
swarm_build.next_focus:
    str     x1, [sp, #56]
    add     x23, x23, #1
    // swarm_area_coordinate_map[last_focus_coord] = find_coords_in_circle(..)
    // (a repeated key keeps its position and gets the same list)
    LDX     x9, swm_areas
    LDX     x3, swm_k
swarm_build.find_key:
    cbz     x3, swarm_build.new_key
    ldr     x2, [x9, #SWM_AR_KEY]
    cmp     x2, x24
    b.eq    swarm_build.keyed
    add     x9, x9, #32
    sub     x3, x3, #1
    b       swarm_build.find_key
swarm_build.new_key:
    str     x24, [x9, #SWM_AR_KEY]
    ldr     x3, [x20, #SWM_CE_IN]
    str     x3, [x9, #SWM_AR_IN]
    ldr     w3, [x20, #SWM_CE_IN_N]
    str     x3, [x9, #SWM_AR_IN_N]
    LDX     x3, swm_k
    add     x3, x3, #1
    STX     x3, swm_k
swarm_build.keyed:
    ldr     x24, [sp, #56]
    b       swarm_build.area
swarm_build.areas_done:
    // path names in insertion order, for chain_paths
    LDX     x0, swm_names
    LDX     x10, swm_k
    mov     x3, #0
    mov     x2, #0                      // path count
swarm_build.name:
    cmp     x3, x10
    b.hs    swarm_build.names_done
    orr     w9, w3, #NAME_LITERAL
    add     x8, x0, x2, lsl #2
    str     w9, [x8]
    add     w9, w2, #1
    str     w9, [x8, #4]
    add     w9, w2, #2
    str     w9, [x8, #8]
    add     w2, w2, #3
    add     x3, x3, #1
    b       swarm_build.name
swarm_build.names_done:
    str     w2, [x0, x2, lsl #2]        // the landing path
    // every character of the swarm
    LDX     x9, swm_bounds
    add     x9, x9, x21, lsl #4
    ldp     x23, x24, [x9]
swarm_build.char:
    cmp     x23, x24
    b.hs    swarm_build.next_swarm
    LDX     x9, swm_order
    ldr     w0, [x9, x23, lsl #2]
    bl      swm_build_char
    add     x23, x23, #1
    b       swarm_build.char
swarm_build.next_swarm:
    add     x21, x21, #1
    b       swarm_build.swarm
swarm_build.built:
    mov     w9, #1
    STB     w9, swm_call_next
    mov     w9, #NAME_LITERAL
    STW     w9, swm_active_area
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// swm_make_swarms: SwarmIterator.make_swarms. Swarms pop from the end of
// the bottom-to-top, right-to-left list, so swm_order is that list reversed
// and swarm j is the range [j * size, (j + 1) * size) of it; a final swarm
// smaller than size // 2 joins the one before.
swm_make_swarms:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_BOTTOM_TO_TOP_R2L
    bl      get_characters
    mov     x19, x9
    mov     x21, x2
    lsl     x0, x2, #2
    bl      alloc
    STX     x9, swm_order
    add     x3, x19, x21, lsl #2
    sub     x3, x3, #4
    mov     x2, #0
swm_make_swarms.reverse:
    cmp     x2, x21
    b.hs    swm_make_swarms.ranges
    ldr     w1, [x3]
    str     w1, [x9, x2, lsl #2]
    sub     x3, x3, #4
    add     x2, x2, #1
    b       swm_make_swarms.reverse
swm_make_swarms.ranges:
    LDX     x22, swm_size
    add     x9, x21, x22
    sub     x9, x9, #1
    udiv    x9, x9, x22                 // ceil(count / size)
    STX     x9, swm_nswarms
    lsl     x0, x9, #3
    add     x0, x0, #8
    lsl     x0, x0, #1
    bl      alloc
    STX     x9, swm_bounds
    LDX     x10, swm_nswarms
    mov     x3, #0
    mov     x2, #0
swm_make_swarms.range:
    cmp     x3, x10
    b.hs    swm_make_swarms.final
    str     x2, [x9]
    add     x2, x2, x22
    cmp     x2, x21
    csel    x2, x21, x2, hi
    str     x2, [x9, #8]
    add     x9, x9, #16
    add     x3, x3, #1
    b       swm_make_swarms.range
swm_make_swarms.final:
    // x9 = past the last range
    ldur    x3, [x9, #-8]
    ldur    x10, [x9, #-16]
    sub     x3, x3, x10                 // len(final_swarm)
    lsr     x2, x22, #1                 // size // 2 (size >= 1)
    cmp     x3, x2
    b.ge    swm_make_swarms.done
    LDX     x10, swm_nswarms
    cmp     x10, #1
    b.ls    swm_make_swarms.done        // (Rust panics; unreachable for ratios <= 1)
    stur    x21, [x9, #-24]
    sub     x10, x10, #1
    STX     x10, swm_nswarms
swm_make_swarms.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// swm_build_char(w0=slot): one swarm character's flash scene, swarm area
// and inner paths, landing path and scene, events and chain.
swm_build_char:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w19, w0
    LDX     x1, swm_spawn
    bl      set_coordinate
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #SCF_SYNC_DISTANCE
    mov     w3, #NONE
    bl      scene_new
    mov     w21, w9
    // the swarm's mirrored gradient over the input symbol; the visuals are
    // shared by symbol and base color (tagged apart from the landing keys)
    LDX     x0, ch_sym
    ldr     x0, [x0, x19, lsl #3]
    ADRG    x1, swm_mirror
    LDX     x2, swm_mirror_len
    mov     x3, #NONE
    LDX     x4, swm_base
    orr     x4, x4, #(1 << 47)
    bl      visual_run
    mov     w0, w21
    mov     x1, x9
    mov     w3, #1
    bl      visual_frames
swm_build_char.areas:
    mov     x22, #0                     // area index
swm_build_char.area:
    LDX     x9, swm_k
    cmp     x22, x9
    b.hs    swm_build_char.landing
    LDX     x20, swm_areas
    add     x20, x20, x22, lsl #5
    ldr     x0, [x20, #SWM_AR_IN_N]
    bl      rng_below
    ldr     x3, [x20, #SWM_AR_IN]
    ldr     x23, [x3, x9, lsl #3]
    orr     w24, w22, #NAME_LITERAL
    mov     w0, w19
    LDD     d0, swm_speed_area
    mov     w1, #SWM_EASE_OUT_SINE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, w24
    bl      path_new
    mov     w0, w9
    mov     x1, x23
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    SWM_EVENT w19, EV_PATH_ACTIVATED, w24, ACT_ACTIVATE_SCENE, SWM_FLASH_SCENE
    SWM_EVENT w19, EV_PATH_ACTIVATED, w24, ACT_SET_LAYER, 1
    SWM_EVENT w19, EV_PATH_COMPLETE, w24, ACT_DEACTIVATE_SCENE, NONE
    // two inner paths, named by the path count
    add     w24, w22, w22, lsl #1
    add     w24, w24, #1
    bl      swm_inner_path
    add     w24, w24, #1
    bl      swm_inner_path
    add     x22, x22, #1
    b       swm_build_char.area
swm_build_char.landing:
    LDX     x24, swm_k
    add     w24, w24, w24, lsl #1       // the landing path's auto id
    mov     w0, w19
    LDD     d0, swm_speed_land
    mov     w1, #SWM_EASE_IN_OUT_QUAD
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, w24
    bl      path_new
    mov     w23, w9
    mov     w0, w19
    bl      char_input_coord
    mov     x1, x9
    mov     w0, w23
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w0, w9
    mov     w1, w19
    bl      swm_landing_frames
    SWM_EVENT w19, EV_PATH_COMPLETE, w24, ACT_ACTIVATE_SCENE, SWM_LAND_SCENE
    SWM_EVENT w19, EV_PATH_COMPLETE, w24, ACT_SET_LAYER, 0
    SWM_EVENT w19, EV_PATH_ACTIVATED, w24, ACT_ACTIVATE_SCENE, SWM_FLASH_SCENE
    mov     w0, w19
    LDX     x1, swm_names
    add     w2, w24, #1
    mov     w3, #0
    bl      chain_paths
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// swm_inner_path: in swm_build_char's frame (w19 = slot, x20 = area entry,
// w24 = name) - choose a coordinate of the area, then the path to it.
// [sp, #8]: the coordinate.
swm_inner_path:
    str     x30, [sp, #-16]!
    ldr     x0, [x20, #SWM_AR_IN_N]
    bl      rng_below
    ldr     x3, [x20, #SWM_AR_IN]
    ldr     x9, [x3, x9, lsl #3]
    str     x9, [sp, #8]
    mov     w0, w19
    LDD     d0, swm_speed_inner
    mov     w1, #SWM_EASE_IN_OUT_SINE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, w24
    bl      path_new
    mov     w0, w9
    ldr     x1, [sp, #8]
    ldr     x30, [sp], #16
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    b       path_new_waypoint

// swm_landing_frames(w0=scene, w1=slot): the landing scene - flash to the
// final color in 10 steps (3 ticks each); when dynamic, flash to the input
// colors, or to white and then no color when the character has none.
swm_landing_frames:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    mov     w19, w0
    mov     w21, w1
    LDX     x9, ch_sym
    ldr     x9, [x9, x21, lsl #3]
    STX     x9, swm_sym
    LDX     x9, effect_config
    ldr     x9, [x9, #SWARM.flash_color]
    STX     x9, swm_pair
    LDB     w9, swm_dynamic
    cbnz    w9, swm_landing_frames.dynamic
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x21, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, swm_final_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x21, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, swm_final_map
    ldr     x24, [x3, x9, lsl #3]
    // flash -> final color over the symbol, shared by symbol and color
    LDX     x0, swm_sym
    mov     x1, x24
    mov     x2, #NONE
    bl      visual_run_find
    cbnz    x9, swm_landing_frames.plain_frames
    mov     x9, x24
    bl      swm_to_color                // w9 = 11
    LDX     x0, swm_sym
    ADRG    x1, swm_spectrum
    mov     w2, w9
    mov     x3, #NONE
    mov     x4, x24
    bl      visual_run
swm_landing_frames.plain_frames:
    mov     w0, w19
    mov     x1, x9
    mov     w3, #3
    bl      visual_frames
    b       swm_landing_frames.done
swm_landing_frames.dynamic:
    LDX     x22, ch_fg
    ldr     x22, [x22, x21, lsl #3]
    LDX     x23, ch_bg
    ldr     x23, [x23, x21, lsl #3]
    cmn     x22, #1                     // NONE
    b.ne    swm_landing_frames.gradients
    cmn     x23, #1
    b.ne    swm_landing_frames.gradients
    mov     x9, #0xffffff               // DYNAMIC_CLEAR_COLOR
    bl      swm_to_color
    mov     w22, w9
    b       swm_landing_frames.plain_then_clear
swm_landing_frames.gradients:
    // fg into swm_spectrum, bg into swm_spectrum2 (either may be absent)
    mov     w24, #0                     // fg count
    cmn     x22, #1
    b.eq    swm_landing_frames.bg
    mov     x9, x22
    bl      swm_to_color
    mov     w24, w9
swm_landing_frames.bg:
    mov     w21, #0                     // bg count
    cmn     x23, #1
    b.eq    swm_landing_frames.apply
    ADRG    x0, swm_pair
    str     x23, [x0, #8]
    mov     w1, #2
    ADRG    x2, swm_ten
    mov     w3, #1
    ADRG    x4, swm_spectrum2
    bl      gradient_new
    mov     w21, w9
swm_landing_frames.apply:
    mov     x6, #0
    cbz     w21, swm_landing_frames.no_bg
    ADRG    x6, swm_spectrum2
swm_landing_frames.no_bg:
    mov     x7, x21
    mov     w0, w19
    ADRG    x1, swm_sym
    mov     w2, #1
    mov     w3, #3
    mov     x4, #0
    cbz     w24, swm_landing_frames.no_fg
    ADRG    x4, swm_spectrum
swm_landing_frames.no_fg:
    mov     w5, w24
    bl      scene_apply_gradient
    b       swm_landing_frames.done
swm_landing_frames.plain_then_clear:
    mov     w23, #0
swm_landing_frames.clear:
    cmp     w23, w22
    b.hs    swm_landing_frames.no_color
    ADRG    x9, swm_spectrum
    ldr     x3, [x9, x23, lsl #3]
    LDX     x1, swm_sym
    mov     w0, w19
    mov     w2, #3
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    add     w23, w23, #1
    b       swm_landing_frames.clear
swm_landing_frames.no_color:
    LDX     x1, swm_sym
    mov     w0, w19
    mov     w2, #3
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
swm_landing_frames.done:
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// swm_to_color(x9=color) -> w9 = length: Gradient::with_steps([flash
// (already in swm_pair), color], 10) into swm_spectrum.
swm_to_color:
    ADRG    x0, swm_pair
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, swm_ten
    mov     w3, #1
    ADRG    x4, swm_spectrum
    b       gradient_new

// swm_make_final_map: Gradient::new(final stops, final steps) and its coordinate
// mapping over the text rectangle.
swm_make_final_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #SWARM.final_steps]
    ldr     x3, [x19, #SWARM.final_step_count]
    ldr     x1, [x19, #SWARM.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, swm_final_spectrum
    ldr     x0, [x19, #SWARM.final_stops]
    ldr     x1, [x19, #SWARM.final_stop_count]
    ldr     x2, [x19, #SWARM.final_steps]
    ldr     x3, [x19, #SWARM.final_step_count]
    LDX     x4, swm_final_spectrum
    bl      gradient_new
    LDX     x0, swm_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, swm_final_map_width
    ldr     x6, [x19, #SWARM.final_direction]
    bl      gradient_map
    STX     x9, swm_final_map
    POP2    x19, x30
    ret

// ------------------------------------------------------------ circle cache

// swm_cache_get(x0=coord) -> x9 = the cache entry for the coordinate (valid
// until the next call), made on first use: find_coords_on_circle(coord,
// radius, 0, unique) and find_coords_in_circle(coord, diameter).
swm_cache_get:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     x19, x0
swm_cache_get.probe:
    MOV64   x3, SWM_HASH_MUL
    mul     x9, x19, x3
    lsr     x9, x9, #32
    LDX     x2, swm_cache_mask
swm_cache_get.slot:
    and     x9, x9, x2
    LDX     x21, swm_cache
    add     x21, x21, x9, lsl #5
    ldr     w3, [x21, #SWM_CE_ON_N]
    cbz     w3, swm_cache_get.miss
    ldr     x3, [x21, #SWM_CE_KEY]
    cmp     x3, x19
    b.eq    swm_cache_get.hit
    add     x9, x9, #1
    b       swm_cache_get.slot
swm_cache_get.miss:
    // keep the table at most half full
    LDX     x9, swm_cache_used
    add     x9, x9, #1
    add     x9, x9, x9
    LDX     x3, swm_cache_mask
    add     x3, x3, #1
    cmp     x9, x3
    b.ls    swm_cache_get.insert
    bl      swm_cache_grow
    b       swm_cache_get.probe
swm_cache_get.insert:
    LDX     x9, swm_cache_used
    add     x9, x9, #1
    STX     x9, swm_cache_used
    str     x19, [x21, #SWM_CE_KEY]
    mov     x0, x19
    LDX     x1, swm_radius
    mov     x2, #0
    mov     w3, #1
    bl      find_coords_on_circle
    str     x9, [x21, #SWM_CE_ON]
    str     w2, [x21, #SWM_CE_ON_N]
    mov     x0, x19
    LDX     x1, swm_diameter
    bl      find_coords_in_circle
    str     x9, [x21, #SWM_CE_IN]
    str     w2, [x21, #SWM_CE_IN_N]
swm_cache_get.hit:
    mov     x9, x21
    POP2    x22, x30
    POP2    x19, x21
    ret

// swm_cache_grow: double the table and rehash.
swm_cache_grow:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x21, swm_cache
    LDX     x22, swm_cache_mask
    add     x0, x22, #1
    lsl     x0, x0, #6
    bl      alloc
    STX     x9, swm_cache
    lsl     x9, x22, #1
    add     x9, x9, #1
    STX     x9, swm_cache_mask
    add     x19, x22, #1                // old slots
swm_cache_grow.entry:
    cbz     x19, swm_cache_grow.done
    sub     x19, x19, #1
    add     x1, x21, x19, lsl #5
    ldr     w3, [x1, #SWM_CE_ON_N]
    cbz     w3, swm_cache_grow.entry
    ldr     x9, [x1, #SWM_CE_KEY]
    MOV64   x3, SWM_HASH_MUL
    mul     x9, x9, x3
    lsr     x9, x9, #32
    LDX     x2, swm_cache_mask
swm_cache_grow.slot:
    and     x9, x9, x2
    LDX     x0, swm_cache
    add     x0, x0, x9, lsl #5
    ldr     w3, [x0, #SWM_CE_ON_N]
    cbz     w3, swm_cache_grow.move
    add     x9, x9, #1
    b       swm_cache_grow.slot
swm_cache_grow.move:
    ldp     q0, q1, [x1]
    stp     q0, q1, [x0]
    b       swm_cache_grow.entry
swm_cache_grow.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// ------------------------------------------------------------ frames

// swm_first_digit(w9=n >= 0) -> w9 = the leading decimal digit of n.
// Clobbers x3.
swm_first_digit:
    mov     w3, #10
swm_first_digit.loop:
    cmp     w9, #10
    b.lo    swm_first_digit.done
    udiv    w9, w9, w3
    b       swm_first_digit.loop
swm_first_digit.done:
    ret

// swarm_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
swarm_next_frame:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x9, swm_nswarms
    cbnz    x9, swarm_next_frame.running
    bl      active_empty
    cbnz    w9, swarm_next_frame.finished
swarm_next_frame.running:
    LDX     x9, swm_nswarms
    cbz     x9, swarm_next_frame.landed
    LDB     w9, swm_call_next
    cbz     w9, swarm_next_frame.landed
    // the next swarm (from the end) takes off for its first area
    STB     wzr, swm_call_next
    LDX     x9, swm_nswarms
    sub     x9, x9, #1
    STX     x9, swm_nswarms
    LDX     x3, swm_bounds
    add     x9, x3, x9, lsl #4
    ldp     x3, x2, [x9]
    STX     x3, swm_cur_start
    STX     x2, swm_cur_end
    mov     w9, #NAME_LITERAL
    STW     w9, swm_active_area
    LDX     x21, swm_cur_start
swarm_next_frame.launch:
    LDX     x10, swm_cur_end
    cmp     x21, x10
    b.hs    swarm_next_frame.landed
    LDX     x9, swm_order
    ldr     w19, [x9, x21, lsl #2]
    mov     w0, w19
    mov     w1, #NAME_LITERAL
    bl      path_activate_name
    mov     w0, w19
    bl      set_visible
    mov     w0, w19
    bl      active_insert
    add     x21, x21, #1
    b       swarm_next_frame.launch
swarm_next_frame.landed:
    // some of the characters have landed
    bl      active_count
    LDX     x3, swm_cur_end
    LDX     x10, swm_cur_start
    sub     x3, x3, x10
    cmp     x9, x3
    b.hs    swarm_next_frame.follow
    mov     w9, #1
    STB     w9, swm_call_next
swarm_next_frame.follow:
    // the first character to reach a later swarm area leads the others there
    LDX     x21, swm_cur_start
swarm_next_frame.lead:
    LDX     x10, swm_cur_end
    cmp     x21, x10
    b.hs    swarm_next_frame.update
    LDX     x9, swm_order
    ldr     w19, [x9, x21, lsl #2]
    add     x21, x21, #1
    LDX     x9, ch_path
    ldr     w9, [x9, x19, lsl #2]
    cmn     w9, #1                      // NONE
    b.eq    swarm_next_frame.lead
    // the path's name, cached per character (ch_user0: path + 1, name)
    // while the character stays on it: path records are cold
    LDX     x2, ch_user0
    add     x8, x2, x19, lsl #3
    add     w3, w9, #1
    ldr     w10, [x8]
    cmp     w3, w10
    b.ne    swarm_next_frame.name
    ldr     w20, [x8, #4]
    b       swarm_next_frame.named
swarm_next_frame.name:
    str     w3, [x8]
    PATH_PTR x3, x9
    ldr     w20, [x3, #PA_NAME]
    str     w20, [x8, #4]
swarm_next_frame.named:
    LDW     w10, swm_active_area
    cmp     w20, w10
    b.eq    swarm_next_frame.lead
    tbz     w20, #31, swarm_next_frame.lead  // below NAME_LITERAL: not a swarm area
    and     w9, w10, #0x7fffffff
    bl      swm_first_digit
    mov     w22, w9
    and     w9, w20, #0x7fffffff
    bl      swm_first_digit
    cmp     w9, w22
    b.ls    swarm_next_frame.lead
    STW     w20, swm_active_area
    LDX     x23, effect_config
    LDX     x22, swm_cur_start
swarm_next_frame.coordinate:
    LDX     x10, swm_cur_end
    cmp     x22, x10
    b.hs    swarm_next_frame.update
    LDX     x9, swm_order
    ldr     w24, [x9, x22, lsl #2]
    add     x22, x22, #1
    cmp     w24, w19
    b.eq    swarm_next_frame.coordinate
    bl      rng_random
    ldr     d1, [x23, #SWARM.coordination]
    fcmp    d1, d0
    b.le    swarm_next_frame.coordinate  // random() < coordination, ordered
    mov     w0, w24
    mov     w1, w20
    bl      path_activate_name
    b       swarm_next_frame.coordinate
swarm_next_frame.update:
    bl      update
    mov     w9, #1
    b       swarm_next_frame.out
swarm_next_frame.finished:
    mov     w9, #0
swarm_next_frame.out:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

    .section .rodata
    .balign 8
swm_seven:          .quad 7
swm_ten:            .quad 10
swm_speed_area:     .double 0.4
swm_speed_inner:    .double 0.18
swm_speed_land:     .double 0.45

    TSTATE
    .balign 8
swm_size:           .skip 8
swm_base:           .skip 8         // the swarm's base color
swm_order:          .skip 8         // u32 slots, in swarm order
swm_bounds:         .skip 8         // (start, end) per swarm
swm_nswarms:        .skip 8         // swarms not yet launched
swm_radius:         .skip 8
swm_diameter:       .skip 8
swm_cache:          .skip 8
swm_cache_mask:     .skip 8
swm_cache_used:     .skip 8
swm_areas:          .skip 8
swm_k:              .skip 8         // entries of the current area map
swm_names:          .skip 8
swm_spawn:          .skip 8
swm_mirror_len:     .skip 8
swm_sym:            .skip 8
swm_final_spectrum: .skip 8
swm_final_map:      .skip 8
swm_final_map_width: .skip 8
swm_cur_start:      .skip 8
swm_cur_end:        .skip 8
swm_pair:           .skip 8 * 2
swm_spectrum:       .skip 8 * 16
swm_spectrum2:      .skip 8 * 16
swm_mirror:         .skip 8 * 32
swm_active_area:    .skip 4
swm_dynamic:        .skip 1
swm_call_next:      .skip 1

    .text
