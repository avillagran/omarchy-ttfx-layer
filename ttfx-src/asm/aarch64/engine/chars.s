// engine/chars.s - the character store (EffectCharacter, engine/character.rs).
//
// Characters live in struct-of-arrays form, indexed by slot. Slots are
// allocated in the order Rust pushes EffectCharacters to its arena, so
// ascending slot order is ascending character_id order - the canonical order
// the parity rules demand. Every array is a reserved region sized for
// CHAR_LIMIT characters and committed lazily, so particles and added
// characters can keep growing the store without moving anything.
//
// Field arrays (pointer globals; index by slot):
//   ch_sym      u64  input symbol, packed (utf8_pack format)
//   ch_row/col  i32  current coordinate (Motion.current_coord)
//   ch_irow/icol i32 input coordinate
//   ch_id       u32  character_id (Python-compatible allocation id)
//   ch_layer    i32
//   ch_handle   u32  current visual (see visual.s; SET_HANDLE to change it)
//   ch_scene    i32  active scene index or NONE
//   ch_scenes   i32  first scene of the character's scene map or NONE
//   ch_path     i32  active path index or NONE
//   ch_done_path i32 completed path index or NONE (Motion.completed_path)
//   ch_paths    i32  first path of the character's path map or NONE
//   ch_events   i32  first event entry or NONE
//   ch_subs     u8   bitmask of subscribed event kinds (1 << EV_*)
//   ch_flags    u16  CF_* bits
//   ch_fg/bg    u64  input colors (NONE when absent)
//   ch_nbr      4 x i32 neighbors: north, east, south, west (NONE at edges)
//   ch_cell     i32  render cell (render.s)
//   ch_user0/1  u64  two words per character for the effect's own use
//
// CHAR_LIMIT, the CF_* bits, the NBR_* offsets and MADV_NOHUGEPAGE are in
// defs.inc; CHAR_FIELDS is in ttfx.inc.

    .text

// chars_init: reserve every field array. (Staggering them within a page
// against 4K aliasing measured neutral overall and cost matrix 10%.)
.macro CHARS_RESERVE_FIELD name, size, init
    MOV64   x0, (CHAR_LIMIT * \size)
    bl      reserve_small
    STX     x9, \name
.endm
chars_init:
    PUSH1   x30
    CHAR_FIELDS CHARS_RESERVE_FIELD
    MOV64   x0, (CHAR_LIMIT * 4)
    bl      reserve_small
    STX     x9, added_chars
    POP1    x30
    b       update_init

// reserve_small(x0=bytes) -> x9: reserve for a region that usually stays
// small. Transparent huge pages would commit (and zero) 2 MB at its first
// touch, a few kilobytes used; for the dozens of such regions a run
// touches that is milliseconds of kernel time, more than all the work of
// the short effects. So these regions keep 4 KB pages.
reserve_small:
    PUSH2   x0, x30
    bl      reserve
    ldr     x1, [sp]                    // bytes
    str     x9, [sp]                    // the base, returned
    // reserve staggers the base within its first pages; madvise wants the
    // page it starts in
    and     x0, x9, #-4096
    sub     x9, x9, x0
    add     x1, x1, x9
    mov     x2, #MADV_NOHUGEPAGE
    SYSCALL SYS_madvise
    POP2    x9, x30
    ret

// new_char(x0=packed symbol, w1=column, w2=row) -> w9 = slot.
// EffectCharacter::new: the next character_id, the coordinate as both input
// and current coordinate, and the plain visual of the symbol as the current
// visual (Animation::new). Clobbers x0-x5, x10, x11, scratch and vectors.
new_char:
    PUSH2   x19, x30
    PUSH2   x21, x22
    mov     x21, x0
    mov     w22, w1
    LDW     w19, char_count
    lsr     w9, w19, #26                // >= CHAR_LIMIT
    cbnz    w9, new_char.full
    add     w9, w19, #1
    STW     w9, char_count
    // generic sentinels, then the 16-byte neighbor record
    mov     x12, #NONE
    LDX     x9, ch_scene
    str     w12, [x9, x19, lsl #2]
    LDX     x9, ch_scenes
    str     w12, [x9, x19, lsl #2]
    LDX     x9, ch_path
    str     w12, [x9, x19, lsl #2]
    LDX     x9, ch_done_path
    str     w12, [x9, x19, lsl #2]
    LDX     x9, ch_paths
    str     w12, [x9, x19, lsl #2]
    LDX     x9, ch_events
    str     w12, [x9, x19, lsl #2]
    LDX     x9, ch_fg
    str     x12, [x9, x19, lsl #3]
    LDX     x9, ch_bg
    str     x12, [x9, x19, lsl #3]
    LDX     x9, ch_cell
    str     w12, [x9, x19, lsl #2]
    LDX     x9, ch_nbr
    add     x9, x9, x19, lsl #4
    stp     x12, x12, [x9]
    // identity, symbol, coordinates
    LDX     x9, ch_id
    LDW     w3, next_character_id
    str     w3, [x9, x19, lsl #2]
    add     w3, w3, #1
    STW     w3, next_character_id
    LDX     x9, ch_sym
    str     x21, [x9, x19, lsl #3]
    LDX     x9, ch_row
    str     w2, [x9, x19, lsl #2]
    LDX     x9, ch_irow
    str     w2, [x9, x19, lsl #2]
    LDX     x9, ch_col
    str     w22, [x9, x19, lsl #2]
    LDX     x9, ch_icol
    str     w22, [x9, x19, lsl #2]
    // the plain visual of the symbol
    mov     x0, #NONE
    mov     x1, #NONE
    mov     x2, x21
    mov     w3, #0
    bl      visual_make
    LDX     x3, ch_handle
    str     w9, [x3, x19, lsl #2]
    mov     w9, w19
    POP2    x21, x22
    POP2    x19, x30
    ret
new_char.full:
    ADRG    x0, msg_chars_full
    mov     x1, #msg_chars_full_len
    b       fatal

// add_character(x0=packed symbol, x1=coord) -> w9 = slot.
// Terminal.add_character: registered only in added_characters (not in the
// input-coordinate map or the neighbor graph), fill-less, no preexisting
// colors.
add_character:
    PUSH2   x19, x30
    asr     x2, x1, #32                 // row
    // w1 keeps the column (low half)
    bl      new_char
    mov     w19, w9
    LDX     x3, ch_flags
    ldrh    w2, [x3, x19, lsl #1]
    orr     w2, w2, #CF_ADDED
    strh    w2, [x3, x19, lsl #1]
    LDW     w3, added_count
    LDX     x2, added_chars
    str     w19, [x2, x3, lsl #2]
    add     w3, w3, #1
    STW     w3, added_count
    mov     w9, w19
    POP2    x19, x30
    ret

// char_coord(w0=slot) -> x9 = current coordinate (packed). Clobbers x3.
char_coord:
    LDX     x9, ch_row
    ldr     w9, [x9, w0, uxtw #2]
    lsl     x9, x9, #32
    LDX     x3, ch_col
    ldr     w3, [x3, w0, uxtw #2]
    orr     x9, x9, x3
    ret

// char_input_coord(w0=slot) -> x9 = input coordinate (packed). Clobbers x3.
char_input_coord:
    LDX     x9, ch_irow
    ldr     w9, [x9, w0, uxtw #2]
    lsl     x9, x9, #32
    LDX     x3, ch_icol
    ldr     w3, [x3, w0, uxtw #2]
    orr     x9, x9, x3
    ret

// set_coordinate(w0=slot, x1=coord): Motion.set_coordinate. A visible
// character that changes cells invalidates the render grid.
set_coordinate:
    LDX     x9, ch_row
    asr     x3, x1, #32
    str     w3, [x9, w0, uxtw #2]
    LDX     x9, ch_col
    str     w1, [x9, w0, uxtw #2]
    b       coordinate_changed

// set_layer(w0=slot, x1=layer)
set_layer:
    LDX     x9, ch_layer
    ldr     w12, [x9, w0, uxtw #2]
    cmp     w12, w1
    b.eq    set_layer.same
    str     w1, [x9, w0, uxtw #2]
    b       layer_changed
set_layer.same:
    ret

    .section .rodata
STRING msg_chars_full, "ttfx: asm engine: character limit reached\n"

    TSTATE
    .balign 8
// The field pointers, in CHAR_FIELDS order. Spelled out rather than made by
// a CHAR_FIELDS macro so that tools/asm/aarch64/check.py sees the labels;
// the check below fails the build if CHAR_FIELDS gains a field.
ch_sym:             .skip 8
ch_row:             .skip 8
ch_col:             .skip 8
ch_irow:            .skip 8
ch_icol:            .skip 8
ch_id:              .skip 8
ch_layer:           .skip 8
ch_handle:          .skip 8
ch_scene:           .skip 8
ch_scenes:          .skip 8
ch_path:            .skip 8
ch_done_path:       .skip 8
ch_paths:           .skip 8
ch_events:          .skip 8
ch_subs:            .skip 8
ch_flags:           .skip 8
ch_fg:              .skip 8
ch_bg:              .skip 8
ch_nbr:             .skip 8
ch_cell:            .skip 8
ch_user0:           .skip 8
ch_user1:           .skip 8
.macro CHARS_DECLARE_FIELD name, size, init
    .ifndef \name
    .error "chars.s: CHAR_FIELDS field \name not declared"
    .endif
.endm
CHAR_FIELDS CHARS_DECLARE_FIELD
char_count:         .skip 4
next_character_id:  .skip 4
added_chars:        .skip 8
added_count:        .skip 4
