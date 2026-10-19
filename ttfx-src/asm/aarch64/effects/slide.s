// effects/slide.s - "Slide characters into view from outside the terminal"
// (src/effects/slide.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Slide).
//
// The effect draws nothing from the RNG and registers no events, so the
// order in which characters get their paths and scenes is free; it follows
// Rust's anyway. sld_groups is the group array (reversed in place where
// Rust reverses) walked by index; sld_active is a (cursor, remaining)
// array compacted in order like Vec::retain.

.equ HAVE_slide, 1

.equ SLIDE.speed,               0       // f64
.equ SLIDE.grouping,            8       // 0 row, 1 column, 2 diagonal
.equ SLIDE.gap,                 16
.equ SLIDE.reverse,             24      // bool
.equ SLIDE.merge,               32      // bool
.equ SLIDE.easing,              40      // easing id
.equ SLIDE.final_stops,         48      // *const u64
.equ SLIDE.final_stop_count,    56
.equ SLIDE.final_steps,         64      // *const i64
.equ SLIDE.final_step_count,    72
.equ SLIDE.final_frames,        80      // fits i32 (marshal checks)
.equ SLIDE.final_direction,     88
.equ SLIDE_size,                96

.equ SLD_ROW,                   0
.equ SLD_COLUMN,                1

// path names
.equ SLD_INPUT_PATH,            NAME_LITERAL + 0

    .text

// slide_build: Slide::build.
slide_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    bl      sld_final_map
    // the per-character gradient: with_steps([final stop 0, final fg], 10)
    ADRG    x0, sld_ten_steps
    mov     w3, #1
    mov     w1, #2
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, sld_pair_spectrum
    ldr     x9, [x19, #SLIDE.final_stops]
    ldr     x9, [x9]
    STX     x9, sld_pair_stops
    mov     x9, #NONE
    STX     x9, sld_last_fg
    ldr     x9, [x19, #SLIDE.grouping]
    ADRG    x3, sld_groupings
    ldrb    w1, [x3, x9]
    mov     w0, #FILTER_INPUT
    bl      get_characters_grouped
    STX     x9, sld_groups
    STX     x2, sld_group_count
    // every character: the "input_path" to its input coordinate
    mov     x21, #0
slide_build.path_group:
    LDX     x9, sld_group_count
    cmp     x21, x9
    b.hs    slide_build.paths_done
    LDX     x22, sld_groups
    add     x22, x22, x21, lsl #4
    ldr     x23, [x22, #8]
    ldr     x22, [x22]
slide_build.path_char:
    cbz     x23, slide_build.path_next
    ldr     w20, [x22]
    mov     w0, w20
    ldr     d0, [x19, #SLIDE.speed]
    ldr     w1, [x19, #SLIDE.easing]
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, SLD_INPUT_PATH
    bl      path_new
    mov     w24, w9
    mov     w0, w20
    bl      char_input_coord
    mov     w0, w24
    mov     x1, x9
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    add     x22, x22, #4
    sub     x23, x23, #1
    b       slide_build.path_char
slide_build.path_next:
    add     x21, x21, #1
    b       slide_build.path_group
slide_build.paths_done:
    mov     x21, #0
slide_build.group:
    LDX     x9, sld_group_count
    cmp     x21, x9
    b.hs    slide_build.grouped
    bl      sld_place_group
    add     x21, x21, #1
    b       slide_build.group
slide_build.grouped:
    // active_groups: at most one entry per group
    LDX     x0, sld_group_count
    lsl     x0, x0, #4
    add     x0, x0, #16
    bl      alloc
    STX     x9, sld_active
    STX     xzr, sld_active_count
    STX     xzr, sld_next_group
    STX     xzr, sld_gap_cur
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// sld_place_group (x21 = group index, x19 = config): the build loop's body
// for one group - the starting coordinates, the gradient scenes, and the
// group reversed in place where Rust reverses it.
//
// Rust's row/column/diagonal branches reduce to one flag, flip = merge ?
// (index even) : reverse_direction:
//   row:      flip -> from canvas.right + 1 in order, else from left - 1 reversed
//   column:   flip -> from canvas.bottom - 1 in order, else from top + 1 reversed
//   diagonal: flip -> from above the first character reversed, else from
//             below the last character in order
// Locals: [sp + 56] the starting coordinate.
sld_place_group:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x9, sld_groups
    add     x9, x9, x21, lsl #4
    ldr     x22, [x9]                   // slots
    ldr     x23, [x9, #8]               // count
    // flip -> w20
    ldr     w20, [x19, #SLIDE.reverse]
    and     w20, w20, #1
    ldr     x9, [x19, #SLIDE.merge]
    cbz     x9, sld_place_group.flipped
    mvn     w20, w21
    and     w20, w20, #1
sld_place_group.flipped:
    mov     w24, #0                     // reversed?
    ldr     x9, [x19, #SLIDE.grouping]
    cmp     x9, #SLD_ROW
    b.eq    sld_place_group.row
    cmp     x9, #SLD_COLUMN
    b.eq    sld_place_group.column
    // diagonal
    cbnz    w20, sld_place_group.diag_top
    // below the last character: (column - row, row - row) = (column - row, 0)
    add     x9, x22, x23, lsl #2
    ldur    w0, [x9, #-4]
    bl      char_input_coord
    asr     x3, x9, #32                 // row - (canvas.bottom - 1)
    sub     w9, w9, w3                  // row 0
    b       sld_place_group.diag_set
sld_place_group.diag_top:
    // above the first: + (canvas.top + 1 - row) on both axes
    mov     w24, #1
    ldr     w0, [x22]
    bl      char_input_coord
    asr     x3, x9, #32                 // row
    LDX     x2, canvas_top
    add     x2, x2, #1
    sub     x2, x2, x3                  // distance_from_outside
    add     w9, w9, w2                  // column + distance
    add     w3, w3, w2                  // row + distance
    lsl     x3, x3, #32
    orr     x9, x9, x3
sld_place_group.diag_set:
    str     x9, [sp, #56]
    mov     x21, #0
sld_place_group.diag_char:
    cmp     x21, x23
    b.hs    sld_place_group.scenes
    ldr     w0, [x22, x21, lsl #2]
    ldr     x1, [sp, #56]
    bl      set_coordinate
    add     x21, x21, #1
    b       sld_place_group.diag_char

sld_place_group.row:
    // (start column, input row)
    mov     x9, #0                      // canvas.left - 1
    cbnz    w20, sld_place_group.row_right
    mov     w24, #1
    b       sld_place_group.row_set
sld_place_group.row_right:
    LDX     x9, canvas_right
    add     w9, w9, #1
sld_place_group.row_set:
    str     x9, [sp, #56]
    mov     x21, #0
sld_place_group.row_char:
    cmp     x21, x23
    b.hs    sld_place_group.scenes
    ldr     w0, [x22, x21, lsl #2]
    bl      char_input_coord
    lsr     x9, x9, #32
    lsl     x9, x9, #32
    ldr     w3, [sp, #56]
    orr     x9, x9, x3
    mov     x1, x9
    ldr     w0, [x22, x21, lsl #2]
    bl      set_coordinate
    add     x21, x21, #1
    b       sld_place_group.row_char

sld_place_group.column:
    // (input column, start row)
    mov     x9, #0                      // canvas.bottom - 1
    cbnz    w20, sld_place_group.column_set
    mov     w24, #1
    LDX     x9, canvas_top
    add     x9, x9, #1
sld_place_group.column_set:
    lsl     x9, x9, #32
    str     x9, [sp, #56]
    mov     x21, #0
sld_place_group.column_char:
    cmp     x21, x23
    b.hs    sld_place_group.scenes
    ldr     w0, [x22, x21, lsl #2]
    bl      char_input_coord
    mov     w9, w9
    ldr     x3, [sp, #56]
    orr     x9, x9, x3
    mov     x1, x9
    ldr     w0, [x22, x21, lsl #2]
    bl      set_coordinate
    add     x21, x21, #1
    b       sld_place_group.column_char

sld_place_group.scenes:
    // the gradient scenes, in the group's original order
    mov     x21, #0
sld_place_group.scene_char:
    cmp     x21, x23
    b.hs    sld_place_group.reverse
    ldr     w0, [x22, x21, lsl #2]
    bl      sld_scene
    add     x21, x21, #1
    b       sld_place_group.scene_char
sld_place_group.reverse:
    cbz     w24, sld_place_group.out
    add     x3, x22, x23, lsl #2
    sub     x3, x3, #4
sld_place_group.swap:
    cmp     x22, x3
    b.hs    sld_place_group.out
    ldr     w9, [x22]
    ldr     w2, [x3]
    str     w2, [x22]
    str     w9, [x3]
    add     x22, x22, #4
    sub     x3, x3, #4
    b       sld_place_group.swap
sld_place_group.out:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// sld_scene(w0=slot), x19 = config: the character's gradient scene
// (Gradient::with_steps([final stop 0, mapped fg], 10) applied to its
// symbol, or its input colors under dynamic handling), activated.
sld_scene:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w20, w0
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w22, w9
    LDX     x9, ch_sym
    ldr     x9, [x9, x20, lsl #3]
    STX     x9, sld_symbol
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    sld_scene.dynamic
    mov     w0, w20
    bl      char_input_coord
    asr     x3, x9, #32
    LDX     x10, text_bottom
    sub     x3, x3, x10
    LDX     x10, sld_map_width
    mul     x3, x3, x10
    sxtw    x9, w9
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, sld_map
    ldr     x9, [x3, x9, lsl #3]
    LDX     x10, sld_last_fg
    cmp     x9, x10
    b.eq    sld_scene.gradient
    STX     x9, sld_last_fg
    ADRG    x0, sld_pair_stops
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, sld_ten_steps
    mov     w3, #1
    LDX     x4, sld_pair_spectrum
    bl      gradient_new
    mov     w9, w9
    STX     x9, sld_pair_len
sld_scene.gradient:
    // apply_gradient_to_symbols([symbol], final frames, pair spectrum): a
    // frame per color, the visuals shared by symbol and mapped color
    LDX     x0, sld_symbol
    LDX     x1, sld_pair_spectrum
    LDX     x2, sld_pair_len
    mov     x3, #NONE
    LDX     x4, sld_last_fg
    bl      visual_run
    mov     w0, w22
    mov     x1, x9
    ldr     w3, [x19, #SLIDE.final_frames]
    bl      visual_frames
    b       sld_scene.activate
sld_scene.dynamic:
    LDX     x9, ch_fg
    ldr     x3, [x9, x20, lsl #3]
    LDX     x9, ch_bg
    ldr     x4, [x9, x20, lsl #3]
    mov     w0, w22
    LDX     x1, sld_symbol
    ldr     w2, [x19, #SLIDE.final_frames]
    mov     w5, #0
    bl      scene_add_frame
sld_scene.activate:
    mov     w0, w20
    mov     w1, w22
    bl      scene_activate
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// sld_final_map (x19 = config): Gradient::new(final stops, final steps) and
// its coordinate mapping over the text rectangle.
sld_final_map:
    PUSH1   x30
    ldr     x0, [x19, #SLIDE.final_steps]
    ldr     x3, [x19, #SLIDE.final_step_count]
    ldr     x1, [x19, #SLIDE.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, sld_spectrum
    ldr     x0, [x19, #SLIDE.final_stops]
    ldr     x1, [x19, #SLIDE.final_stop_count]
    ldr     x2, [x19, #SLIDE.final_steps]
    ldr     x3, [x19, #SLIDE.final_step_count]
    LDX     x4, sld_spectrum
    bl      gradient_new
    LDX     x0, sld_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, sld_map_width
    ldr     x6, [x19, #SLIDE.final_direction]
    bl      gradient_map
    STX     x9, sld_map
    POP1    x30
    ret

// slide_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
slide_next_frame:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    LDX     x19, effect_config
    LDX     x9, sld_next_group
    LDX     x10, sld_group_count
    cmp     x9, x10
    b.lo    slide_next_frame.run
    LDX     x9, sld_active_count
    cbnz    x9, slide_next_frame.run
    bl      active_empty
    cbnz    w9, slide_next_frame.finished
slide_next_frame.run:
    LDX     x9, sld_next_group
    LDX     x10, sld_group_count
    cmp     x9, x10
    b.hs    slide_next_frame.release
    LDX     x3, sld_gap_cur
    ldr     x10, [x19, #SLIDE.gap]
    cmp     x3, x10
    b.ne    slide_next_frame.wait
    // active_groups.push(pending_groups.remove(0))
    add     x10, x9, #1
    STX     x10, sld_next_group
    lsl     x9, x9, #4
    LDX     x10, sld_groups
    add     x9, x9, x10
    LDX     x3, sld_active_count
    lsl     x10, x3, #4
    LDX     x11, sld_active
    add     x10, x10, x11
    ldp     x2, x11, [x9]
    stp     x2, x11, [x10]
    add     x3, x3, #1
    STX     x3, sld_active_count
    STX     xzr, sld_gap_cur
    b       slide_next_frame.release
slide_next_frame.wait:
    add     x3, x3, #1
    STX     x3, sld_gap_cur
slide_next_frame.release:
    // each active group releases its next character; empty groups are
    // dropped in place (retain)
    LDX     x21, sld_active
    LDX     x22, sld_active_count
    mov     x23, #0                     // read index
    mov     x24, #0                     // write index
slide_next_frame.group:
    cmp     x23, x22
    b.hs    slide_next_frame.released
    add     x9, x21, x23, lsl #4
    ldr     x3, [x9]                    // cursor
    ldr     x2, [x9, #8]                // remaining (never 0 here)
    ldr     w19, [x3]
    add     x3, x3, #4
    subs    x2, x2, #1
    b.eq    slide_next_frame.drop
    add     x9, x21, x24, lsl #4
    str     x3, [x9]
    str     x2, [x9, #8]
    add     x24, x24, #1
slide_next_frame.drop:
    mov     w0, w19
    bl      set_visible
    mov     w0, w19
    MOV64   w1, SLD_INPUT_PATH
    bl      path_activate_name
    mov     w0, w19
    bl      active_insert
    add     x23, x23, #1
    b       slide_next_frame.group
slide_next_frame.released:
    STX     x24, sld_active_count
    bl      update
    mov     w9, #1
    b       slide_next_frame.out
slide_next_frame.finished:
    mov     w9, #0
slide_next_frame.out:
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

    .section .rodata
    .balign 8
sld_ten_steps:  .quad 10
// CharacterGroup per SlideGrouping
sld_groupings:  .byte GROUP_ROW_TOP_TO_BOTTOM, GROUP_COLUMN_L2R, GROUP_DIAG_TL_TO_BR

    TSTATE
    .balign 8
sld_groups:             .skip 8
sld_group_count:        .skip 8
sld_next_group:         .skip 8
sld_gap_cur:            .skip 8
sld_active:             .skip 8
sld_active_count:       .skip 8
sld_spectrum:           .skip 8
sld_map:                .skip 8
sld_map_width:          .skip 8
sld_pair_stops:         .skip 8 * 2
sld_pair_spectrum:      .skip 8
sld_pair_len:           .skip 8
sld_last_fg:            .skip 8
sld_symbol:             .skip 8

    .text
