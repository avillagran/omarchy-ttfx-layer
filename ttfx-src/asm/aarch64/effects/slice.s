// effects/slice.s - "Slices the input in half and slides it into place
// from opposite directions" (src/effects/slice.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Slice).
//
// The effect draws nothing from the RNG, and the active set and the
// renderer's painter order are both by slot, so characters are sent in
// Rust's order only for the auto path names' sake.

.equ HAVE_slice, 1

.equ SLICE.direction,           0       // 0 vertical, 1 horizontal, 2 diagonal
.equ SLICE.speed,               8       // f64
.equ SLICE.ease,                16      // easing id
.equ SLICE.final_stops,         24      // *const u64
.equ SLICE.final_stop_count,    32
.equ SLICE.final_steps,         40      // *const i64
.equ SLICE.final_step_count,    48
.equ SLICE.final_direction,     56
.equ SLICE_size,                64

.equ SL_VERTICAL,               0
.equ SL_HORIZONTAL,             1

    .text

// slice_build: Slice::build.
slice_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    bl      sl_final_colors
    ldr     d0, [x19, #SLICE.speed]
    STD     d0, sl_speed
    ldr     w9, [x19, #SLICE.ease]
    STW     w9, sl_ease
    ldr     x9, [x19, #SLICE.direction]
    cmp     x9, #SL_VERTICAL
    b.eq    slice_build.vertical
    cmp     x9, #SL_HORIZONTAL
    b.eq    slice_build.horizontal
    // diagonal: the first half of the diagonals from the bottom (origin
    // column of the group's first character), the second half from the top
    // (origin column of its last), interleaved
    mov     w0, #FILTER_INPUT
    mov     w1, #GROUP_DIAG_BL_TO_TR
    bl      get_characters_grouped
    mov     x21, x9                     // groups
    lsr     x22, x2, #1                 // len(left) = n / 2
    sub     x24, x2, x22                // len(right)
    mov     x23, #0
slice_build.diag:
    cmp     x23, x22
    b.hs    slice_build.diag_right
    add     x9, x21, x23, lsl #4
    ldr     x20, [x9]                   // slots
    ldr     x3, [x9, #8]
    ldr     w9, [x20]
    LDX     x2, ch_icol
    ldr     w9, [x2, x9, lsl #2]        // column (row 0 = canvas.bottom - 1)
    mov     x2, x9
    bl      sl_send_group
slice_build.diag_right:
    cmp     x23, x24
    b.hs    slice_build.diag_next
    add     x9, x22, x23
    add     x9, x21, x9, lsl #4
    ldr     x20, [x9]
    ldr     x3, [x9, #8]
    add     x9, x20, x3, lsl #2
    ldur    w9, [x9, #-4]
    LDX     x2, ch_icol
    ldr     w9, [x2, x9, lsl #2]
    LDX     x2, canvas_top
    add     x2, x2, #1
    lsl     x2, x2, #32
    orr     x2, x2, x9
    bl      sl_send_group
slice_build.diag_next:
    add     x23, x23, #1
    cmp     x23, x24                    // len(right) >= len(left)
    b.lo    slice_build.diag
    b       slice_build.done

slice_build.vertical:
    // rows bottom to top: row i's left half from the top, the opposite
    // row's right half from the bottom
    mov     w0, #FILTER_INPUT
    mov     w1, #GROUP_ROW_BOTTOM_TO_TOP
    bl      get_characters_grouped
    mov     x21, x9
    mov     x22, x2
    mov     x23, #0
slice_build.row:
    cmp     x23, x22
    b.hs    slice_build.done
    add     x9, x21, x23, lsl #4
    ldr     x20, [x9]
    ldr     x24, [x9, #8]
slice_build.left:
    cbz     x24, slice_build.right_row
    ldr     w0, [x20]
    LDX     x9, ch_icol
    ldrsw   x1, [x9, x0, lsl #2]
    LDX     x9, text_center_col
    cmp     x1, x9
    b.gt    slice_build.left_next
    mov     w9, w1
    LDX     x1, canvas_top
    add     x1, x1, #1
    lsl     x1, x1, #32
    orr     x1, x1, x9
    bl      sl_send
slice_build.left_next:
    add     x20, x20, #4
    sub     x24, x24, #1
    b       slice_build.left
slice_build.right_row:
    sub     x9, x22, x23
    sub     x9, x9, #1
    add     x9, x21, x9, lsl #4
    ldr     x20, [x9]
    ldr     x24, [x9, #8]
slice_build.right:
    cbz     x24, slice_build.row_next
    ldr     w0, [x20]
    LDX     x9, ch_icol
    ldrsw   x1, [x9, x0, lsl #2]
    LDX     x9, text_center_col
    cmp     x1, x9
    b.le    slice_build.right_next
    mov     w1, w1                      // row 0 = canvas.bottom - 1
    bl      sl_send
slice_build.right_next:
    add     x20, x20, #4
    sub     x24, x24, #1
    b       slice_build.right
slice_build.row_next:
    add     x23, x23, #1
    b       slice_build.row

slice_build.horizontal:
    LDD     d0, sl_speed
    fadd    d0, d0, d0                  // movement_speed *= 2.0
    STD     d0, sl_speed
    mov     w0, #(FILTER_INPUT | FILTER_INNER_FILL | FILTER_OUTER_FILL)
    mov     w1, #GROUP_COLUMN_R2L
    bl      get_characters_grouped
    // trim each column to the text rectangle in place; drop empty columns
    mov     x21, x9
    mov     x4, x2                      // groups in
    mov     x22, #0                     // groups out
    mov     x5, #0
    LDX     x12, text_left
    LDX     x13, text_right
    LDX     x14, text_bottom
    LDX     x15, text_top
    LDX     x6, ch_icol
    LDX     x7, ch_irow
slice_build.trim:
    cmp     x5, x4
    b.hs    slice_build.trimmed
    add     x9, x21, x5, lsl #4
    ldr     x1, [x9]                    // slots
    ldr     x10, [x9, #8]
    mov     x11, #0                     // kept
    mov     x3, #0
slice_build.trim_char:
    cmp     x3, x10
    b.hs    slice_build.trim_kept
    ldr     w0, [x1, x3, lsl #2]
    ldrsw   x9, [x6, x0, lsl #2]
    cmp     x9, x12
    b.lt    slice_build.trim_skip
    cmp     x9, x13
    b.gt    slice_build.trim_skip
    ldrsw   x9, [x7, x0, lsl #2]
    cmp     x9, x14
    b.lt    slice_build.trim_skip
    cmp     x9, x15
    b.gt    slice_build.trim_skip
    str     w0, [x1, x11, lsl #2]
    add     x11, x11, #1
slice_build.trim_skip:
    add     x3, x3, #1
    b       slice_build.trim_char
slice_build.trim_kept:
    cbz     x11, slice_build.trim_next
    add     x9, x21, x22, lsl #4
    str     x1, [x9]
    str     x11, [x9, #8]
    add     x22, x22, #1
slice_build.trim_next:
    add     x5, x5, #1
    b       slice_build.trim
slice_build.trimmed:
    // column i's bottom half from the left, the opposite column's top half
    // from the right
    mov     x23, #0
slice_build.column:
    cmp     x23, x22
    b.hs    slice_build.done
    add     x9, x21, x23, lsl #4
    ldr     x20, [x9]
    ldr     x24, [x9, #8]
slice_build.bottom:
    cbz     x24, slice_build.top_column
    ldr     w0, [x20]
    LDX     x9, ch_irow
    ldrsw   x1, [x9, x0, lsl #2]
    LDX     x9, text_center_row
    cmp     x1, x9
    b.gt    slice_build.bottom_next
    lsl     x1, x1, #32                 // column 0 = canvas.left - 1
    bl      sl_send
slice_build.bottom_next:
    add     x20, x20, #4
    sub     x24, x24, #1
    b       slice_build.bottom
slice_build.top_column:
    sub     x9, x22, x23
    sub     x9, x9, #1
    add     x9, x21, x9, lsl #4
    ldr     x20, [x9]
    ldr     x24, [x9, #8]
slice_build.top:
    cbz     x24, slice_build.column_next
    ldr     w0, [x20]
    LDX     x9, ch_irow
    ldrsw   x1, [x9, x0, lsl #2]
    LDX     x9, text_center_row
    cmp     x1, x9
    b.le    slice_build.top_next
    lsl     x1, x1, #32
    LDW     w9, canvas_right
    add     w9, w9, #1
    orr     x1, x1, x9
    bl      sl_send
slice_build.top_next:
    add     x20, x20, #4
    sub     x24, x24, #1
    b       slice_build.top
slice_build.column_next:
    add     x23, x23, #1
    b       slice_build.column

slice_build.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// sl_send_group(x20=u32 slots, x3=count, x2=origin): sl_send for each.
// Preserves x19 and x21-x24.
sl_send_group:
    PUSH2   x21, x22
    PUSH2   x23, x30
    mov     x21, x3
    mov     x22, x2
    mov     x23, x20
sl_send_group.each:
    cbz     x21, sl_send_group.out
    ldr     w0, [x23]
    mov     x1, x22
    bl      sl_send
    add     x23, x23, #4
    sub     x21, x21, #1
    b       sl_send_group.each
sl_send_group.out:
    POP2    x23, x30
    POP2    x21, x22
    ret

// sl_send(w0=slot, x1=origin): the send_to! macro - set the origin, a path
// to the input coordinate, activate it; then active_characters.extend and
// the closing set_character_visibility. Clobbers C.
sl_send:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w19, w0
    bl      set_coordinate
    mov     w0, w19
    LDD     d0, sl_speed
    LDW     w1, sl_ease
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #AUTO
    bl      path_new
    mov     w21, w9
    mov     w0, w19
    bl      char_input_coord
    mov     w0, w21
    mov     x1, x9
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w19
    mov     w1, w21
    bl      path_activate
    mov     w0, w19
    bl      active_insert
    mov     w0, w19
    bl      set_visible
    POP2    x22, x30
    POP2    x19, x21
    ret

// sl_final_colors (x19 = config): the final gradient, its coordinate
// mapping over the text rectangle, and each input character's appearance
// (the mapped fg, or its input colors under dynamic handling).
sl_final_colors:
    PUSH2   x21, x22
    PUSH2   x23, x30
    ldr     x0, [x19, #SLICE.final_steps]
    ldr     x3, [x19, #SLICE.final_step_count]
    ldr     x1, [x19, #SLICE.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    mov     x21, x9
    ldr     x0, [x19, #SLICE.final_stops]
    ldr     x1, [x19, #SLICE.final_stop_count]
    ldr     x2, [x19, #SLICE.final_steps]
    ldr     x3, [x19, #SLICE.final_step_count]
    mov     x4, x21
    bl      gradient_new
    mov     x0, x21
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, sl_map_width
    ldr     x6, [x19, #SLICE.final_direction]
    bl      gradient_map
    mov     x21, x9                     // map
    LDX     x22, input_count
    mov     x23, #0
sl_final_colors.char:
    cmp     x23, x22
    b.hs    sl_final_colors.out
    LDX     x9, input_chars
    ldr     w0, [x9, x23, lsl #2]
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    sl_final_colors.dynamic
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x0, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, sl_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x0, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    ldr     x2, [x21, x9, lsl #3]
    mov     x3, #NONE
    b       sl_final_colors.appear
sl_final_colors.dynamic:
    LDX     x2, ch_fg
    ldr     x2, [x2, x0, lsl #3]
    LDX     x3, ch_bg
    ldr     x3, [x3, x0, lsl #3]
sl_final_colors.appear:
    mov     x1, #0
    bl      set_appearance
    add     x23, x23, #1
    b       sl_final_colors.char
sl_final_colors.out:
    POP2    x23, x30
    POP2    x21, x22
    ret

// slice_next_frame -> w9 = 1 while characters are active, 0 when done.
slice_next_frame:
    PUSH1   x30
    bl      active_empty
    cbnz    w9, slice_next_frame.finished
    bl      update
    mov     w9, #1
    POP1    x30
    ret
slice_next_frame.finished:
    mov     w9, #0
    POP1    x30
    ret

    TSTATE
    .balign 8
sl_speed:               .skip 8
sl_map_width:           .skip 8
sl_ease:                .skip 4

    .text
