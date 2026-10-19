// effects/binarypath.s - src/effects/binarypath.rs.

.equ HAVE_binarypath, 1

.equ BINARYPATH.stops,          0
.equ BINARYPATH.stop_count,     8
.equ BINARYPATH.steps,          16
.equ BINARYPATH.step_count,     24
.equ BINARYPATH.direction,      32
.equ BINARYPATH.colors,         40
.equ BINARYPATH.color_count,    48
.equ BINARYPATH.speed,          56
.equ BINARYPATH.active,         64
.equ BINARYPATH_size,           72

// Bits are contiguous arena slots. Pending and active vectors contain pointers
// to these records; removals preserve Rust's Vec order.
.equ BP_REP.source,             0
.equ BP_REP.first,              4
.equ BP_REP.count,              8
.equ BP_REP.emitted,            12
.equ BP_REP.coord,              16
.equ BP_REP_size,               24
.equ BP_COLLAPSE,               NAME_LITERAL + 0
.equ BP_BRIGHTEN,               NAME_LITERAL + 1

    .text
// BinaryPath::build. Clobbers C.
binarypath_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w0, #FILTER_INPUT
    mov     w1, #GROUP_DIAG_TR_TO_BL
    bl      get_characters_grouped
    STX     x9, bp_groups
    STX     x2, bp_group_count
    bl      bp_color_map
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    STX     x9, bp_chars
    STX     x2, bp_count
    STX     x2, bp_pending_count
    mov     x8, #BP_REP_size
    mul     x0, x2, x8
    bl      alloc
    STX     x9, bp_reps
    LDX     x0, bp_count
    lsl     x0, x0, #3
    bl      alloc
    STX     x9, bp_pending
    LDX     x0, bp_count
    lsl     x0, x0, #3
    bl      alloc
    STX     x9, bp_active
    LDX     x0, canvas_right
    LDX     x9, canvas_top
    add     x0, x0, x9
    add     x0, x0, #5
    lsl     x0, x0, #3
    bl      alloc
    STX     x9, bp_coords
    mov     x19, #0
binarypath_build.add_rep:
    LDX     x9, bp_count
    cmp     x19, x9
    b.hs    binarypath_build.paths
    mov     x8, #BP_REP_size
    mul     x20, x19, x8
    LDX     x9, bp_reps
    add     x20, x20, x9
    LDX     x9, bp_pending
    str     x20, [x9, x19, lsl #3]
    LDX     x9, bp_chars
    ldr     w0, [x9, x19, lsl #2]
    str     w0, [x20, #BP_REP.source]
    bl      char_input_coord
    str     x9, [x20, #BP_REP.coord]
    ldr     w0, [x20, #BP_REP.source]
    LDX     x1, ch_sym
    add     x1, x1, x0, lsl #3
    bl      utf8_decode
    mov     w21, w9
    orr     w9, w9, #0x80               // minimum width 8, including U+0000
    clz     w9, w9
    mov     w22, #31
    sub     w22, w22, w9
    add     w9, w22, #1
    str     w9, [x20, #BP_REP.count]
binarypath_build.bits:
    lsr     w0, w21, w22
    and     w0, w0, #1
    add     w0, w0, #'0'
    orr     x0, x0, #0x100000000
    mov     x1, #0
    bl      add_character
    ldr     w3, [x20, #BP_REP.count]
    sub     w3, w3, #1
    cmp     w3, w22
    b.ne    binarypath_build.next_bit
    str     w9, [x20, #BP_REP.first]
binarypath_build.next_bit:
    subs    w22, w22, #1
    b.pl    binarypath_build.bits
    add     x19, x19, #1
    b       binarypath_build.add_rep
binarypath_build.paths:
    mov     x19, #0
binarypath_build.rep_path:
    LDX     x9, bp_count
    cmp     x19, x9
    b.hs    binarypath_build.scenes
    mov     x8, #BP_REP_size
    mul     x20, x19, x8
    LDX     x9, bp_reps
    add     x20, x20, x9
    bl      bp_make_coords
    mov     w21, #0
binarypath_build.bit_path:
    ldr     w9, [x20, #BP_REP.count]
    cmp     w21, w9
    b.hs    binarypath_build.next_rep
    ldr     w22, [x20, #BP_REP.first]
    add     w22, w22, w21
    mov     w0, w22
    LDX     x9, bp_coords
    ldr     x1, [x9]
    bl      set_coordinate
    mov     w0, w22
    LDX     x9, effect_config
    ldr     d0, [x9, #BINARYPATH.speed]
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w23, w9
    mov     x24, #0
binarypath_build.waypoint:
    LDX     x9, bp_coords
    ldr     x1, [x9, x24, lsl #3]
    mov     w0, w23
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    add     x24, x24, #1
    LDX     x9, bp_coord_count
    cmp     x24, x9
    b.lo    binarypath_build.waypoint
    mov     w0, w22
    mov     w1, w23
    bl      path_activate
    mov     w0, w22
    mov     x1, #1
    bl      set_layer
    LDX     x9, effect_config
    ldr     x0, [x9, #BINARYPATH.color_count]
    bl      rng_below
    LDX     x3, effect_config
    ldr     x3, [x3, #BINARYPATH.colors]
    ldr     x24, [x3, x9, lsl #3]
    mov     w0, w22
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w23, w9
    mov     w0, w9
    LDX     x1, ch_sym
    ldr     x1, [x1, x22, lsl #3]
    mov     w2, #1
    mov     x3, x24
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w22
    mov     w1, w23
    bl      scene_activate
    add     w21, w21, #1
    b       binarypath_build.bit_path
binarypath_build.next_rep:
    add     x19, x19, #1
    b       binarypath_build.rep_path
binarypath_build.scenes:
    mov     x19, #0
binarypath_build.scene:
    LDX     x9, bp_count
    cmp     x19, x9
    b.hs    binarypath_build.done
    LDX     x9, bp_chars
    ldr     w0, [x9, x19, lsl #2]
    bl      bp_make_scenes
    add     x19, x19, #1
    b       binarypath_build.scene
binarypath_build.done:
    LDX     x9, bp_count
    scvtf   d0, x9
    LDX     x9, effect_config
    ldr     d1, [x9, #BINARYPATH.active]
    fmul    d0, d0, d1
    fcvtzs  x9, d0
    mov     x3, #1
    cmp     x9, x3
    csel    x9, x3, x9, lt
    STX     x9, bp_max_active
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// The alternating row/column walk in BinaryPath::build, including the two
// duplicate terminal waypoints. x20 = representation; preserves callee saves.
bp_make_coords:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    mov     w0, #1
    mov     w1, #0
    bl      canvas_random_coord
    mov     x21, x9
    LDX     x9, bp_coords
    str     x21, [x9]
    mov     x22, #1
    mov     x0, #2
    bl      rng_below
    mov     w23, w9
    LDX     x9, canvas_right
    scvtf   d0, x9
    LDD     d1, bp_fifth
    fmul    d0, d0, d1
    fcvtzs  x9, d0
    mov     x3, #10
    cmp     x9, x3
    csel    x9, x3, x9, lt
    mov     x24, x9
bp_make_coords.walk:
    ldr     x9, [x20, #BP_REP.coord]
    cmp     x21, x9
    b.eq    bp_make_coords.finish
    cbnz    w23, bp_make_coords.column
    asr     x9, x21, #32
    ldrsw   x1, [x20, #BP_REP.coord + 4]
    subs    x1, x1, x9
    b.eq    bp_make_coords.direct
    mov     x19, #1
    b.pl    bp_make_coords.row_positive
    neg     x1, x1
    neg     x19, x19
bp_make_coords.row_positive:
    cmp     x1, x24
    csel    x1, x24, x1, gt
    mov     x0, #1
    bl      rng_randint
    mul     x9, x9, x19
    add     x21, x21, x9, lsl #32
    mov     w23, #1
    b       bp_make_coords.append
bp_make_coords.column:
    sxtw    x9, w21
    ldrsw   x1, [x20, #BP_REP.coord]
    subs    x1, x1, x9
    b.eq    bp_make_coords.direct
    mov     x19, #1
    b.pl    bp_make_coords.col_positive
    neg     x1, x1
    neg     x19, x19
bp_make_coords.col_positive:
    mov     x9, #4
    cmp     x1, x9
    csel    x1, x9, x1, gt
    mov     x0, #1
    bl      rng_randint
    mul     x9, x9, x19
    add     w3, w21, w9
    bfi     x21, x3, #0, #32
    mov     w23, #0
    b       bp_make_coords.append
bp_make_coords.direct:
    ldr     x21, [x20, #BP_REP.coord]
bp_make_coords.append:
    LDX     x9, bp_coords
    str     x21, [x9, x22, lsl #3]
    add     x22, x22, #1
    b       bp_make_coords.walk
bp_make_coords.finish:
    LDX     x9, bp_coords
    add     x9, x9, x22, lsl #3
    stp     x21, x21, [x9]
    add     x22, x22, #2
    STX     x22, bp_coord_count
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// Gradient::new + build_coordinate_color_mapping.
bp_color_map:
    stp     x19, x20, [sp, #-32]!
    str     x30, [sp, #16]
    LDX     x19, effect_config
    ldr     x0, [x19, #BINARYPATH.steps]
    ldr     x3, [x19, #BINARYPATH.step_count]
    ldr     x1, [x19, #BINARYPATH.stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    mov     x4, x9
    mov     x20, x9
    ldr     x0, [x19, #BINARYPATH.stops]
    ldr     x1, [x19, #BINARYPATH.stop_count]
    ldr     x2, [x19, #BINARYPATH.steps]
    ldr     x3, [x19, #BINARYPATH.step_count]
    bl      gradient_new
    mov     x0, x20
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, bp_map_width
    ldr     x6, [x19, #BINARYPATH.direction]
    bl      gradient_map
    STX     x9, bp_map
    ldr     x30, [sp, #16]
    ldp     x19, x20, [sp], #32
    ret

// Build collapse/brighten scenes, including Dynamic foreground/background.
// w0 = source. Clobbers C.
bp_make_scenes:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w21, w0
    LDX     x9, ch_sym
    ldr     x9, [x9, x21, lsl #3]
    STX     x9, bp_symbol
    mov     x22, #NONE
    mov     x23, #NONE
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    bp_make_scenes.mapped
    LDX     x9, ch_fg
    ldr     x22, [x9, x21, lsl #3]
    LDX     x9, ch_bg
    ldr     x23, [x9, x21, lsl #3]
    b       bp_make_scenes.colors
bp_make_scenes.mapped:
    mov     w0, w21
    bl      char_input_coord
    asr     x10, x9, #32
    LDX     x11, text_bottom
    sub     x10, x10, x11
    LDX     x11, bp_map_width
    mul     x10, x10, x11
    add     x10, x10, w9, uxtw
    LDX     x11, text_left
    sub     x10, x10, x11
    LDX     x11, bp_map
    ldr     x22, [x11, x10, lsl #3]
bp_make_scenes.colors:
    STX     x22, bp_final_fg
    STX     x23, bp_final_bg
    mov     x0, x22
    bl      bp_dim
    STX     x9, bp_dim_fg
    mov     x0, x23
    bl      bp_dim
    STX     x9, bp_dim_bg
    mov     w19, #0
bp_make_scenes.scene:
    mov     w0, w21
    MOV64   w1, BP_COLLAPSE
    add     w1, w1, w19
    mov     w2, #0
    mov     w3, #4                      // InQuad
    cbz     w19, bp_make_scenes.new
    mov     w3, #NONE
bp_make_scenes.new:
    bl      scene_new
    mov     w20, w9
    mov     w22, #7
    mov     w23, #3
    mov     x0, #0xffffff
    LDX     x1, bp_dim_fg
    ADRG    x4, bp_fg_spectrum
    cbz     w19, bp_make_scenes.fg
    mov     w22, #10
    mov     w23, #2
    LDX     x0, bp_dim_fg
    LDX     x1, bp_final_fg
bp_make_scenes.fg:
    bl      bp_pair_gradient
    mov     w24, w9
    mov     x0, #0xffffff
    LDX     x1, bp_dim_bg
    ADRG    x4, bp_bg_spectrum
    cbz     w19, bp_make_scenes.bg
    LDX     x0, bp_dim_bg
    LDX     x1, bp_final_bg
bp_make_scenes.bg:
    bl      bp_pair_gradient
    mov     w5, w24
    ADRG    x4, bp_fg_spectrum
    mov     w2, #1
    ADRG    x1, bp_symbol
    mov     w0, w20
    mov     w3, w23
    cbnz    w9, bp_make_scenes.gradient
    cbnz    w24, bp_make_scenes.gradient
    LDX     x1, bp_symbol
    mov     w2, w23
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       bp_make_scenes.next
bp_make_scenes.gradient:
    mov     x7, x9
    ADRG    x6, bp_bg_spectrum
    bl      scene_apply_gradient
bp_make_scenes.next:
    add     w19, w19, #1
    cmp     w19, #2
    b.lo    bp_make_scenes.scene
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

bp_dim:
    mov     x9, x0
    cmn     x0, #1                      // NONE
    b.eq    bp_dim.done
    LDD     d0, bp_half
    b       adjust_color_brightness
bp_dim.done:
    ret

// x0/start, x1/end, x22/steps, x4/output. Absent end -> empty gradient.
bp_pair_gradient:
    mov     w9, #0
    cmn     x1, #1                      // NONE
    b.eq    bp_pair_gradient.done
    ADRG    x2, bp_pair
    stp     x0, x1, [x2]
    STX     x22, bp_steps
    mov     x0, x2
    mov     w1, #2
    ADRG    x2, bp_steps
    mov     w3, #1
    b       gradient_new
bp_pair_gradient.done:
    ret

// BinaryPath::next_frame. Clobbers C.
binarypath_next_frame:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    stp     x23, x30, [sp, #32]
    LDB     w9, bp_complete
    cbz     w9, binarypath_next_frame.run
    bl      active_empty
    cbz     w9, binarypath_next_frame.run
    mov     w9, #0
    LDB     w10, bp_last
    cbnz    w10, binarypath_next_frame.out
    mov     w10, #1
    STB     w10, bp_last
    b       binarypath_next_frame.frame
binarypath_next_frame.run:
    LDB     w9, bp_wipe
    cbnz    w9, binarypath_next_frame.wipe
binarypath_next_frame.select:
    LDX     x9, bp_active_count
    LDX     x10, bp_max_active
    cmp     x9, x10
    b.hs    binarypath_next_frame.travel
    LDX     x0, bp_pending_count
    cbz     x0, binarypath_next_frame.travel
    bl      rng_below
    LDX     x1, bp_pending
    ldr     x2, [x1, x9, lsl #3]
    LDX     x3, bp_active_count
    LDX     x0, bp_active
    str     x2, [x0, x3, lsl #3]
    add     x3, x3, #1
    STX     x3, bp_active_count
    LDX     x3, bp_pending_count
    sub     x3, x3, #1
    STX     x3, bp_pending_count
    sub     x3, x3, x9
    add     x0, x1, x9, lsl #3
    add     x1, x0, #8
    REP_MOVSQ
    b       binarypath_next_frame.select
binarypath_next_frame.travel:
    mov     x19, #0
    mov     x21, #0                     // retained count
binarypath_next_frame.rep:
    LDX     x9, bp_active_count
    cmp     x19, x9
    b.hs    binarypath_next_frame.traveled
    LDX     x9, bp_active
    ldr     x20, [x9, x19, lsl #3]
    ldr     w9, [x20, #BP_REP.emitted]
    ldr     w10, [x20, #BP_REP.count]
    cmp     w9, w10
    b.hs    binarypath_next_frame.check
    ldr     w10, [x20, #BP_REP.first]
    add     w22, w9, w10
    add     w9, w9, #1
    str     w9, [x20, #BP_REP.emitted]
    mov     w0, w22
    bl      active_insert
    mov     w0, w22
    bl      set_visible
    b       binarypath_next_frame.retain
binarypath_next_frame.check:
    mov     w22, #0
binarypath_next_frame.bit:
    ldr     w0, [x20, #BP_REP.first]
    add     w0, w0, w22
    bl      char_coord
    ldr     x10, [x20, #BP_REP.coord]
    cmp     x9, x10
    b.ne    binarypath_next_frame.retain
    add     w22, w22, #1
    ldr     w10, [x20, #BP_REP.count]
    cmp     w22, w10
    b.lo    binarypath_next_frame.bit
    mov     w22, #0
binarypath_next_frame.hide:
    ldr     w0, [x20, #BP_REP.first]
    add     w0, w0, w22
    mov     w1, #0
    bl      set_visibility
    add     w22, w22, #1
    ldr     w10, [x20, #BP_REP.count]
    cmp     w22, w10
    b.lo    binarypath_next_frame.hide
    ldr     w0, [x20, #BP_REP.source]
    bl      set_visible
    ldr     w0, [x20, #BP_REP.source]
    MOV64   w1, BP_COLLAPSE
    bl      scene_activate_name
    ldr     w0, [x20, #BP_REP.source]
    bl      active_insert
    b       binarypath_next_frame.next
binarypath_next_frame.retain:
    LDX     x9, bp_active
    str     x20, [x9, x21, lsl #3]
    add     x21, x21, #1
binarypath_next_frame.next:
    add     x19, x19, #1
    b       binarypath_next_frame.rep
binarypath_next_frame.traveled:
    STX     x21, bp_active_count
    bl      active_empty
    cbz     w9, binarypath_next_frame.tick
    mov     w9, #1
    STB     w9, bp_wipe
binarypath_next_frame.wipe:
    mov     w19, #2
binarypath_next_frame.group:
    LDX     x9, bp_group_pos
    LDX     x10, bp_group_count
    cmp     x9, x10
    b.hs    binarypath_next_frame.complete
    add     x10, x9, #1
    STX     x10, bp_group_pos
    LDX     x10, bp_groups
    add     x9, x10, x9, lsl #4
    ldp     x20, x21, [x9]
    mov     x22, #0
binarypath_next_frame.char:
    cmp     x22, x21
    b.hs    binarypath_next_frame.next_group
    ldr     w23, [x20, x22, lsl #2]
    mov     w0, w23
    MOV64   w1, BP_BRIGHTEN
    bl      scene_activate_name
    mov     w0, w23
    bl      set_visible
    mov     w0, w23
    bl      active_insert
    add     x22, x22, #1
    b       binarypath_next_frame.char
binarypath_next_frame.complete:
    mov     w9, #1
    STB     w9, bp_complete
binarypath_next_frame.next_group:
    subs    w19, w19, #1
    b.ne    binarypath_next_frame.group
binarypath_next_frame.tick:
    bl      update
binarypath_next_frame.frame:
    mov     w9, #1
binarypath_next_frame.out:
    ldp     x23, x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

    .section .rodata
    .balign 8
bp_fifth: .double 0.2
bp_half: .double 0.5

    TSTATE
    .balign 8
bp_groups: .skip 8
bp_group_count: .skip 8
bp_group_pos: .skip 8
bp_chars: .skip 8
bp_count: .skip 8
bp_reps: .skip 8
bp_pending: .skip 8
bp_pending_count: .skip 8
bp_active: .skip 8
bp_active_count: .skip 8
bp_max_active: .skip 8
bp_coords: .skip 8
bp_coord_count: .skip 8
bp_map: .skip 8
bp_map_width: .skip 8
bp_symbol: .skip 8
bp_final_fg: .skip 8
bp_final_bg: .skip 8
bp_dim_fg: .skip 8
bp_dim_bg: .skip 8
bp_pair: .skip 8 * 2
bp_steps: .skip 8
bp_fg_spectrum: .skip 8 * 11
bp_bg_spectrum: .skip 8 * 11
bp_complete: .skip 1
bp_last: .skip 1
bp_wipe: .skip 1

    .text
