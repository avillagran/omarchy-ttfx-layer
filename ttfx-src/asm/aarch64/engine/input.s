// engine/input.s - Terminal._preprocess_input_data (src/engine/input.rs):
// the mini terminal emulator that turns input text into rows of characters.
//
// The input walks codepoint by codepoint (one codepoint is one cell,
// faithfully), tracking the SGR color state and the cursor. Characters land in
// a screen map keyed by (row, column); a later write to a cell orphans the
// character that was there. Which malformed sequences are errors and which
// SGR parameters are silently ignored follows input.rs exactly, error text
// included.
//
// Slots are allocated per parsed character in Rust's arena order. The
// padding characters Rust builds for empty screen cells are never referenced,
// so only their ids are consumed.
//
// SCREEN_EMPTY, MADV_POPULATE_WRITE and the COLOR_SORT_* ids are in defs.inc.

    .text

// input_init: parse the input into rows of character slots.
input_init:
    PUSH1   x30
    bl      count_capacity
    bl      screen_init
    bl      preprocess
    bl      build_lines
    POP1    x30
    b       finish_lines

// count_capacity: an upper bound on parsed characters (tabs expand to at most
// tab_width cells), counted per byte: the input is valid UTF-8, so every
// codepoint is one byte outside 0x80-0xBF. Also finds whether the input is
// plain - no ESC and no carriage return - so the cursor only ever moves
// forward and down and no cell is written twice (see put_char).
count_capacity:
    LDX     x1, input_ptr
    LDX     x5, input_len
    add     x5, x5, x1
    mov     x10, #0                     // codepoints
    mov     x11, #0                     // tabs
    mov     x4, #0                      // newlines
    mov     w0, #0                      // ESC or CR seen
count_capacity.loop:
    cmp     x1, x5
    b.hs    count_capacity.done
    ldrb    w9, [x1], #1
    and     w3, w9, #0xC0
    cmp     w3, #0x80
    b.eq    count_capacity.loop         // continuation byte
    add     x10, x10, #1
    cmp     w9, #13
    b.hi    count_capacity.check_escape
    cmp     w9, #9
    b.eq    count_capacity.tab
    cmp     w9, #10
    b.eq    count_capacity.newline
    cmp     w9, #13
    b.ne    count_capacity.loop
    mov     w0, #1
    b       count_capacity.loop
count_capacity.check_escape:
    cmp     w9, #0x1b
    b.ne    count_capacity.loop
    mov     w0, #1
    b       count_capacity.loop
count_capacity.tab:
    add     x11, x11, #1
    b       count_capacity.loop
count_capacity.newline:
    add     x4, x4, #1
    b       count_capacity.loop
count_capacity.done:
    LDX     x9, cfg_tab_width
    sub     x9, x9, #1
    mul     x9, x9, x11
    add     x10, x10, x9
    add     x10, x10, #2
    STX     x10, char_capacity
    cbnz    w0, count_capacity.mapped
    // plain: per row, the count of characters written to it
    lsl     x0, x4, #3
    add     x0, x0, #64
    PUSH1   x30
    bl      alloc
    POP1    x30
    STX     x9, plain_rows
count_capacity.mapped:
    LDW     w0, char_count
    LDX     x1, char_capacity
    b       populate_chars

// populate_chars(w0=first slot, x1=count): commit the character arrays'
// pages for these slots with one call per array rather than a page fault
// per page (MADV_POPULATE_WRITE; an old kernel refuses it, and the pages
// fault in as before). Clobbers x0-x5, x8, x9, x11.
.macro INPUT_POPULATE_FIELD name, size, init
    mov     x16, #\size
    mul     x0, x4, x16
    mul     x1, x5, x16
    LDX     x9, \name
    add     x0, x0, x9
    bl      populate
.endm
populate_chars:
    PUSH1   x30
    mov     w4, w0
    mov     x5, x1
    CHAR_FIELDS INPUT_POPULATE_FIELD
    POP1    x30
    ret

// populate(x0=start, x1=bytes): MADV_POPULATE_WRITE over the pages holding
// [start, start + bytes). Clobbers x0-x3, x8, x9, x11; keeps x4, x5.
populate:
    mov     x9, x0
    and     x0, x0, #-4096
    sub     x9, x9, x0
    add     x1, x1, x9
    mov     x2, #MADV_POPULATE_WRITE
    SYSCALL SYS_madvise
    ret

// screen_init: an open-addressing map (row << 32 | column) -> slot, sized
// for every parsed character at half load.
screen_init:
    LDX     x9, plain_rows
    cbnz    x9, screen_init.plain
    LDX     x9, char_capacity
    add     x9, x9, x9
    clz     x3, x9                      // 64 - clz; x9 >= 4 here
    neg     x3, x3
    add     x3, x3, #64
    mov     x9, #1
    lsl     x9, x9, x3                  // next power of two >= 2 * capacity
    sub     x2, x9, #1
    STX     x2, screen_mask
    lsl     x0, x9, #4                  // 16 bytes: key, slot
    PUSH1   x30
    bl      reserve
    POP1    x30
    STX     x9, screen
screen_init.plain:
    ret

// screen_slot(x0=key) -> x9 = pointer to the entry for key (empty or found).
// Clobbers x2, x3, x12.
screen_slot:
    MOV64   x3, 0x9E3779B97F4A7C15
    mul     x9, x0, x3
    lsr     x9, x9, #32
    LDX     x2, screen
    LDX     x12, screen_mask
screen_slot.probe:
    and     x9, x9, x12
    add     x3, x2, x9, lsl #4
    ldr     w16, [x3, #8]
    cbz     w16, screen_slot.found      // empty (slots are stored + 1)
    ldr     x16, [x3]
    cmp     x16, x0
    b.eq    screen_slot.found
    add     x9, x9, #1
    b       screen_slot.probe
screen_slot.found:
    mov     x9, x3
    ret

// preprocess: the emulator loop.
// x19 = input cursor, x20 = input end, x21 = row, x22 = column.
preprocess:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, input_ptr
    LDX     x9, input_len
    add     x20, x19, x9
    mov     x21, #0
    mov     x22, #0
    STX     xzr, max_row
    STX     xzr, max_col
    mov     x9, #NONE
    STX     x9, sgr_fg
    STX     x9, sgr_bg
    STB     wzr, sgr_bold
    STX     x9, sgr_standard
preprocess.loop:
    cmp     x19, x20
    b.hs    preprocess.end
    mov     x1, x19
    bl      utf8_decode
    cmp     w9, #0x1b
    b.eq    preprocess.escape
    cmp     w9, #10
    b.eq    preprocess.newline
    cmp     w9, #13
    b.eq    preprocess.return
    cmp     w9, #9
    b.eq    preprocess.tab
    // ordinary character: its packed UTF-8 bytes
    add     x19, x19, x2
    mov     w0, w9
    bl      utf8_pack
    mov     x0, x9
    bl      put_char
    b       preprocess.loop
preprocess.tab:
    add     x19, x19, #1
    LDX     x3, cfg_tab_width
    udiv    x9, x22, x3
    msub    x2, x9, x3, x22             // column % tab_width
    sub     x23, x3, x2
preprocess.tab_space:
    mov     x0, #' '
    movk    x0, #1, lsl #32
    bl      put_char
    subs    x23, x23, #1
    b.ne    preprocess.tab_space
    b       preprocess.loop
preprocess.return:
    add     x19, x19, #1
    mov     x22, #0
    b       preprocess.loop
preprocess.newline:
    add     x19, x19, #1
    add     x21, x21, #1
    mov     x22, #0
    LDX     x9, max_row
    cmp     x21, x9
    b.ls    preprocess.loop
    STX     x21, max_row
    b       preprocess.loop
preprocess.escape:
    bl      escape_sequence
    b       preprocess.loop
preprocess.end:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// put_char(x0=packed symbol): build_character with the current SGR state at
// (row x21, column x22), then advance. Plain input writes its cells in
// row-major order, each once, so it only counts them per row (build_lines
// lays them out); otherwise the screen map finds the cell. Uses x24.
put_char:
    PUSH2   x0, x30
    LDX     x9, plain_rows
    cbz     x9, put_char.mapped
    ldr     x3, [x9, x21, lsl #3]
    add     x3, x3, #1
    str     x3, [x9, x21, lsl #3]
    bl      build_character
    b       put_char.extent
put_char.mapped:
    lsl     x0, x21, #32
    orr     x0, x0, x22
    bl      screen_slot
    mov     x24, x9
    ldr     x0, [sp]
    bl      build_character             // w9 = slot
    // the cell's previous character, if any, is orphaned
    ldr     w3, [x24, #8]
    cbz     w3, put_char.store
    sub     w3, w3, #1
    LDX     x2, ch_flags
    ldrh    w12, [x2, x3, lsl #1]
    orr     w12, w12, #CF_ORPHAN
    strh    w12, [x2, x3, lsl #1]
put_char.store:
    lsl     x3, x21, #32
    orr     x3, x3, x22
    str     x3, [x24]
    add     w9, w9, #1
    str     w9, [x24, #8]
put_char.extent:
    LDX     x9, max_row
    cmp     x21, x9
    b.ls    put_char.col
    STX     x21, max_row
put_char.col:
    LDX     x9, max_col
    cmp     x22, x9
    b.ls    put_char.advance
    STX     x22, max_col
put_char.advance:
    add     x22, x22, #1
    POP2    x0, x30
    ret

// build_character(x0=packed symbol) -> w9 = slot. Captures the active
// colors (counting their frequency at creation, fg first), bold, and under
// --existing-color-handling always shows the input colors at once.
build_character:
    PUSH2   x19, x30
    mov     w1, #0
    mov     w2, #0
    bl      new_char
    mov     w19, w9
    LDX     x3, ch_flags
    ldrh    w2, [x3, x19, lsl #1]
    orr     w2, w2, #CF_PREEXISTING
    LDB     w9, sgr_bold
    cbz     w9, build_character.flags
    orr     w2, w2, #CF_BOLD
build_character.flags:
    strh    w2, [x3, x19, lsl #1]
build_character.fg:
    LDX     x0, sgr_fg
    cmn     x0, #1                      // NONE
    b.eq    build_character.bg
    LDX     x9, ch_fg
    str     x0, [x9, x19, lsl #3]
    bl      count_input_color
build_character.bg:
    LDX     x0, sgr_bg
    cmn     x0, #1
    b.eq    build_character.always
    LDX     x9, ch_bg
    str     x0, [x9, x19, lsl #3]
    bl      count_input_color
build_character.always:
    LDX     x9, cfg_existing_colors
    cbnz    x9, build_character.done
    mov     w0, w19
    bl      reset_appearance            // set_appearance(input symbol, uses=true)
build_character.done:
    mov     w9, w19
    POP2    x19, x30
    ret

// count_input_color(x0=color): _input_colors_frequency, insertion ordered.
// Colors are equal by their constructor argument; for input colors that is
// exactly the u64 value (xterm codes carry their code, 24-bit ones none).
// Clobbers x2-x4, x9.
count_input_color:
    LDX     x9, color_freq
    cbnz    x9, count_input_color.search
    PUSH2   x0, x30
    mov     x0, #(4096 * 16)
    bl      alloc
    STX     x9, color_freq
    POP2    x0, x30
count_input_color.search:
    LDX     x3, color_freq_count
    mov     x2, #0
count_input_color.next:
    cmp     x2, x3
    b.hs    count_input_color.new
    add     x4, x9, x2, lsl #4
    ldr     x16, [x4]
    cmp     x16, x0
    b.eq    count_input_color.found
    add     x2, x2, #1
    b       count_input_color.next
count_input_color.found:
    ldr     x2, [x4, #8]
    add     x2, x2, #1
    str     x2, [x4, #8]
    ret
count_input_color.new:
    cmp     x3, #4096
    b.hs    count_input_color.full
    add     x4, x9, x3, lsl #4
    mov     x2, #1
    stp     x0, x2, [x4]
    add     x3, x3, #1
    STX     x3, color_freq_count
    ret
count_input_color.full:
    ADRG    x0, msg_too_many_colors
    mov     x1, #msg_too_many_colors_len
    b       fatal

// ------------------------------------------------------------ escapes

// escape_sequence: x19 at an ESC. match_escape_sequence's alternation (OSC,
// CSI, then ESC + any character but newline), then SGR, the four supported
// private modes, or cursor movement. Advances x19 past the sequence.
escape_sequence:
    add     x1, x19, #1
    cmp     x1, x20
    b.hs    escape_sequence.lone_escape
    ldrb    w12, [x1]
    cmp     w12, #']'
    b.ne    escape_sequence.csi
    // OSC: the first BEL after "ESC ]" ends it ...
    add     x3, x19, #2
escape_sequence.bel:
    cmp     x3, x20
    b.hs    escape_sequence.no_bel
    ldrb    w12, [x3]
    cmp     w12, #7
    b.eq    escape_sequence.osc_bel
    add     x3, x3, #1
    b       escape_sequence.bel
escape_sequence.osc_bel:
    add     x2, x3, #1
    b       unsupported_sequence
escape_sequence.no_bel:
    // ... else the rightmost "ESC \" at or after the run start
    mov     x3, x20
escape_sequence.st:
    add     x9, x19, #4
    cmp     x3, x9
    b.lo    escape_sequence.csi
    ldurb   w12, [x3, #-2]
    cmp     w12, #0x1b
    b.ne    escape_sequence.st_next
    ldurb   w12, [x3, #-1]
    cmp     w12, #'\\'
    b.ne    escape_sequence.st_next
    mov     x2, x3
    b       unsupported_sequence
escape_sequence.st_next:
    sub     x3, x3, #1
    b       escape_sequence.st
escape_sequence.csi:
    ldrb    w12, [x1]
    cmp     w12, #'['
    b.ne    escape_sequence.any
    add     x3, x19, #2
escape_sequence.params:
    cmp     x3, x20
    b.hs    escape_sequence.any
    ldrb    w9, [x3]
    sub     w9, w9, #0x30
    cmp     w9, #0x0f
    b.hi    escape_sequence.inter
    add     x3, x3, #1
    b       escape_sequence.params
escape_sequence.inter:
    mov     x4, x3                      // end of the parameters
escape_sequence.inter_next:
    cmp     x3, x20
    b.hs    escape_sequence.any
    ldrb    w9, [x3]
    sub     w9, w9, #0x20
    cmp     w9, #0x0f
    b.hi    escape_sequence.final
    add     x3, x3, #1
    b       escape_sequence.inter_next
escape_sequence.final:
    ldrb    w9, [x3]
    sub     w9, w9, #0x40
    cmp     w9, #0x3e
    b.hi    escape_sequence.any
    add     x2, x3, #1                  // end of the sequence
    b       csi_sequence
escape_sequence.any:
    // ESC and any one character but a newline
    ldrb    w12, [x1]
    cmp     w12, #10
    b.eq    escape_sequence.lone_escape
    PUSH2   x1, x30
    bl      utf8_decode
    POP2    x1, x30
    add     x2, x1, x2
    // "ESC [" alone is not a well-formed CSI sequence either
    b       unsupported_sequence
escape_sequence.lone_escape:
    add     x2, x19, #1
    b       unsupported_sequence

// unsupported_sequence(x19=start, x2=end): UnsupportedAnsiSequence(bytes).
unsupported_sequence:
    LDX     x9, request
    str     x19, [x9, #RQ_ERROR_PTR]
    sub     x2, x2, x19
    str     x2, [x9, #RQ_ERROR_LEN]
    mov     x3, #ERR_ANSI
    str     x3, [x9, #RQ_ERROR_KIND]
    LDX     x9, fail_rsp
    mov     sp, x9
    mov     w9, #OUT_ERROR
    b       run_return

// csi_sequence(x19=ESC, x4=end of parameters, x3=final byte, x2=end).
// Its frame: [sp] = x4, [sp+8] = x3, [sp+16] = x2 (apply_sgr reads it),
// [sp+24] = x30.
.equ CSI_FRAME, 32
.equ CSI_END, 16
csi_sequence:
    stp     x4, x3, [sp, #-CSI_FRAME]!
    stp     x2, x30, [sp, #16]
    ldrb    w9, [x3]
    cmp     w9, #'m'
    b.eq    csi_sequence.sgr
    // the four supported private modes (cursor show/hide, autowrap on/off)
    // are ignored: exactly ESC [ ? 2 5 h|l or ESC [ ? 7 h|l
    sub     x9, x2, x19
    cmp     x9, #6
    b.eq    csi_sequence.private25
    cmp     x9, #5
    b.ne    csi_sequence.cursor
    ldr     w9, [x19]
    MOV64   x12, 0x373f5b1b             // ESC [ ? 7
    cmp     w9, w12
    b.ne    csi_sequence.cursor
    ldrb    w9, [x19, #4]
    b       csi_sequence.private_final
csi_sequence.private25:
    ldr     w9, [x19]
    MOV64   x12, 0x323f5b1b             // ESC [ ? 2
    cmp     w9, w12
    b.ne    csi_sequence.cursor
    ldrb    w9, [x19, #4]
    cmp     w9, #'5'
    b.ne    csi_sequence.cursor
    ldrb    w9, [x19, #5]
csi_sequence.private_final:
    cmp     w9, #'h'
    b.eq    csi_sequence.ignored
    cmp     w9, #'l'
    b.eq    csi_sequence.ignored
    b       csi_sequence.cursor
csi_sequence.ignored:
    ldp     x19, x30, [sp, #16]         // continue after the sequence
    ldp     x4, x3, [sp], #CSI_FRAME
    ret
csi_sequence.sgr:
    bl      parse_parameters            // errors on anything but digits/;
    bl      apply_sgr
    ldp     x19, x30, [sp, #16]
    ldp     x4, x3, [sp], #CSI_FRAME
    ret
csi_sequence.cursor:
    // intermediates or a private marker make it unsupported
    ldp     x4, x3, [sp]
    cmp     x4, x3
    b.ne    csi_sequence.unsupported
    ldrb    w9, [x19, #2]
    cmp     w9, #'?'
    b.eq    csi_sequence.unsupported
    bl      parse_parameters
    ldr     x3, [sp, #8]
    ldrb    w9, [x3]
    bl      apply_cursor
    b.cs    csi_sequence.unsupported
    ldp     x19, x30, [sp, #16]
    ldp     x4, x3, [sp], #CSI_FRAME
    ret
csi_sequence.unsupported:
    ldr     x2, [sp, #CSI_END]
    b       unsupported_sequence

// parse_parameters(x19=ESC, x4=end of parameters) -> params[]/param_count.
// parse_csi_parameters: only digits and ';' (else UnsupportedAnsiSequence
// of "ESC [ <parameters>"); empty fields are 0. Clobbers x1-x3, x9, x12.
parse_parameters:
    PUSH1   x30
    add     x1, x19, #2
    STX     xzr, param_count
    cmp     x1, x4
    b.eq    parse_parameters.done
    mov     x9, #0                      // current value
parse_parameters.char:
    cmp     x1, x4
    b.hs    parse_parameters.last
    ldrb    w3, [x1]
    cmp     w3, #';'
    b.eq    parse_parameters.field
    sub     w3, w3, #'0'
    cmp     w3, #9
    b.hi    parse_parameters.bad
    mov     x12, #10
    mul     x9, x9, x12
    add     x9, x9, x3
    add     x1, x1, #1
    b       parse_parameters.char
parse_parameters.field:
    bl      parse_parameters.push
    mov     x9, #0
    add     x1, x1, #1
    b       parse_parameters.char
parse_parameters.last:
    bl      parse_parameters.push
parse_parameters.done:
    POP1    x30
    ret
parse_parameters.push:
    LDX     x3, param_count
    cmp     x3, #256
    b.hs    parse_parameters.too_many
    ADRG    x2, params
    str     x9, [x2, x3, lsl #3]
    add     x3, x3, #1
    STX     x3, param_count
    ret
parse_parameters.too_many:
    ADRG    x0, msg_too_many_params
    mov     x1, #msg_too_many_params_len
    b       fatal
parse_parameters.bad:
    mov     x2, x4                      // reporting "ESC [ <parameters>"
    b       unsupported_sequence

// apply_sgr(x19=ESC, csi_sequence's frame above) -
// Preprocessor.apply_sgr_sequence.
.equ SGR_FRAME, 32
apply_sgr:
    stp     x21, x22, [sp, #-SGR_FRAME]!
    str     x30, [sp, #16]
    LDX     x9, param_count
    cbnz    x9, apply_sgr.loop_start
    STX     xzr, params
    mov     x9, #1
    STX     x9, param_count
apply_sgr.loop_start:
    mov     x21, #0                     // idx
apply_sgr.next:
    LDX     x9, param_count
    cmp     x21, x9
    b.hs    apply_sgr.done
    ADRG    x9, params
    ldr     x22, [x9, x21, lsl #3]
    cbz     x22, apply_sgr.reset
    cmp     x22, #1
    b.eq    apply_sgr.bold
    cmp     x22, #22
    b.eq    apply_sgr.unbold
    cmp     x22, #39
    b.eq    apply_sgr.fg_reset
    cmp     x22, #49
    b.eq    apply_sgr.bg_reset
    sub     x9, x22, #30
    cmp     x9, #7
    b.ls    apply_sgr.fg_standard
    sub     x9, x22, #90
    cmp     x9, #7
    b.ls    apply_sgr.fg_bright
    sub     x9, x22, #40
    cmp     x9, #7
    b.ls    apply_sgr.bg_standard
    sub     x9, x22, #100
    cmp     x9, #7
    b.ls    apply_sgr.bg_bright
    cmp     x22, #38
    b.eq    apply_sgr.extended
    cmp     x22, #48
    b.eq    apply_sgr.extended
    b       apply_sgr.advance           // anything else is silently ignored
apply_sgr.reset:
    mov     x9, #NONE
    STX     x9, sgr_fg
    STX     x9, sgr_bg
    STB     wzr, sgr_bold
    STX     x9, sgr_standard
    b       apply_sgr.advance
apply_sgr.bold:
    mov     w9, #1
    STB     w9, sgr_bold
    LDX     x9, sgr_standard
    cmn     x9, #1                      // NONE
    b.eq    apply_sgr.advance
    sub     x9, x9, #(30 - 8)
    bl      xterm_input_color
    STX     x9, sgr_fg
    b       apply_sgr.advance
apply_sgr.unbold:
    STB     wzr, sgr_bold
    LDX     x9, sgr_standard
    cmn     x9, #1
    b.eq    apply_sgr.advance
    sub     x9, x9, #30
    bl      xterm_input_color
    STX     x9, sgr_fg
    b       apply_sgr.advance
apply_sgr.fg_reset:
    mov     x9, #NONE
    STX     x9, sgr_fg
    STX     x9, sgr_standard
    b       apply_sgr.advance
apply_sgr.bg_reset:
    mov     x9, #NONE
    STX     x9, sgr_bg
    b       apply_sgr.advance
apply_sgr.fg_standard:
    LDB     w3, sgr_bold
    cbz     w3, apply_sgr.fg_plain
    add     x9, x9, #8
apply_sgr.fg_plain:
    bl      xterm_input_color
    STX     x9, sgr_fg
    STX     x22, sgr_standard
    b       apply_sgr.advance
apply_sgr.fg_bright:
    add     x9, x9, #8
    bl      xterm_input_color
    STX     x9, sgr_fg
    mov     x9, #NONE
    STX     x9, sgr_standard
    b       apply_sgr.advance
apply_sgr.bg_standard:
    bl      xterm_input_color
    STX     x9, sgr_bg
    b       apply_sgr.advance
apply_sgr.bg_bright:
    add     x9, x9, #8
    bl      xterm_input_color
    STX     x9, sgr_bg
    b       apply_sgr.advance
apply_sgr.extended:
    // 38/48 ; 5 ; code  or  38/48 ; 2 ; r ; g ; b
    LDX     x2, param_count
    add     x9, x21, #1
    cmp     x9, x2
    b.hs    apply_sgr.unsupported
    ADRG    x3, params
    add     x3, x3, x21, lsl #3         // &params[idx]
    ldr     x9, [x3, #8]
    cmp     x9, #5
    b.eq    apply_sgr.indexed
    cmp     x9, #2
    b.eq    apply_sgr.truecolor
    b       apply_sgr.unsupported
apply_sgr.indexed:
    add     x9, x21, #2
    cmp     x9, x2
    b.hs    apply_sgr.unsupported
    ldr     x9, [x3, #16]
    bl      xterm_input_color
    add     x21, x21, #2
    b       apply_sgr.store_extended
apply_sgr.truecolor:
    add     x9, x21, #4
    cmp     x9, x2
    b.hs    apply_sgr.unsupported
    add     x0, x3, #16
    bl      hex_input_color
    add     x21, x21, #4
apply_sgr.store_extended:
    cmp     x22, #38
    b.ne    apply_sgr.store_bg
    STX     x9, sgr_fg
    mov     x9, #NONE
    STX     x9, sgr_standard
    b       apply_sgr.advance
apply_sgr.store_bg:
    STX     x9, sgr_bg
apply_sgr.advance:
    add     x21, x21, #1
    b       apply_sgr.next
apply_sgr.done:
    ldr     x30, [sp, #16]
    ldp     x21, x22, [sp], #SGR_FRAME
    ret
apply_sgr.unsupported:
    // UnsupportedAnsiSequence(the whole sequence): its end is csi_sequence's
    // saved x2, above our frame
    ldr     x2, [sp, #(SGR_FRAME + CSI_END)]
    b       unsupported_sequence

// xterm_input_color(x9=code) -> x9 = Color::from_xterm(code); codes outside
// 0..=255 are an error ("invalid xterm color code in input: N").
// Clobbers x2, x3.
xterm_input_color:
    cmp     x9, #255
    b.hi    xterm_input_color.bad
    mov     x3, x9
    ADRG    x2, xterm_rgb
    ldr     w9, [x2, x3, lsl #2]
    orr     x9, x9, x3, lsl #32
    orr     x9, x9, #(1 << COLOR_XTERM_BIT)
    ret
xterm_input_color.bad:
    mov     x1, x9
    ADRG    x0, msg_bad_xterm_code
    mov     w2, #msg_bad_xterm_code_len
    b       fail_with_number

// hex_input_color(x0=three params) -> x9. The 24-bit path formats each
// channel as {:02X} and parses the result with Color::from_hex, faithfully:
// channels over 255 widen the hex string; 7 digits still parse (first six
// used), anything else is an invalid color. Clobbers x2-x5, x10, x11.
hex_input_color:
    mov     w9, #0                      // digits so far
    mov     x2, #0                      // first six digits as a number
    mov     w5, #0                      // channel
hex_input_color.channel:
    cmp     w5, #3
    b.hs    hex_input_color.check
    ldr     x10, [x0, x5, lsl #3]
    // digits of the channel in hex, at least two
    mov     x3, x10
    mov     w11, #1
hex_input_color.width:
    lsr     x3, x3, #4
    cbz     x3, hex_input_color.widthed
    add     w11, w11, #1
    b       hex_input_color.width
hex_input_color.widthed:
    cmp     w11, #2
    b.hs    hex_input_color.emit
    mov     w11, #2
hex_input_color.emit:
    // append x11 digits of x10, most significant first
    lsl     w3, w11, #2
    sub     w3, w3, #4
hex_input_color.digit:
    lsr     x4, x10, x3
    and     x4, x4, #15
    cmp     w9, #6
    b.hs    hex_input_color.skip
    orr     x2, x4, x2, lsl #4
hex_input_color.skip:
    add     w9, w9, #1
    subs    w3, w3, #4
    b.pl    hex_input_color.digit
    add     w5, w5, #1
    b       hex_input_color.channel
hex_input_color.check:
    cmp     w9, #6
    b.eq    hex_input_color.ok
    cmp     w9, #7
    b.eq    hex_input_color.ok
    FAIL    msg_invalid_color
hex_input_color.ok:
    mov     x9, x2
    ret

// apply_cursor(w9=final byte) -> C set when the final is unsupported.
// apply_cursor_sequence, clamping the cursor at 0 and extending max_row/col.
// Clobbers x3, x9, x12.
apply_cursor:
    // default_parameter: the first parameter or 1, at least 1
    mov     x3, #1
    LDX     x12, param_count
    cbz     x12, apply_cursor.default
    LDX     x3, params
    cmp     x3, #1
    b.ge    apply_cursor.default
    mov     x3, #1
apply_cursor.default:
    cmp     w9, #'A'
    b.eq    apply_cursor.up
    cmp     w9, #'B'
    b.eq    apply_cursor.down
    cmp     w9, #'C'
    b.eq    apply_cursor.right
    cmp     w9, #'D'
    b.eq    apply_cursor.left
    cmp     w9, #'E'
    b.eq    apply_cursor.next_line
    cmp     w9, #'F'
    b.eq    apply_cursor.previous_line
    cmp     w9, #'G'
    b.eq    apply_cursor.column
    cmp     w9, #'H'
    b.eq    apply_cursor.position
    cmp     w9, #'f'
    b.eq    apply_cursor.position
    cmp     xzr, xzr                    // C set: unsupported
    ret
apply_cursor.up:
    sub     x21, x21, x3
    b       apply_cursor.clamp
apply_cursor.down:
    add     x21, x21, x3
    b       apply_cursor.clamp
apply_cursor.right:
    add     x22, x22, x3
    b       apply_cursor.clamp
apply_cursor.left:
    sub     x22, x22, x3
    b       apply_cursor.clamp
apply_cursor.next_line:
    add     x21, x21, x3
    mov     x22, #0
    b       apply_cursor.clamp
apply_cursor.previous_line:
    sub     x21, x21, x3
    mov     x22, #0
    b       apply_cursor.clamp
apply_cursor.column:
    sub     x22, x3, #1
    b       apply_cursor.clamp
apply_cursor.position:
    sub     x21, x3, #1
    mov     x22, #0
    LDX     x9, param_count
    cmp     x9, #2
    b.lo    apply_cursor.clamp
    ADRG    x9, params
    ldr     x9, [x9, #8]
    cbz     x9, apply_cursor.clamp
    sub     x22, x9, #1
apply_cursor.clamp:
    cmp     x21, #0
    csel    x21, xzr, x21, lt
    cmp     x22, #0
    csel    x22, xzr, x22, lt
    LDX     x9, max_row
    cmp     x21, x9
    b.ls    apply_cursor.max_col
    STX     x21, max_row
apply_cursor.max_col:
    LDX     x9, max_col
    cmp     x22, x9
    b.ls    apply_cursor.ok
    STX     x22, max_col
apply_cursor.ok:
    cmn     xzr, xzr                    // C clear
    ret

// ------------------------------------------------------------ lines

// build_lines: every screen cell of (max_row + 1) x (max_col + 1), row-major:
// the character written there, or padding (an id, but no slot).
build_lines:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    str     x30, [sp, #32]
    LDX     x19, max_row
    add     x19, x19, #1
    lsl     x0, x19, #3
    add     x0, x0, #64
    bl      alloc
    STX     x9, row_start
    lsl     x0, x19, #3
    add     x0, x0, #64
    bl      alloc
    STX     x9, row_len
    lsl     x0, x19, #3
    add     x0, x0, #64
    bl      alloc
    STX     x9, line_len
    LDX     x23, max_col
    add     x23, x23, #1                // row width
    mul     x0, x19, x23
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      reserve
    STX     x9, cells
    LDX     x9, plain_rows
    cbnz    x9, build_lines.plain
    mov     x21, #0                     // row
build_lines.row:
    cmp     x21, x19
    b.hs    build_lines.done
    mul     x9, x21, x23
    LDX     x3, row_start
    str     x9, [x3, x21, lsl #3]
    LDX     x3, row_len
    str     x23, [x3, x21, lsl #3]
    mov     x22, #0                     // column
build_lines.column:
    cmp     x22, x23
    b.hs    build_lines.next_row
    lsl     x0, x21, #32
    orr     x0, x0, x22
    bl      screen_slot
    ldr     w3, [x9, #8]
    sub     w3, w3, #1                  // NONE for padding
    cmn     w3, #1
    b.ne    build_lines.cell
    LDW     w9, next_character_id       // padding consumes an id
    add     w9, w9, #1
    STW     w9, next_character_id
build_lines.cell:
    mul     x9, x21, x23
    add     x9, x9, x22
    LDX     x2, cells
    str     w3, [x2, x9, lsl #2]
    add     x22, x22, #1
    b       build_lines.column
build_lines.next_row:
    add     x21, x21, #1
    b       build_lines.row
build_lines.plain:
    // each row's characters in slot order from its first column, then
    // padding to the width
    LDX     x0, cells
    mov     w3, #0                      // next slot (input characters come first)
    mov     x21, #0
    mov     w12, #NONE
build_lines.plain_row:
    cmp     x21, x19
    b.hs    build_lines.done
    mul     x9, x21, x23
    LDX     x2, row_start
    str     x9, [x2, x21, lsl #3]
    LDX     x2, row_len
    str     x23, [x2, x21, lsl #3]
    LDX     x2, plain_rows
    ldr     x4, [x2, x21, lsl #3]       // characters in the row
    sub     x5, x23, x4                 // padding
    LDW     w2, next_character_id
    add     w2, w2, w5
    STW     w2, next_character_id
build_lines.plain_char:
    cbz     x4, build_lines.plain_pad
    str     w3, [x0], #4
    add     w3, w3, #1
    sub     x4, x4, #1
    b       build_lines.plain_char
build_lines.plain_pad:
    cbz     x5, build_lines.plain_next
    str     w12, [x0], #4
    sub     x5, x5, #1
    b       build_lines.plain_pad
build_lines.plain_next:
    add     x21, x21, #1
    b       build_lines.plain_row
build_lines.done:
    ldr     x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// is_plain_space(w3=slot or NONE) -> Z set for padding or an uncolored " ".
// Clobbers x2, x9; keeps x3.
is_plain_space:
    cmn     w3, #1                      // NONE
    b.eq    is_plain_space.yes
    LDX     x9, ch_sym
    ldr     x9, [x9, x3, lsl #3]
    mov     x2, #' '
    movk    x2, #1, lsl #32
    cmp     x9, x2
    b.ne    is_plain_space.no
    LDX     x9, ch_fg
    ldr     x9, [x9, x3, lsl #3]
    cmn     x9, #1
    b.ne    is_plain_space.no
    LDX     x9, ch_bg
    ldr     x9, [x9, x3, lsl #3]
    cmn     x9, #1
is_plain_space.no:
    ret
is_plain_space.yes:
    cmp     w3, w3
    ret

// finish_lines: trim trailing plain spaces and trailing empty lines, assign
// bottom-up 1-based input coordinates, and collect the input characters
// (anything but a plain space). With nothing left, the fallback character
// carries the end-of-input SGR state, faithfully.
finish_lines:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    str     x30, [sp, #32]
    LDX     x22, max_row
    add     x22, x22, #1                // rows
    mov     x19, #0
    mov     x21, #0                     // last non-empty row + 1
finish_lines.rows:
    cmp     x19, x22
    b.hs    finish_lines.rows_done
    LDX     x9, row_len
    ldr     x23, [x9, x19, lsl #3]
    LDX     x9, row_start
    ldr     x4, [x9, x19, lsl #3]
finish_lines.trim:
    cbz     x23, finish_lines.trimmed
    add     x9, x4, x23
    sub     x9, x9, #1
    LDX     x2, cells
    ldr     w3, [x2, x9, lsl #2]
    bl      is_plain_space              // keeps x4
    b.ne    finish_lines.trimmed
    sub     x23, x23, #1
    b       finish_lines.trim
finish_lines.trimmed:
    LDX     x9, line_len
    str     x23, [x9, x19, lsl #3]
    cbz     x23, finish_lines.next_row
    add     x21, x19, #1
finish_lines.next_row:
    add     x19, x19, #1
    b       finish_lines.rows
finish_lines.rows_done:
    cbnz    x21, finish_lines.have_lines
    // no lines: one space with the end-of-input state
    mov     x0, #' '
    movk    x0, #1, lsl #32
    bl      build_character
    LDX     x2, cells
    str     w9, [x2]
    LDX     x9, row_start
    str     xzr, [x9]
    LDX     x9, line_len
    mov     x2, #1
    str     x2, [x9]
    mov     x21, #1
finish_lines.have_lines:
    STX     x21, line_count
    LDX     x9, request
    str     x21, [x9, #RQ_LINE_COUNT]
    LDX     x3, line_len
    str     x3, [x9, #RQ_LINE_LENGTHS]
    ldr     x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// wrapped_line_count(x0=width) -> x9: formatted rows after wrapping every
// line at width (wrapped_line_count / wrap_lines): a line longer than the
// width splits into width-sized pieces; an empty line stays one row.
// Clobbers x2, x3, x12.
wrapped_line_count:
    mov     x9, #0
    mov     x3, #0
    LDX     x12, line_count
wrapped_line_count.line:
    cmp     x3, x12
    b.hs    wrapped_line_count.done
    LDX     x2, line_len
    ldr     x2, [x2, x3, lsl #3]
wrapped_line_count.split:
    cmp     x2, x0
    b.le    wrapped_line_count.last
    add     x9, x9, #1
    sub     x2, x2, x0
    b       wrapped_line_count.split
wrapped_line_count.last:
    add     x9, x9, #1
    add     x3, x3, #1
    b       wrapped_line_count.line
wrapped_line_count.done:
    ret

// assign_coordinates: Terminal._setup_input_characters - wrap the lines at
// the canvas width under --wrap-text, give every character of the formatted
// lines a bottom-up 1-based input coordinate, and collect the input
// characters (anything but a plain space), top row first.
assign_coordinates:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    // the formatted height
    LDX     x24, line_count
    LDB     w9, cfg_wrap_text
    cbz     w9, assign_coordinates.height
    LDX     x0, canvas_right
    bl      wrapped_line_count
    mov     x24, x9
assign_coordinates.height:
    LDW     w0, char_count
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, input_chars
    mov     x23, #0                     // input count
    mov     x21, #0                     // formatted row index
    mov     x19, #0                     // line
assign_coordinates.line:
    LDX     x9, line_count
    cmp     x19, x9
    b.hs    assign_coordinates.collected
    LDX     x9, row_start
    ldr     x20, [x9, x19, lsl #3]      // first cell of the line
    LDX     x9, line_len
    ldr     x22, [x9, x19, lsl #3]      // cells left in the line
assign_coordinates.piece:
    // one formatted row: the whole rest, or a width-sized piece of it
    mov     x5, x22
    LDB     w9, cfg_wrap_text
    cbz     w9, assign_coordinates.row
    LDX     x9, canvas_right
    cmp     x22, x9
    b.le    assign_coordinates.row
    mov     x5, x9
assign_coordinates.row:
    mov     x1, #0
assign_coordinates.cell:
    cmp     x1, x5
    b.hs    assign_coordinates.row_done
    add     x9, x20, x1
    LDX     x2, cells
    ldr     w3, [x2, x9, lsl #2]
    bl      is_plain_space              // keeps x1, x3, x5
    b.eq    assign_coordinates.skip
    add     w0, w1, #1
    LDX     x9, ch_col
    str     w0, [x9, x3, lsl #2]
    LDX     x9, ch_icol
    str     w0, [x9, x3, lsl #2]
    sub     w0, w24, w21
    LDX     x9, ch_row
    str     w0, [x9, x3, lsl #2]
    LDX     x9, ch_irow
    str     w0, [x9, x3, lsl #2]
    LDX     x9, input_chars
    str     w3, [x9, x23, lsl #2]
    add     x23, x23, #1
assign_coordinates.skip:
    add     x1, x1, #1
    b       assign_coordinates.cell
assign_coordinates.row_done:
    add     x21, x21, #1
    add     x20, x20, x5
    subs    x22, x22, x5
    b.ne    assign_coordinates.piece    // more of this line to wrap
    add     x19, x19, #1
    b       assign_coordinates.line
assign_coordinates.collected:
    STX     x23, input_count
    // preexisting_colors_present: any input character with a color
    mov     x19, #0
assign_coordinates.present:
    cmp     x19, x23
    b.hs    assign_coordinates.done
    LDX     x9, input_chars
    ldr     w3, [x9, x19, lsl #2]
    LDX     x9, ch_fg
    ldr     x9, [x9, x3, lsl #3]
    cmn     x9, #1
    b.ne    assign_coordinates.colored
    LDX     x9, ch_bg
    ldr     x9, [x9, x3, lsl #3]
    cmn     x9, #1
    b.ne    assign_coordinates.colored
    add     x19, x19, #1
    b       assign_coordinates.present
assign_coordinates.colored:
    mov     w9, #1
    STB     w9, preexisting_colors_present
assign_coordinates.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

error_no_input_chars:
    FAIL    msg_no_input_chars

// get_input_colors(w0=COLOR_SORT_*) -> x9 = u64 colors, x2 = count.
// Terminal.get_input_colors: most/least frequent first (stable, so ties keep
// insertion order), or shuffled with the engine RNG.
get_input_colors:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x30, [sp, #16]
    mov     w19, w0
    LDX     x21, color_freq_count
    // (count key, color) pairs; most-to-least sorts by the negated count
    lsl     x0, x21, #4
    add     x0, x0, #64
    bl      alloc
    mov     x22, x9
    mov     x3, #0
    LDX     x4, color_freq
get_input_colors.pair:
    cmp     x3, x21
    b.hs    get_input_colors.sort
    add     x2, x4, x3, lsl #4
    ldr     x9, [x2, #8]                // count
    cmp     w19, #COLOR_SORT_MOST_TO_LEAST
    b.ne    get_input_colors.key
    neg     x9, x9
get_input_colors.key:
    eor     x9, x9, #0x8000000000000000 // + 2^63
    ldr     x2, [x2]
    add     x12, x22, x3, lsl #4
    stp     x9, x2, [x12]
    add     x3, x3, #1
    b       get_input_colors.pair
get_input_colors.sort:
    cmp     w19, #COLOR_SORT_RANDOM
    b.eq    get_input_colors.colors
    mov     x0, x22
    mov     x1, x21
    bl      sort_pairs
get_input_colors.colors:
    lsl     x0, x21, #3
    add     x0, x0, #64
    bl      alloc
    mov     x3, #0
get_input_colors.copy:
    cmp     x3, x21
    b.hs    get_input_colors.shuffle
    add     x2, x22, x3, lsl #4
    ldr     x2, [x2, #8]
    str     x2, [x9, x3, lsl #3]
    add     x3, x3, #1
    b       get_input_colors.copy
get_input_colors.shuffle:
    cmp     w19, #COLOR_SORT_RANDOM
    b.ne    get_input_colors.done
    str     x9, [sp, #32]
    mov     x0, x9
    mov     x1, x21
    bl      rng_shuffle64
    ldr     x9, [sp, #32]
get_input_colors.done:
    mov     x2, x21
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

    .section .rodata
STRING msg_no_input_chars, "no input characters to anchor"
STRING msg_bad_xterm_code, "invalid xterm color code in input: "
STRING msg_invalid_color, "Invalid color value. Color must be an XTerm-256 color code or an RGB hex color string. Example: 255 or 'ffffff' or '#ffffff'"
STRING msg_too_many_colors, "ttfx: asm engine: too many distinct input colors\n"
STRING msg_too_many_params, "ttfx: asm engine: too many SGR parameters\n"

    TSTATE
    .balign 8
char_capacity:      .skip 8
cells:              .skip 8
row_start:          .skip 8
row_len:            .skip 8
line_len:           .skip 8
line_count:         .skip 8
input_chars:        .skip 8
input_count:        .skip 8
max_row:            .skip 8
max_col:            .skip 8
screen:             .skip 8
screen_mask:        .skip 8
plain_rows:         .skip 8         // plain input: characters per row
sgr_fg:             .skip 8
sgr_bg:             .skip 8
sgr_standard:       .skip 8
color_freq:         .skip 8
color_freq_count:   .skip 8
param_count:        .skip 8
params:             .skip 8 * 256
sgr_bold:           .skip 1
preexisting_colors_present: .skip 1
