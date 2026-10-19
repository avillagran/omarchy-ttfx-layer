// effects/print.s - "Lines are printed one at a time following a print head"
// (src/effects/print_effect.rs).
//
// Config (src/asm/effects.rs, the Print arm).
//
// Rows are the RowTopToBottom groups, trimmed in place as Row.__init__ and
// the carriage return trim them, so each group's slot array is the row: the
// first pr_pos slots of the current row are typed, the rest untyped, and
// every row before pr_cur is a processed row (fully typed). Print draws no
// random numbers.

.equ HAVE_print, 1

.equ PRINT.return_speed,        0       // f64 print_head_return_speed
.equ PRINT.print_speed,         8
.equ PRINT.easing,              16      // print_head_easing id
.equ PRINT.final_stops,         24      // *const u64
.equ PRINT.final_stop_count,    32
.equ PRINT.final_steps,         40      // *const i64
.equ PRINT.final_step_count,    48
.equ PRINT.final_direction,     56
.equ PRINT_size,                64

// path name
.equ PR_CARRIAGE_RETURN,        NAME_LITERAL + 0

.equ PR_SPACE,                  0x100000020 // " " packed
.equ PR_WHITE,                  0xffffff

    .text

// print_build: PrintIterator.__init__ (the typing head) + Print::build.
print_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    // the typing head at (1, 1), added before anything else
    mov     w0, #0x2588                 // █
    bl      utf8_pack
    mov     x0, x9
    MOV64   x1, ((1 << 32) | 1)
    bl      add_character
    STW     w9, pr_head
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    cset    w9, eq
    STB     w9, pr_dynamic
    // the final gradient mapping (built before the dynamic check upstream;
    // it draws nothing, so building it unconditionally is equivalent)
    bl      pr_final_map
    // the frame symbols: █ ▓ ▒ ░ and the character's own
    ADRG    x19, pr_block_codes
    mov     w21, #0
print_build.block:
    ldr     w0, [x19, x21, lsl #2]
    bl      utf8_pack
    ADRG    x3, pr_symbols
    str     x9, [x3, x21, lsl #3]
    add     w21, w21, #1
    cmp     w21, #4
    b.lo    print_build.block
    // one row per input row, top to bottom, fill characters included
    mov     w0, #(FILTER_INPUT | FILTER_INNER_FILL | FILTER_OUTER_FILL)
    mov     w1, #GROUP_ROW_TOP_TO_BOTTOM
    bl      get_characters_grouped
    STX     x9, pr_rows
    STX     x2, pr_row_count
    cbz     x2, print_build.no_rows
    mov     x21, x9
    mov     x22, x2
print_build.row:
    mov     x0, x21
    bl      pr_make_row
    add     x21, x21, #16
    subs    x22, x22, #1
    b.ne    print_build.row
    STX     xzr, pr_cur
    STX     xzr, pr_pos
    mov     w9, #1
    STB     w9, pr_typing
    STX     xzr, pr_last_column
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret
print_build.no_rows:
    // pending_rows.remove(0) on an empty list panics upstream
    ADRG    x0, pr_msg_no_rows
    mov     w1, #pr_msg_no_rows_len
    b       fatal

// pr_make_row(x0=group record): PrintIterator.Row.__init__ - trims the
// group in place, then gives each kept character its typed scene.
pr_make_row:
    stp     x19, x20, [sp, #-80]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    stp     x30, x0, [sp, #48]          // [sp, #56]: the group record
                                        // [sp, #64]: the empty memo entry
    ldr     x21, [x0]                   // slots
    ldr     x22, [x0, #8]               // count
    // a row of spaces keeps only its first character
    MOV64   x9, PR_SPACE
    LDX     x3, ch_sym
    mov     x19, #0
pr_make_row.space:
    cmp     x19, x22
    b.hs    pr_make_row.all_spaces
    ldr     w2, [x21, x19, lsl #2]
    ldr     x10, [x3, x2, lsl #3]
    cmp     x10, x9
    b.ne    pr_make_row.extent
    add     x19, x19, #1
    b       pr_make_row.space
pr_make_row.all_spaces:
    mov     w22, #1
    b       pr_make_row.trimmed
pr_make_row.extent:
    // right extent: the rightmost non-fill input column
    LDX     x9, ch_flags
    LDX     x3, ch_icol
    mov     x4, #0x8000000000000000
    mov     x19, #0
pr_make_row.max:
    cmp     x19, x22
    b.hs    pr_make_row.keep
    ldr     w2, [x21, x19, lsl #2]
    add     x19, x19, #1
    ldrh    w10, [x9, x2, lsl #1]
    tst     w10, #CF_FILL
    b.ne    pr_make_row.max
    ldrsw   x5, [x3, x2, lsl #2]
    cmp     x5, x4
    csel    x4, x5, x4, gt
    b       pr_make_row.max
pr_make_row.keep:
    mov     x19, #0
    mov     x5, #0                      // kept
pr_make_row.filter:
    cmp     x19, x22
    b.hs    pr_make_row.filtered
    ldr     w2, [x21, x19, lsl #2]
    add     x19, x19, #1
    ldrsw   x10, [x3, x2, lsl #2]
    cmp     x10, x4
    b.gt    pr_make_row.filter
    str     w2, [x21, x5, lsl #2]
    add     x5, x5, #1
    b       pr_make_row.filter
pr_make_row.filtered:
    mov     x22, x5
pr_make_row.trimmed:
    ldr     x9, [sp, #56]
    str     x22, [x9, #8]
    mov     x19, #0
pr_make_row.char:
    cmp     x19, x22
    b.hs    pr_make_row.done
    ldr     w23, [x21, x19, lsl #2]
    add     x19, x19, #1
    // moved to (input column, 1)
    LDX     x1, ch_icol
    ldr     w1, [x1, x23, lsl #2]
    orr     x1, x1, #(1 << 32)
    mov     w0, w23
    bl      set_coordinate
    mov     w0, w23
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9
    LDX     x9, ch_sym
    ldr     x9, [x9, x23, lsl #3]
    ADRG    x3, pr_symbols
    str     x9, [x3, #32]
    LDB     w9, pr_dynamic
    cbnz    w9, pr_make_row.dynamic
    // white -> the final color in 5 steps over the five symbols; the frames
    // depend on the input symbol and final color only, so later characters
    // with the same pair copy the first one's scene (when neither scene
    // applies preexisting colors)
    mov     w0, w23
    bl      pr_final_color
    mov     x20, x9
    SCENE_PTR x3, x24
    ldr     w9, [x3, #SC_FLAGS]
    tst     w9, #(SCF_PREEXISTING | SCF_PRE_BOLD)
    b.ne    pr_make_row.head_gradient
    ADRG    x0, pr_symbols
    ldr     x0, [x0, #32]
    mov     x1, x20
    mov     x2, #NONE
    bl      visual_run_find
    cbz     x9, pr_make_row.head_template
    ldr     w9, [x9]                    // the template scene
    SCENE_PTR x9, x9
    mov     w0, w24
    ldr     x1, [x9, #SC_FRAMES]
    ldr     w2, [x9, #SC_COUNT]
    bl      scene_append_frames
    b       pr_make_row.activate
pr_make_row.head_template:
    str     x3, [sp, #64]               // the empty memo entry
    mov     x0, #8
    bl      alloc
    mov     w10, #1
    str     w10, [x9]
    str     w24, [x9, #4]
    add     x4, x9, #4
    ldr     x3, [sp, #64]
    ADRG    x0, pr_symbols
    ldr     x0, [x0, #32]
    mov     x1, x20
    mov     x2, #NONE
    bl      visual_run_keep
pr_make_row.head_gradient:
    ADRG    x0, pr_fg_spectrum
    mov     x1, x20
    bl      pr_head_gradient
    mov     w5, w9
    ADRG    x4, pr_fg_spectrum
    mov     x6, #0
    mov     x7, #0
    mov     w0, w24
    ADRG    x1, pr_symbols
    mov     w2, #5
    mov     w3, #3
    bl      scene_apply_gradient
    b       pr_make_row.activate
pr_make_row.dynamic:
    // white -> each input color present; neither: a white head, then the
    // symbol without colors
    mov     w20, #0                     // fg count
    LDX     x1, ch_fg
    ldr     x1, [x1, x23, lsl #3]
    cmn     x1, #1                      // NONE
    b.eq    pr_make_row.dyn_bg
    ADRG    x0, pr_fg_spectrum
    bl      pr_head_gradient
    mov     w20, w9
pr_make_row.dyn_bg:
    mov     w9, #0                      // bg count
    LDX     x1, ch_bg
    ldr     x1, [x1, x23, lsl #3]
    cmn     x1, #1                      // NONE
    b.eq    pr_make_row.dyn_pair
    ADRG    x0, pr_bg_spectrum
    bl      pr_head_gradient
pr_make_row.dyn_pair:
    orr     w3, w20, w9
    cbz     w3, pr_make_row.dyn_plain
    // spectra (0 for an absent gradient) and counts
    ADRG    x6, pr_bg_spectrum
    cmp     w9, #0
    csel    x6, xzr, x6, eq
    ADRG    x4, pr_fg_spectrum
    cmp     w20, #0
    csel    x4, x4, xzr, ne
    mov     w5, w20
    mov     w7, w9
    mov     w0, w24
    ADRG    x1, pr_symbols
    mov     w2, #5
    mov     w3, #3
    bl      scene_apply_gradient
    b       pr_make_row.activate
pr_make_row.dyn_plain:
    ADRG    x4, pr_fg_spectrum
    ADRG    x0, pr_pair_stops
    mov     x9, #PR_WHITE
    str     x9, [x0]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, pr_four_steps
    mov     w3, #1
    bl      gradient_new
    mov     w5, w9
    ADRG    x4, pr_fg_spectrum
    mov     x6, #0
    mov     x7, #0
    mov     w0, w24
    ADRG    x1, pr_symbols
    mov     w2, #4
    mov     w3, #3
    bl      scene_apply_gradient
    mov     w0, w24
    ADRG    x1, pr_symbols
    ldr     x1, [x1, #32]
    mov     w2, #3
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
pr_make_row.activate:
    mov     w0, w23
    mov     w1, w24
    bl      scene_activate
    b       pr_make_row.char
pr_make_row.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #80
    ret

// pr_head_gradient(x0=out spectrum, x1=color) -> w9 = length:
// Gradient::with_steps([white, color], 5).
pr_head_gradient:
    mov     x4, x0
    ADRG    x0, pr_pair_stops
    mov     x9, #PR_WHITE
    str     x9, [x0]
    str     x1, [x0, #8]
    mov     w1, #2
    ADRG    x2, pr_five_steps
    mov     w3, #1
    b       gradient_new

// pr_final_color(w0=slot) -> x9: the final gradient at the input
// coordinate, white outside the text rectangle (character_final_color_map).
// Clobbers x2, x3, x10, x11.
pr_final_color:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, w0, uxtw #2]
    LDX     x3, ch_icol
    ldrsw   x3, [x3, w0, uxtw #2]
    LDX     x10, text_bottom
    cmp     x9, x10
    b.lt    pr_final_color.white
    LDX     x11, text_top
    cmp     x9, x11
    b.gt    pr_final_color.white
    LDX     x11, text_left
    cmp     x3, x11
    b.lt    pr_final_color.white
    LDX     x2, text_right
    cmp     x3, x2
    b.gt    pr_final_color.white
    sub     x9, x9, x10
    LDX     x2, pr_final_map_width
    mul     x9, x9, x2
    add     x9, x9, x3
    sub     x9, x9, x11
    LDX     x3, pr_final_map_ptr
    ldr     x9, [x3, x9, lsl #3]
    ret
pr_final_color.white:
    mov     x9, #PR_WHITE
    ret

// pr_final_map: Gradient::new(final stops, final steps) and its coordinate
// mapping over the text rectangle.
pr_final_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #PRINT.final_steps]
    ldr     x3, [x19, #PRINT.final_step_count]
    ldr     x1, [x19, #PRINT.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, pr_final_spectrum
    ldr     x0, [x19, #PRINT.final_stops]
    ldr     x1, [x19, #PRINT.final_stop_count]
    ldr     x2, [x19, #PRINT.final_steps]
    ldr     x3, [x19, #PRINT.final_step_count]
    LDX     x4, pr_final_spectrum
    bl      gradient_new
    LDX     x0, pr_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, pr_final_map_width
    ldr     x6, [x19, #PRINT.final_direction]
    bl      gradient_map
    STX     x9, pr_final_map_ptr
    POP2    x19, x30
    ret

// pr_hide_head(w0=slot, x1=payload): SET_INVISIBLE_CALLBACK.
pr_hide_head:
    mov     w1, #0
    b       set_visibility

// pr_all_fill(x0=slots, x1=count) -> w9 = 1 when every one is a fill
// character (vacuously for none). Clobbers x2, x3, x10.
pr_all_fill:
    LDX     x3, ch_flags
    mov     x2, #0
pr_all_fill.next:
    cmp     x2, x1
    b.hs    pr_all_fill.yes
    ldr     w9, [x0, x2, lsl #2]
    add     x2, x2, #1
    ldrh    w10, [x3, x9, lsl #1]
    tst     w10, #CF_FILL
    b.ne    pr_all_fill.next
    mov     w9, #0
    ret
pr_all_fill.yes:
    mov     w9, #1
    ret

// print_next_frame -> w9 = 1 for a frame, 0 when done.
print_next_frame:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDB     w9, pr_typing
    cbnz    w9, print_next_frame.step
    bl      active_empty
    cbnz    w9, print_next_frame.finished
print_next_frame.step:
    // the print head performing a carriage return
    LDW     w0, pr_head
    LDX     x9, ch_path
    ldr     w9, [x9, x0, lsl #2]
    cmn     w9, #1                      // NONE
    b.ne    print_next_frame.tick
    LDX     x9, pr_cur
    LDX     x10, pr_rows
    add     x9, x10, x9, lsl #4
    ldr     x21, [x9]                   // slots
    ldr     x22, [x9, #8]               // count
    LDX     x19, pr_pos
    cmp     x19, x22
    b.hs    print_next_frame.row_done
    // type min(untyped, print_speed) characters
    sub     x20, x22, x19
    LDX     x9, effect_config
    ldr     x9, [x9, #PRINT.print_speed]
    cmp     x9, x20
    csel    x20, x9, x20, lt
print_next_frame.type:
    cmp     x20, #0
    b.le    print_next_frame.typed
    sub     x20, x20, #1
    ldr     w23, [x21, x19, lsl #2]
    add     x19, x19, #1
    mov     w0, w23
    bl      set_visible
    mov     w0, w23
    bl      active_insert
    LDX     x9, ch_icol
    ldrsw   x9, [x9, x23, lsl #2]
    STX     x9, pr_last_column
    b       print_next_frame.type
print_next_frame.typed:
    STX     x19, pr_pos
    b       print_next_frame.tick
print_next_frame.row_done:
    LDX     x9, pr_cur
    add     x9, x9, #1
    LDX     x10, pr_row_count
    cmp     x9, x10
    b.hs    print_next_frame.last_row
    STX     x9, pr_cur
    STX     xzr, pr_pos
    // move every processed row up one
    LDX     x23, pr_rows
    mov     x24, x9
print_next_frame.up_row:
    ldr     x21, [x23]
    ldr     x22, [x23, #8]
    mov     x19, #0
print_next_frame.up_char:
    cmp     x19, x22
    b.hs    print_next_frame.up_next
    ldr     w0, [x21, x19, lsl #2]
    bl      char_coord
    ldr     w0, [x21, x19, lsl #2]
    add     x19, x19, #1
    mov     x3, #(1 << 32)
    add     x1, x9, x3
    bl      set_coordinate
    b       print_next_frame.up_char
print_next_frame.up_next:
    add     x23, x23, #16
    subs    x24, x24, #1
    b.ne    print_next_frame.up_row
    // x23 = the new current row, x23 - 16 the last processed one
    ldur    x0, [x23, #-16]
    ldur    x1, [x23, #-8]
    bl      pr_all_fill
    cbnz    w9, print_next_frame.head
    ldr     x0, [x23]
    ldr     x1, [x23, #8]
    bl      pr_all_fill
    cbnz    w9, print_next_frame.head
    // keep left extent <= column <= text right
    ldr     x21, [x23]
    ldr     x22, [x23, #8]
    LDX     x9, ch_flags
    LDX     x3, ch_icol
    mov     x4, #0x7fffffffffffffff
    mov     x19, #0
print_next_frame.min:
    cmp     x19, x22
    b.hs    print_next_frame.retain
    ldr     w2, [x21, x19, lsl #2]
    add     x19, x19, #1
    ldrh    w10, [x9, x2, lsl #1]
    tst     w10, #CF_FILL
    b.ne    print_next_frame.min
    ldrsw   x5, [x3, x2, lsl #2]
    cmp     x5, x4
    csel    x4, x5, x4, lt
    b       print_next_frame.min
print_next_frame.retain:
    LDX     x10, text_right
    mov     x19, #0
    mov     x5, #0
print_next_frame.retain_char:
    cmp     x19, x22
    b.hs    print_next_frame.retained
    ldr     w2, [x21, x19, lsl #2]
    add     x19, x19, #1
    ldrsw   x11, [x3, x2, lsl #2]
    cmp     x11, x4
    b.lt    print_next_frame.retain_char
    cmp     x11, x10
    b.gt    print_next_frame.retain_char
    str     w2, [x21, x5, lsl #2]
    add     x5, x5, #1
    b       print_next_frame.retain_char
print_next_frame.retained:
    str     x5, [x23, #8]
    cbz     x5, print_next_frame.empty_row
print_next_frame.head:
    // the head returns from the last typed column to the row's first
    LDW     w19, pr_head
    LDW     w1, pr_last_column
    orr     x1, x1, #(1 << 32)
    mov     w0, w19
    bl      set_coordinate
    mov     w0, w19
    bl      set_visible
    ldr     x9, [x23]
    ldr     w9, [x9]
    LDX     x3, ch_icol
    ldr     w21, [x3, x9, lsl #2]       // target column
    mov     w0, w19
    bl      paths_clear
    LDX     x9, effect_config
    ldr     d0, [x9, #PRINT.return_speed]
    ldr     w1, [x9, #PRINT.easing]
    mov     w0, w19
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #PR_CARRIAGE_RETURN
    bl      path_new
    mov     w22, w9
    mov     w0, w9
    mov     x1, #(1 << 32)
    orr     x1, x1, x21
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w19
    mov     w1, w22
    bl      path_activate
    // later registrations are duplicates, suppressed upstream
    LDB     w9, pr_registered
    cbnz    w9, print_next_frame.registered
    mov     w9, #1
    STB     w9, pr_registered
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w3, #PR_CARRIAGE_RETURN
    mov     w4, #ACT_CALLBACK
    ADRG    x5, pr_hide_head
    mov     x6, #0
    bl      event_register
print_next_frame.registered:
    mov     w0, w19
    bl      active_insert
    b       print_next_frame.tick
print_next_frame.last_row:
    STB     wzr, pr_typing
print_next_frame.tick:
    bl      update
    mov     w9, #1
    b       print_next_frame.out
print_next_frame.finished:
    mov     w9, #0
print_next_frame.out:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret
print_next_frame.empty_row:
    // current_row.untyped_chars[0] on an empty row panics upstream
    ADRG    x0, pr_msg_empty_row
    mov     w1, #pr_msg_empty_row_len
    b       fatal

    .section .rodata
    .balign 8
pr_five_steps:      .quad 5
pr_four_steps:      .quad 4
// █ ▓ ▒ ░
pr_block_codes:     .4byte 0x2588, 0x2593, 0x2592, 0x2591
STRING pr_msg_no_rows, "ttfx: asm engine: print: no rows\012"
STRING pr_msg_empty_row, "ttfx: asm engine: print: empty row after trimming\012"

    TSTATE
    .balign 8
pr_head:            .skip 8
pr_rows:            .skip 8         // (u32 slots, count) per row
pr_row_count:       .skip 8
pr_cur:             .skip 8
pr_pos:             .skip 8
pr_last_column:     .skip 8
pr_final_spectrum:  .skip 8
pr_final_map_ptr:   .skip 8
pr_final_map_width: .skip 8
pr_symbols:         .skip 8 * 5
pr_pair_stops:      .skip 8 * 2
pr_fg_spectrum:     .skip 8 * 16
pr_bg_spectrum:     .skip 8 * 16
pr_typing:          .skip 1
pr_dynamic:         .skip 1
pr_registered:      .skip 1

    .text
