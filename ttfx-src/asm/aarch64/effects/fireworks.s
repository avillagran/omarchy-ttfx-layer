// effects/fireworks.s - "Characters launch and explode like fireworks and
// fall into place" (src/effects/fireworks.rs).
//
// Config (src/asm/effects.rs, the Fireworks arm).
//
// Rust's shells are consecutive runs of the top-to-bottom order: shell 0 is
// always empty (the loop pushes the empty accumulator at the first
// boundary), and shell k >= 1 holds characters [(k - 1) * volume, k *
// volume). They are launched from the last one down, the empty shell last,
// and every launch draws the next delay, so only the count is kept here.

.equ HAVE_fireworks, 1

.equ FIREWORKS.explode_anywhere,    0
.equ FIREWORKS.colors,              8       // *const u64
.equ FIREWORKS.color_count,         16
.equ FIREWORKS.symbol,              24      // packed firework symbol
.equ FIREWORKS.volume,              32      // f64
.equ FIREWORKS.launch_delay,        40
.equ FIREWORKS.explode_distance,    48      // f64
.equ FIREWORKS.final_stops,         56      // *const u64
.equ FIREWORKS.final_stop_count,    64
.equ FIREWORKS.final_steps,         72      // *const i64
.equ FIREWORKS.final_step_count,    80
.equ FIREWORKS.final_direction,     88
.equ FIREWORKS_size,                96

// path names
.equ FW_PATH_APEX,          NAME_LITERAL + 0
.equ FW_PATH_INPUT,         NAME_LITERAL + 1
// scene names
.equ FW_SCN_FALL,           NAME_LITERAL + 0

.equ FW_EASE_IN_OUT_QUART,  12
.equ FW_EASE_OUT_EXPO,      17
.equ FW_EASE_OUT_CIRC,      20

.equ FW_WHITE,              0xffffff

    .text

// fireworks_build: Fireworks::build (prepare_waypoints, prepare_scenes).
fireworks_build:
    PUSH2   x19, x30
    LDX     x19, effect_config
    // firework_volume = max(1, round(volume * input characters))
    LDX     x9, input_count
    scvtf   d0, x9
    ldr     d1, [x19, #FIREWORKS.volume]
    fmul    d0, d0, d1
    bl      round_half_even
    mov     x3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    STX     x9, fw_volume
    // explode_distance = min(15, max(1, round(right * explode_distance)))
    LDX     x9, canvas_right
    scvtf   d0, x9
    ldr     d1, [x19, #FIREWORKS.explode_distance]
    fmul    d0, d0, d1
    bl      round_half_even
    mov     x3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    mov     x3, #15
    cmp     x9, #15
    csel    x9, x3, x9, gt
    STX     x9, fw_distance
    STX     xzr, fw_delay
    bl      fw_prepare_waypoints
    bl      fw_prepare_scenes
    POP2    x19, x30
    ret

// fw_prepare_waypoints: FireworksIterator.prepare_waypoints - the apex,
// explode and input paths of every character and their chain of events.
fw_prepare_waypoints:
    sub     sp, sp, #96                 // [sp] origin, [+8] explode wpt,
                                        // [+16] control, [+24] path index
    stp     x19, x20, [sp, #32]
    stp     x21, x22, [sp, #48]
    stp     x23, x24, [sp, #64]
    str     x30, [sp, #80]
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    STX     x9, fw_chars
    STX     x2, fw_count
    // shells: the empty one, then ceil(count / volume)
    mov     x3, #0
    cbz     x2, fw_prepare_waypoints.shells
    LDX     x10, fw_volume
    add     x9, x2, x10
    sub     x9, x9, #1
    udiv    x9, x9, x10
    add     x3, x9, #1
fw_prepare_waypoints.shells:
    STX     x3, fw_shells
    mov     x21, #0                     // position
    mov     x24, #0                     // position within the shell
fw_prepare_waypoints.char:
    LDX     x10, fw_count
    cmp     x21, x10
    b.hs    fw_prepare_waypoints.done
    LDX     x9, fw_chars
    ldr     w19, [x9, x21, lsl #2]
    cbz     x24, fw_prepare_waypoints.boundary
    LDX     x10, fw_volume
    cmp     x24, x10
    b.ne    fw_prepare_waypoints.paths
    mov     x24, #0
fw_prepare_waypoints.boundary:
    // a new shell: origin and its explode circle
    mov     x0, #0
    LDX     x1, canvas_right
    bl      rng_randrange
    mov     x22, x9                     // origin_x
    mov     x0, #1                      // canvas bottom
    LDX     x9, effect_config
    ldr     x10, [x9, #FIREWORKS.explode_anywhere]
    cbnz    x10, fw_prepare_waypoints.min_row
    LDX     x9, ch_irow
    ldrsw   x0, [x9, x19, lsl #2]
fw_prepare_waypoints.min_row:
    LDX     x1, canvas_top
    add     x1, x1, #1
    bl      rng_randrange
    lsl     x9, x9, #32
    mov     w3, w22
    orr     x9, x9, x3
    STX     x9, fw_origin
    mov     x0, x9
    LDX     x1, fw_distance
    bl      fw_fill_circle
fw_prepare_waypoints.paths:
    add     x24, x24, #1
    // start at (origin_x, canvas bottom); apex: 0.35, out_expo, layer 2
    mov     w1, w22
    orr     x1, x1, #(1 << 32)
    mov     w0, w19
    bl      set_coordinate
    mov     w0, w19
    LDD     d0, fw_speed_apex
    mov     w1, #FW_EASE_OUT_EXPO
    mov     x2, #2
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, FW_PATH_APEX
    bl      path_new
    mov     w0, w9
    LDX     x1, fw_origin
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // explode: uniform(0.2, 0.4), out_circ, layer 2, auto id
    LDD     d0, fw_speed_explode_lo
    LDD     d1, fw_speed_explode_hi
    bl      rng_uniform
    mov     w0, w19
    mov     w1, #FW_EASE_OUT_CIRC
    mov     x2, #2
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    str     x9, [sp, #24]
    LDX     x0, fw_circle_count
    bl      rng_below
    LDX     x3, fw_circle
    ldr     x1, [x3, x9, lsl #3]
    str     x1, [sp, #8]
    ldr     w0, [sp, #24]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // bloom: past the explode waypoint by distance // 2, raised 7 rows
    LDX     x9, fw_distance
    asr     x9, x9, #1
    scvtf   d0, x9
    LDX     x0, fw_origin
    ldr     x1, [sp, #8]
    bl      extrapolate_along_ray
    str     x9, [sp, #16]
    asr     x3, x9, #32
    sub     x3, x3, #7
    mov     x2, #1
    cmp     x3, #1
    csel    x3, x2, x3, lt
    lsl     x3, x3, #32
    mov     w23, w9                     // bloom column
    orr     x1, x23, x3
    ldr     w0, [sp, #24]
    add     x2, sp, #16
    mov     w3, #1
    mov     w4, #AUTO
    bl      path_new_waypoint
    // input: 0.6, in_out_quart, layer 2, curving through (bloom column, 1)
    mov     w0, w19
    LDD     d0, fw_speed_input
    mov     w1, #FW_EASE_IN_OUT_QUART
    mov     x2, #2
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, FW_PATH_INPUT
    bl      path_new
    mov     w20, w9
    orr     x9, x23, #(1 << 32)
    str     x9, [sp, #16]
    mov     w0, w19
    bl      char_input_coord
    mov     x1, x9
    mov     w0, w20
    add     x2, sp, #16
    mov     w3, #1
    mov     w4, #AUTO
    bl      path_new_waypoint
    // apex -> explode -> input -> layer 0
    ldr     x9, [sp, #24]
    PATH_PTR x3, x9
    ldr     w20, [x3, #PA_NAME]         // the explode path's auto id
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    MOV64   w3, FW_PATH_APEX
    mov     w4, #ACT_ACTIVATE_PATH
    mov     w5, w20
    mov     x6, #0
    bl      event_register
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w3, w20
    mov     w4, #ACT_ACTIVATE_PATH
    MOV64   w5, FW_PATH_INPUT
    mov     x6, #0
    bl      event_register
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    MOV64   w3, FW_PATH_INPUT
    mov     w4, #ACT_SET_LAYER
    mov     w5, #0
    mov     x6, #0
    bl      event_register
    mov     w0, w19
    MOV64   w1, FW_PATH_APEX
    bl      path_activate_name
    add     x21, x21, #1
    b       fw_prepare_waypoints.char
fw_prepare_waypoints.done:
    ldr     x30, [sp, #80]
    ldp     x23, x24, [sp, #64]
    ldp     x21, x22, [sp, #48]
    ldp     x19, x20, [sp, #32]
    add     sp, sp, #96
    ret

// fw_fill_circle(x0=origin, x1=distance): find_coords_in_circle into
// fw_circle / fw_circle_count, reusing one buffer for every shell (it grows
// by doubling, so the build allocates a few lists, not one per shell).
// Clobbers C.
fw_fill_circle:
    sub     sp, sp, #(CIRCLE_ITER_size + 8 + 32)
    stp     x19, x21, [sp, #(CIRCLE_ITER_size + 8)]
    str     x30, [sp, #(CIRCLE_ITER_size + 24)]
    mov     x2, x1
    mov     x1, x0
    mov     x0, sp
    bl      coords_in_circle_init
    mov     x19, #0                     // count
fw_fill_circle.next:
    mov     x0, sp
    bl      coords_in_circle_next
    cbz     w2, fw_fill_circle.done
    LDX     x10, fw_circle_cap
    cmp     x19, x10
    b.lo    fw_fill_circle.store
    // grow: twice the capacity (at least 64), the coordinates so far copied
    mov     x21, x9
    lsl     x0, x10, #1
    mov     x3, #64
    cmp     x0, x3
    csel    x0, x3, x0, lo
    STX     x0, fw_circle_cap
    lsl     x0, x0, #3
    bl      alloc
    LDX     x1, fw_circle
    STX     x9, fw_circle
    mov     x0, x9
    mov     x3, x19
    REP_MOVSQ
    mov     x9, x21
fw_fill_circle.store:
    LDX     x3, fw_circle
    str     x9, [x3, x19, lsl #3]
    add     x19, x19, #1
    b       fw_fill_circle.next
fw_fill_circle.done:
    STX     x19, fw_circle_count
    ldr     x30, [sp, #(CIRCLE_ITER_size + 24)]
    ldp     x19, x21, [sp, #(CIRCLE_ITER_size + 8)]
    add     sp, sp, #(CIRCLE_ITER_size + 8 + 32)
    ret

// fw_prepare_scenes: FireworksIterator.prepare_scenes - per shell a color
// and its bloom gradient; per character the launch, bloom and fall scenes.
fw_prepare_scenes:
    PUSH2   x19, x21
    PUSH2   x22, x23
    PUSH1   x30
    bl      fw_make_final_map
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    cset    w9, eq
    STB     w9, fw_dynamic
    mov     x21, #0                     // shell
fw_prepare_scenes.shell:
    LDX     x10, fw_shells
    cmp     x21, x10
    b.hs    fw_prepare_scenes.done
    LDX     x19, effect_config
    ldr     x0, [x19, #FIREWORKS.color_count]
    bl      rng_below
    STX     x9, fw_color_index
    ldr     x3, [x19, #FIREWORKS.colors]
    ldr     x9, [x3, x9, lsl #3]
    STX     x9, fw_color
    // the launch visuals: the firework symbol in the shell color, and white
    mov     x0, x9
    mov     x1, #NONE
    ldr     x2, [x19, #FIREWORKS.symbol]
    mov     w3, #0
    bl      visual_make
    STW     w9, fw_launch_color
    mov     x0, #FW_WHITE
    mov     x1, #NONE
    ldr     x2, [x19, #FIREWORKS.symbol]
    mov     w3, #0
    bl      visual_make
    STW     w9, fw_launch_white
    LDX     x9, fw_color
    // Gradient::with_steps([color, white, color], 5)
    ADRG    x0, fw_stops
    str     x9, [x0]
    mov     x10, #FW_WHITE
    str     x10, [x0, #8]
    str     x9, [x0, #16]
    mov     w1, #3
    ADRG    x2, fw_five_steps
    mov     w3, #1
    ADRG    x4, fw_shell_spectrum
    bl      gradient_new
    mov     w9, w9
    STX     x9, fw_shell_len
    // the shell's characters
    cbz     x21, fw_prepare_scenes.next_shell
    sub     x22, x21, #1
    LDX     x10, fw_volume
    mul     x22, x22, x10               // first position
    add     x23, x22, x10
    LDX     x10, fw_count
    cmp     x23, x10
    csel    x23, x10, x23, hi           // end
fw_prepare_scenes.char:
    cmp     x22, x23
    b.hs    fw_prepare_scenes.next_shell
    LDX     x9, fw_chars
    ldr     w0, [x9, x22, lsl #2]
    bl      fw_char_scenes
    add     x22, x22, #1
    b       fw_prepare_scenes.char
fw_prepare_scenes.next_shell:
    add     x21, x21, #1
    b       fw_prepare_scenes.shell
fw_prepare_scenes.done:
    POP1    x30
    POP2    x22, x23
    POP2    x19, x21
    ret

// fw_char_scenes(w0=slot): one character of prepare_scenes' shell loop.
fw_char_scenes:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w19, w0
    LDX     x9, ch_sym
    ldr     x9, [x9, x19, lsl #3]
    STX     x9, fw_sym
    // launch: the firework symbol in the shell color, then white; looping
    mov     w1, #AUTO
    mov     w2, #SCF_LOOPING
    mov     w3, #NONE
    bl      scene_new
    mov     w21, w9
    mov     w0, w9
    LDW     w1, fw_launch_color
    mov     w2, #2
    bl      scene_add_frame_visual
    mov     w0, w21
    LDW     w1, fw_launch_white
    mov     w2, #1
    bl      scene_add_frame_visual
    // bloom: the shell gradient over the input symbol, synced to steps
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #SCF_SYNC_STEP
    mov     w3, #NONE
    bl      scene_new
    mov     w22, w9
    LDX     x0, fw_sym
    ADRG    x1, fw_shell_spectrum
    LDX     x2, fw_shell_len
    mov     x3, #NONE
    LDX     x4, fw_color                // the shell spectrum's color, tagged
    orr     x4, x4, #(1 << 47)          // apart from the fall's keys
    bl      visual_run
    mov     w0, w22
    mov     x1, x9
    mov     w3, #2
    bl      visual_frames
fw_char_scenes.fall:
    mov     w0, w19
    MOV64   w1, FW_SCN_FALL
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w23, w9
    LDB     w9, fw_dynamic
    cbnz    w9, fw_char_scenes.dynamic
    // shell color -> final gradient color, 15 steps, 10 ticks each
    LDX     x9, fw_color
    STX     x9, fw_stops
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x19, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, fw_final_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x19, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, fw_final_map
    ldr     x9, [x3, x9, lsl #3]
    ADRG    x3, fw_stops
    str     x9, [x3, #8]
    // apply_gradient_to_symbols([symbol], 10, the pair's spectrum): a frame
    // per color, the visuals shared by symbol and pair (colors fit in 41
    // bits, so the shell color's index above them keys the pair exactly)
    LDX     x20, fw_color_index
    lsl     x20, x20, #48
    orr     x20, x20, x9
    LDX     x0, fw_sym
    mov     x1, x20
    mov     x2, #NONE
    bl      visual_run_find
    cbnz    x9, fw_char_scenes.fall_frames
    ADRG    x0, fw_stops
    ADRG    x4, fw_fg_spectrum
    bl      fw_pair_gradient
    LDX     x0, fw_sym
    ADRG    x1, fw_fg_spectrum
    mov     w2, w9
    mov     x3, #NONE
    mov     x4, x20
    bl      visual_run
fw_char_scenes.fall_frames:
    mov     w0, w23
    mov     x1, x9
    mov     w3, #10
    bl      visual_frames
    b       fw_char_scenes.activate
fw_char_scenes.dynamic:
    // gradients toward whichever input colors exist, else colorless
    mov     w24, #0                     // fg count
    mov     w20, #0                     // bg count
    LDX     x9, ch_fg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    fw_char_scenes.dyn_bg
    ADRG    x0, fw_stops
    LDX     x3, fw_color
    str     x3, [x0]
    str     x9, [x0, #8]
    ADRG    x4, fw_fg_spectrum
    bl      fw_pair_gradient
    mov     w24, w9
fw_char_scenes.dyn_bg:
    LDX     x9, ch_bg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    fw_char_scenes.dyn_frames
    ADRG    x0, fw_stops
    LDX     x3, fw_color
    str     x3, [x0]
    str     x9, [x0, #8]
    ADRG    x4, fw_bg_spectrum
    bl      fw_pair_gradient
    mov     w20, w9
fw_char_scenes.dyn_frames:
    orr     w9, w24, w20
    cbz     w9, fw_char_scenes.colorless
    mov     x7, x20
    mov     x6, #0
    cbz     w20, fw_char_scenes.dyn_no_bg
    ADRG    x6, fw_bg_spectrum
fw_char_scenes.dyn_no_bg:
    mov     x4, #0
    cbz     w24, fw_char_scenes.dyn_apply
    ADRG    x4, fw_fg_spectrum
fw_char_scenes.dyn_apply:
    mov     w0, w23
    ADRG    x1, fw_sym
    mov     x2, #1
    mov     w3, #10
    mov     w5, w24
    bl      scene_apply_gradient
    b       fw_char_scenes.activate
fw_char_scenes.colorless:
    mov     w0, w23
    LDX     x1, fw_sym
    mov     w2, #10
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
fw_char_scenes.activate:
    mov     w0, w19
    mov     w1, w21
    bl      scene_activate
    // apex complete -> bloom; input activated -> fall
    SCENE_PTR x9, x22
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    MOV64   w3, FW_PATH_APEX
    mov     w4, #ACT_ACTIVATE_SCENE
    MOV64   x5, SC_NAME
    ldr     w5, [x9, x5]
    mov     x6, #0
    bl      event_register
    mov     w0, w19
    mov     w1, #EV_PATH_ACTIVATED
    mov     w2, #CALLER_PATH
    MOV64   w3, FW_PATH_INPUT
    mov     w4, #ACT_ACTIVATE_SCENE
    MOV64   w5, FW_SCN_FALL
    mov     x6, #0
    bl      event_register
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// fw_pair_gradient(x0=two stops, x4=out) -> w9 = length:
// Gradient::with_steps(stops, 15).
fw_pair_gradient:
    mov     w1, #2
    ADRG    x2, fw_fifteen_steps
    mov     w3, #1
    b       gradient_new

// fw_make_final_map: Gradient::new(final stops, final steps) and its coordinate
// mapping over the text rectangle.
fw_make_final_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #FIREWORKS.final_steps]
    ldr     x3, [x19, #FIREWORKS.final_step_count]
    ldr     x1, [x19, #FIREWORKS.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, fw_final_spectrum
    ldr     x0, [x19, #FIREWORKS.final_stops]
    ldr     x1, [x19, #FIREWORKS.final_stop_count]
    ldr     x2, [x19, #FIREWORKS.final_steps]
    ldr     x3, [x19, #FIREWORKS.final_step_count]
    LDX     x4, fw_final_spectrum
    bl      gradient_new
    LDX     x0, fw_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, fw_final_map_width
    ldr     x6, [x19, #FIREWORKS.final_direction]
    bl      gradient_map
    STX     x9, fw_final_map
    POP2    x19, x30
    ret

// fireworks_next_frame -> w9 = 1 when a frame should be rendered, 0 when
// done. Fireworks::next_frame: launch the last remaining shell once the
// delay runs out, then tick.
fireworks_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x9, fw_shells
    cbnz    x9, fireworks_next_frame.shells
    bl      active_empty
    cbnz    w9, fireworks_next_frame.finished
    b       fireworks_next_frame.tick
fireworks_next_frame.shells:
    LDX     x10, fw_delay
    cmp     x10, #0
    b.gt    fireworks_next_frame.tick
    sub     x9, x9, #1
    STX     x9, fw_shells
    cbz     x9, fireworks_next_frame.delay  // the empty shell
    sub     x21, x9, #1
    LDX     x10, fw_volume
    mul     x21, x21, x10
    add     x22, x21, x10
    LDX     x10, fw_count
    cmp     x22, x10
    csel    x22, x10, x22, hi
fireworks_next_frame.launch:
    cmp     x21, x22
    b.hs    fireworks_next_frame.delay
    LDX     x9, fw_chars
    ldr     w19, [x9, x21, lsl #2]
    mov     w0, w19
    bl      set_visible
    mov     w0, w19
    bl      active_insert
    add     x21, x21, #1
    b       fireworks_next_frame.launch
fireworks_next_frame.delay:
    // int(launch_delay * uniform(0.5, 1.5))
    fmov    d0, #0.5
    fmov    d1, #1.5
    bl      rng_uniform
    LDX     x9, effect_config
    ldr     x9, [x9, #FIREWORKS.launch_delay]
    scvtf   d1, x9
    fmul    d0, d0, d1
    F64_TO_I64
    STX     x9, fw_delay
fireworks_next_frame.tick:
    LDX     x9, fw_delay
    sub     x9, x9, #1
    STX     x9, fw_delay
    bl      update
    mov     w9, #1
    b       fireworks_next_frame.out
fireworks_next_frame.finished:
    mov     w9, #0
fireworks_next_frame.out:
    POP2    x22, x30
    POP2    x19, x21
    ret

    .section .rodata
    .balign 8
fw_five_steps:          .quad 5
fw_fifteen_steps:       .quad 15
fw_speed_apex:          .double 0.35
fw_speed_explode_lo:    .double 0.2
fw_speed_explode_hi:    .double 0.4
fw_speed_input:         .double 0.6
// dropped: fw_half fw_one_half (fmov immediates)

    TSTATE
    .balign 8
fw_volume:              .skip 8
fw_distance:            .skip 8
fw_delay:               .skip 8
fw_chars:               .skip 8         // input characters, top to bottom
fw_count:               .skip 8
fw_shells:              .skip 8         // shells not launched yet
fw_origin:              .skip 8
fw_circle:              .skip 8         // explode waypoint candidates
fw_circle_count:        .skip 8
fw_circle_cap:          .skip 8
fw_color:               .skip 8         // the shell color
fw_color_index:         .skip 8         // its index in the colors
fw_launch_color:        .skip 4         // the launch scene's visuals
fw_launch_white:        .skip 4
fw_sym:                 .skip 8
fw_shell_len:           .skip 8
fw_final_spectrum:      .skip 8
fw_final_map:           .skip 8
fw_final_map_width:     .skip 8
fw_stops:               .skip 8 * 3
fw_shell_spectrum:      .skip 8 * 32
fw_fg_spectrum:         .skip 8 * 32
fw_bg_spectrum:         .skip 8 * 32
fw_dynamic:             .skip 1

    .text
