// effects/orbittingvolley.s - "Four launchers orbit the canvas, firing
// volleys of characters towards their input coordinates"
// (src/effects/orbittingvolley.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Orbittingvolley).
//
// The effect draws no random numbers. The main (top) launcher rides the
// "perimeter" path along the top row; the other three are placed from its
// progress each frame. Launcher.magazine is a cursor into the flattened
// center-to-outside order: launcher i owns positions i, i + 4, i + 8, ...
// and remove(0) advances its cursor by 4.

.equ HAVE_orbittingvolley, 1

.equ ORBITTINGVOLLEY.top_symbol,        0
.equ ORBITTINGVOLLEY.right_symbol,      8
.equ ORBITTINGVOLLEY.bottom_symbol,     16
.equ ORBITTINGVOLLEY.left_symbol,       24
.equ ORBITTINGVOLLEY.launcher_speed,    32      // f64
.equ ORBITTINGVOLLEY.character_speed,   40      // f64
.equ ORBITTINGVOLLEY.volley_size,       48      // f64
.equ ORBITTINGVOLLEY.launch_delay,      56
.equ ORBITTINGVOLLEY.character_easing,  64
.equ ORBITTINGVOLLEY.final_stops,       72      // *const u64
.equ ORBITTINGVOLLEY.final_stop_count,  80
.equ ORBITTINGVOLLEY.final_steps,       88      // *const i64
.equ ORBITTINGVOLLEY.final_step_count,  96
.equ ORBITTINGVOLLEY.final_direction,   104
.equ ORBITTINGVOLLEY_size,              112

// path names: "input_path", "perimeter"
.equ OV_INPUT,          NAME_LITERAL + 0
.equ OV_PERIMETER,      NAME_LITERAL + 1

    .text

// orbittingvolley_build: OrbittingVolley::build.
orbittingvolley_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    bl      ov_color_maps
    LDX     x22, effect_config
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x23, x9
    mov     x24, x2
    mov     x19, #0
orbittingvolley_build.char:
    cmp     x19, x24
    b.hs    orbittingvolley_build.launchers
    ldr     w21, [x23, x19, lsl #2]
    // the "input_path" home, layer 1
    ldr     d0, [x22, #ORBITTINGVOLLEY.character_speed]
    mov     w0, w21
    ldr     w1, [x22, #ORBITTINGVOLLEY.character_easing]
    mov     x2, #1
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, OV_INPUT
    bl      path_new
    mov     w20, w9
    mov     w0, w21
    bl      char_input_coord
    mov     w0, w20
    mov     x1, x9
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // PathComplete("input_path") -> SetLayer(0)
    mov     w0, w21
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    MOV64   w3, OV_INPUT
    mov     w4, #ACT_SET_LAYER
    mov     x5, #0
    mov     x6, #0
    bl      event_register
    // the final colors: the input colors under dynamic handling, else the
    // final gradient at the input coordinate
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    orbittingvolley_build.final
    LDX     x9, ch_fg
    ldr     x2, [x9, x21, lsl #3]
    LDX     x9, ch_bg
    ldr     x3, [x9, x21, lsl #3]
    b       orbittingvolley_build.appearance
orbittingvolley_build.final:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x21, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, ov_final_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x21, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, ov_final_map
    ldr     x2, [x3, x9, lsl #3]
    mov     x3, #NONE
orbittingvolley_build.appearance:
    mov     w0, w21
    mov     x1, #0
    bl      set_appearance
    add     x19, x19, #1
    b       orbittingvolley_build.char
orbittingvolley_build.launchers:
    // top-left, top-right, bottom-right, bottom-left, each on layer 2
    mov     w19, #0
orbittingvolley_build.launcher:
    cmp     w19, #4
    b.hs    orbittingvolley_build.main
    add     x9, x22, x19, lsl #3
    ldr     x0, [x9, #ORBITTINGVOLLEY.top_symbol]
    bl      ov_corner
    mov     x1, x9
    bl      add_character
    mov     w21, w9
    ADRG    x3, ov_launchers
    str     w9, [x3, x19, lsl #2]
    mov     w0, w21
    mov     x1, #2
    bl      set_layer
    mov     w0, w21
    bl      set_visible
    mov     w0, w21
    bl      active_insert
    add     w19, w19, #1
    b       orbittingvolley_build.launcher
orbittingvolley_build.main:
    LDW     w21, ov_launchers
    mov     w0, w21
    mov     x1, #0
    LDX     x2, ov_last_color
    mov     x3, #NONE
    bl      set_appearance
    // Launcher.build_paths: the main launcher starts at waypoints[0], so the
    // rotation is the identity
    ldr     d0, [x22, #ORBITTINGVOLLEY.launcher_speed]
    mov     w0, w21
    mov     w1, #NONE
    mov     x2, #2
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, OV_PERIMETER
    bl      path_new
    STW     w9, ov_perimeter
    mov     w0, w9
    LDX     x1, canvas_top
    lsl     x1, x1, #32
    orr     x1, x1, #1
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    LDW     w0, ov_perimeter
    LDX     x1, canvas_top
    lsl     x1, x1, #32
    LDW     w9, canvas_right
    orr     x1, x1, x9
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w21
    LDW     w1, ov_perimeter
    bl      path_activate
    // the magazines: center to outside, dealt round robin
    mov     w0, #FILTER_INPUT
    mov     w1, #GROUP_CENTER_TO_OUTSIDE
    bl      get_characters_grouped
    mov     x23, x9
    mov     x24, x2
    mov     x3, #0
    mov     x0, #0
orbittingvolley_build.sum:
    cmp     x3, x24
    b.hs    orbittingvolley_build.flatten
    add     x9, x23, x3, lsl #4
    ldr     x9, [x9, #8]
    add     x0, x0, x9
    add     x3, x3, #1
    b       orbittingvolley_build.sum
orbittingvolley_build.flatten:
    STX     x0, ov_sorted_count
    lsl     x0, x0, #2
    add     x0, x0, #4
    bl      alloc
    STX     x9, ov_sorted
    mov     x0, x9
    mov     x3, #0
orbittingvolley_build.group:
    cmp     x3, x24
    b.hs    orbittingvolley_build.dealt
    add     x9, x23, x3, lsl #4
    ldr     x1, [x9]
    ldr     x2, [x9, #8]
    mov     x4, #0
orbittingvolley_build.member:
    cmp     x4, x2
    b.hs    orbittingvolley_build.next_group
    ldr     w9, [x1, x4, lsl #2]
    str     w9, [x0], #4
    add     x4, x4, #1
    b       orbittingvolley_build.member
orbittingvolley_build.next_group:
    add     x3, x3, #1
    b       orbittingvolley_build.group
orbittingvolley_build.dealt:
    ADRG    x9, ov_cursor
    mov     x10, #1
    stp     xzr, x10, [x9]
    mov     x10, #2
    mov     x11, #3
    stp     x10, x11, [x9, #16]
    // characters per volley: max(int(volley_size * len(input) / 4), 1)
    LDX     x9, input_count
    scvtf   d0, x9
    ldr     d1, [x22, #ORBITTINGVOLLEY.volley_size]
    fmul    d1, d1, d0
    fmov    d2, #4.0
    fdiv    d1, d1, d2
    fcvtzs  x9, d1
    mov     x3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    STX     x9, ov_volley
    STX     xzr, ov_delay
    STB     wzr, ov_complete
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// ov_corner(w19=launcher index) -> x9 = its packed corner coordinate:
// (left, top), (right, top), (right, bottom), (left, bottom). Keeps x0.
// Clobbers x3.
ov_corner:
    mov     x9, #1                      // row = bottom
    cmp     w19, #2
    b.hs    ov_corner.row
    LDX     x9, canvas_top
ov_corner.row:
    lsl     x9, x9, #32
    mov     w3, #1                      // column = left
    cmp     w19, #1
    b.eq    ov_corner.right
    cmp     w19, #2
    b.ne    ov_corner.column
ov_corner.right:
    LDW     w3, canvas_right
ov_corner.column:
    orr     x9, x9, x3
    ret

// ov_color_maps: Gradient::new(final stops, steps), its coordinate maps over
// the text rectangle and over the canvas, and the spectrum's last color.
ov_color_maps:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #ORBITTINGVOLLEY.final_steps]
    ldr     x3, [x19, #ORBITTINGVOLLEY.final_step_count]
    ldr     x1, [x19, #ORBITTINGVOLLEY.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    mov     x21, x9
    ldr     x0, [x19, #ORBITTINGVOLLEY.final_stops]
    ldr     x1, [x19, #ORBITTINGVOLLEY.final_stop_count]
    ldr     x2, [x19, #ORBITTINGVOLLEY.final_steps]
    ldr     x3, [x19, #ORBITTINGVOLLEY.final_step_count]
    mov     x4, x21
    bl      gradient_new
    mov     w22, w9
    add     x9, x21, x22, lsl #3
    ldur    x9, [x9, #-8]
    STX     x9, ov_last_color
    // over the text
    mov     x0, x21
    mov     w1, w22
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, ov_final_width
    ldr     x6, [x19, #ORBITTINGVOLLEY.final_direction]
    bl      gradient_map
    STX     x9, ov_final_map
    // over the canvas (for the launchers)
    mov     x0, x21
    mov     w1, w22
    mov     x2, #1
    LDX     x3, canvas_top
    mov     x4, #1
    LDX     x5, canvas_right
    ldr     x6, [x19, #ORBITTINGVOLLEY.final_direction]
    bl      gradient_map
    STX     x9, ov_launcher_map
    POP2    x22, x30
    POP2    x19, x21
    ret

// ov_launcher_color(w0=slot) -> x9 = launcher_gradient_coordinate_map at
// the character's current coordinate. Clobbers x3, x10.
ov_launcher_color:
    LDX     x9, ch_row
    ldrsw   x9, [x9, w0, uxtw #2]
    sub     x9, x9, #1
    LDX     x10, canvas_right
    mul     x9, x9, x10
    LDX     x3, ch_col
    ldrsw   x3, [x3, w0, uxtw #2]
    add     x9, x9, x3
    sub     x9, x9, #1
    LDX     x3, ov_launcher_map
    ldr     x9, [x3, x9, lsl #3]
    ret

// ov_set_child(w19=child launcher index):
// OrbittingVolleyIterator._set_launcher_coordinates(parent 0, child).
ov_set_child:
    PUSH2   x21, x22
    PUSH2   x23, x30
    ADRG    x9, ov_launchers
    ldr     w21, [x9, x19, lsl #2]      // child
    mov     w0, w21
    bl      char_input_coord
    // parent_progress = main column / canvas right
    LDW     w2, ov_launchers
    LDX     x3, ch_col
    ldrsw   x2, [x3, x2, lsl #2]
    scvtf   d2, x2
    LDX     x3, canvas_right
    scvtf   d0, x3
    fdiv    d2, d2, d0
    LDX     x22, canvas_top
    LDX     x23, canvas_right
    // (right, top)
    lsl     x3, x22, #32
    orr     x3, x3, x23
    cmp     x9, x3
    b.ne    ov_set_child.bottom_right
    scvtf   d0, x22
    fmul    d0, d0, d2
    fcvtzs  x3, d0
    sub     x1, x22, x3                 // top - int(top * progress)
    mov     x3, #1
    cmp     x1, #1
    csel    x1, x3, x1, lt
    lsl     x1, x1, #32
    orr     x1, x1, x23
    b       ov_set_child.move
ov_set_child.bottom_right:
    mov     x3, #(1 << 32)
    orr     x3, x3, x23
    cmp     x9, x3
    b.ne    ov_set_child.bottom_left
    scvtf   d0, x23
    fmul    d0, d0, d2
    fcvtzs  x3, d0
    sub     x1, x23, x3                 // right - int(right * progress)
    mov     x3, #1
    cmp     x1, #1
    csel    x1, x3, x1, lt
    mov     w1, w1
    mov     x3, #(1 << 32)
    orr     x1, x1, x3
    b       ov_set_child.move
ov_set_child.bottom_left:
    MOV64   x3, ((1 << 32) | 1)
    cmp     x9, x3
    b.ne    ov_set_child.color
    scvtf   d0, x22
    fmul    d0, d0, d2
    fcvtzs  x1, d0
    add     x1, x1, #1                  // bottom + int(top * progress)
    cmp     x1, x22
    csel    x1, x22, x1, gt
    lsl     x1, x1, #32
    orr     x1, x1, #1
ov_set_child.move:
    mov     w0, w21
    bl      set_coordinate
ov_set_child.color:
    mov     w0, w21
    bl      ov_launcher_color
    mov     x2, x9
    mov     w0, w21
    mov     x1, #0
    mov     x3, #NONE
    bl      set_appearance
    POP2    x23, x30
    POP2    x21, x22
    ret

// ov_launch(w19=launcher index): Launcher.launch, then the character joins
// the active set.
ov_launch:
    PUSH2   x21, x30
    ADRG    x3, ov_cursor
    ldr     x9, [x3, x19, lsl #3]
    LDX     x10, ov_sorted_count
    cmp     x9, x10
    b.hs    ov_launch.empty
    add     x10, x9, #4
    str     x10, [x3, x19, lsl #3]
    LDX     x3, ov_sorted
    ldr     w21, [x3, x9, lsl #2]
    ADRG    x9, ov_launchers
    ldr     w0, [x9, x19, lsl #2]
    bl      char_coord
    mov     w0, w21
    mov     x1, x9
    bl      set_coordinate
    mov     w0, w21
    MOV64   w1, OV_INPUT
    bl      path_activate_name
    mov     w0, w21
    bl      set_visible
    mov     w0, w21
    bl      active_insert
ov_launch.empty:
    POP2    x21, x30
    ret

// orbittingvolley_next_frame -> w9 = 1 for a frame, 0 when done.
orbittingvolley_next_frame:
    PUSH2   x19, x21
    PUSH2   x22, x30
    // any magazine left, or anything besides one launcher still active
    ADRG    x3, ov_cursor
    LDX     x2, ov_sorted_count
    mov     w9, #0
orbittingvolley_next_frame.magazines:
    ldr     x10, [x3, x9, lsl #3]
    cmp     x10, x2
    b.lo    orbittingvolley_next_frame.running
    add     w9, w9, #1
    cmp     w9, #4
    b.lo    orbittingvolley_next_frame.magazines
    bl      active_count
    cmp     x9, #1
    b.hi    orbittingvolley_next_frame.running
    LDB     w9, ov_complete
    cbnz    w9, orbittingvolley_next_frame.done
    mov     w9, #1
    STB     w9, ov_complete
    mov     w19, #0
orbittingvolley_next_frame.hide:
    ADRG    x9, ov_launchers
    ldr     w0, [x9, x19, lsl #2]
    mov     w1, #0
    bl      set_visibility
    add     w19, w19, #1
    cmp     w19, #4
    b.lo    orbittingvolley_next_frame.hide
    b       orbittingvolley_next_frame.frame
orbittingvolley_next_frame.running:
    LDW     w21, ov_launchers
    LDX     x9, ch_path
    ldr     w9, [x9, x21, lsl #2]
    cmn     w9, #1                      // NONE
    b.ne    orbittingvolley_next_frame.main_color
    // the perimeter run ended: back to its first waypoint and go again
    LDX     x1, canvas_top
    lsl     x1, x1, #32
    orr     x1, x1, #1
    mov     w0, w21
    bl      set_coordinate
    mov     w0, w21
    LDW     w1, ov_perimeter
    bl      path_activate
    mov     w0, w21
    bl      active_insert
orbittingvolley_next_frame.main_color:
    mov     w0, w21
    bl      ov_launcher_color
    mov     x2, x9
    mov     w0, w21
    mov     x1, #0                      // top_launcher_symbol is its symbol
    mov     x3, #NONE
    bl      set_appearance
    mov     w19, #1
orbittingvolley_next_frame.children:
    bl      ov_set_child
    add     w19, w19, #1
    cmp     w19, #4
    b.lo    orbittingvolley_next_frame.children
    LDX     x9, ov_delay
    cbnz    x9, orbittingvolley_next_frame.wait
    mov     w19, #0
orbittingvolley_next_frame.volley:
    LDX     x22, ov_volley
orbittingvolley_next_frame.shot:
    ADRG    x3, ov_cursor
    ldr     x9, [x3, x19, lsl #3]
    LDX     x10, ov_sorted_count
    cmp     x9, x10
    b.hs    orbittingvolley_next_frame.next_launcher    // the rest of the volley is empty
    bl      ov_launch
    subs    x22, x22, #1
    b.ne    orbittingvolley_next_frame.shot
orbittingvolley_next_frame.next_launcher:
    add     w19, w19, #1
    cmp     w19, #4
    b.lo    orbittingvolley_next_frame.volley
    LDX     x9, effect_config
    ldr     x9, [x9, #ORBITTINGVOLLEY.launch_delay]
    STX     x9, ov_delay
    b       orbittingvolley_next_frame.update
orbittingvolley_next_frame.wait:
    sub     x9, x9, #1
    STX     x9, ov_delay
orbittingvolley_next_frame.update:
    bl      update
orbittingvolley_next_frame.frame:
    mov     w9, #1
    b       orbittingvolley_next_frame.out
orbittingvolley_next_frame.done:
    mov     w9, #0
orbittingvolley_next_frame.out:
    POP2    x22, x30
    POP2    x19, x21
    ret

// dropped: ov_four (fmov d2, #4.0)

    TSTATE
    .balign 8
ov_final_map:       .skip 8
ov_final_width:     .skip 8
ov_launcher_map:    .skip 8
ov_last_color:      .skip 8
ov_sorted:          .skip 8
ov_sorted_count:    .skip 8
ov_cursor:          .skip 8 * 4
ov_volley:          .skip 8
ov_delay:           .skip 8
ov_launchers:       .skip 4 * 4
ov_perimeter:       .skip 4
ov_complete:        .skip 1

    .text
