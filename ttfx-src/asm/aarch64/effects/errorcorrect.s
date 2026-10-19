// effects/errorcorrect.s - "Some characters start in the wrong position and
// are corrected in sequence" (src/effects/errorcorrect.rs).
//
// Config (src/asm/effects.rs, the Errorcorrect arm).
//
// character_final_color_map lives in ch_user0 (fg) and ch_user1 (bg). Scenes
// are created in Rust's order, so their auto ids (and so the event keys) are
// Rust's; the ids are read back from the scene records.

.equ HAVE_errorcorrect, 1

.equ ERRORCORRECT.error_pairs,      0       // f64
.equ ERRORCORRECT.swap_delay,       8
.equ ERRORCORRECT.error_color,      16
.equ ERRORCORRECT.correct_color,    24
.equ ERRORCORRECT.movement_speed,   32      // f64
.equ ERRORCORRECT.final_stops,      40      // *const u64
.equ ERRORCORRECT.final_stop_count, 48
.equ ERRORCORRECT.final_steps,      56      // *const i64
.equ ERRORCORRECT.final_step_count, 64
.equ ERRORCORRECT.final_direction,  72
.equ ERRORCORRECT_size,             80

// scene names
.equ EC_SCN_ERROR,          NAME_LITERAL + 0
// path names
.equ EC_PATH_INPUT,         NAME_LITERAL + 0

// a packed block element U+2580..U+25BF (E2 96 xx): EC_BLOCK(cp) is
// 0x300000000 | ((0x80 | (cp & 0x3f)) << 16) | 0x96e2
.equ EC_BLOCK_BASE,         0x3000096e2     // EC_BLOCK(0x2580) & ~0xff0000
.equ EC_BLOCK_2593,         (0x300000000 | ((0x80 | (0x2593 & 0x3f)) << 16) | 0x96e2)
.equ EC_BLOCK_2588,         (0x300000000 | ((0x80 | (0x2588 & 0x3f)) << 16) | 0x96e2)

    .text

// errorcorrect_build: ErrorCorrect::build.
errorcorrect_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56]: char2
    LDX     x19, effect_config
    bl      ec_final_map
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    cset    w9, eq
    STB     w9, ec_dynamic
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    STX     x2, ec_char_count
    // the final color map, then a one-frame spawn scene per character
    // (the map has no observable effect, so both loops run as one)
    mov     x23, #0
errorcorrect_build.spawn:
    cmp     x23, x22
    b.hs    errorcorrect_build.swaps
    ldr     w24, [x21, x23, lsl #2]
    mov     w0, w24
    bl      ec_final_colors
    LDX     x3, ch_user0
    str     x9, [x3, x24, lsl #3]
    LDX     x3, ch_user1
    str     x2, [x3, x24, lsl #3]
    mov     w0, w24
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    LDX     x3, ch_user0
    ldr     x3, [x3, x24, lsl #3]
    LDX     x4, ch_user1
    ldr     x4, [x4, x24, lsl #3]
    LDX     x1, ch_sym
    ldr     x1, [x1, x24, lsl #3]
    mov     w0, w20
    mov     w2, #1
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w24
    mov     w1, w20
    bl      scene_activate
    mov     w0, w24
    bl      set_visible
    add     x23, x23, #1
    b       errorcorrect_build.spawn
errorcorrect_build.swaps:
    // the correcting gradient: error -> correct in 10 steps
    ADRG    x0, ec_pair_stops
    ldr     x9, [x19, #ERRORCORRECT.error_color]
    str     x9, [x0]
    ldr     x9, [x19, #ERRORCORRECT.correct_color]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, ec_ten_steps
    mov     w3, #1
    ADRG    x4, ec_correcting
    bl      gradient_new
    mov     w9, w9
    STX     x9, ec_correcting_len
    // all_characters: a copy of input_characters
    LDX     x23, input_count
    lsl     x0, x23, #2
    add     x0, x0, #8
    bl      alloc
    mov     x21, x9
    LDX     x1, input_chars
    mov     x3, #0
errorcorrect_build.copy:
    cmp     x3, x23
    b.hs    errorcorrect_build.copied
    ldr     w9, [x1, x3, lsl #2]
    str     w9, [x21, x3, lsl #2]
    add     x3, x3, #1
    b       errorcorrect_build.copy
errorcorrect_build.copied:
    bl      ec_fen_init
    lsl     x0, x23, #2
    add     x0, x0, #8
    bl      alloc
    STX     x9, ec_swapped
    mov     x22, x9                     // write pointer
    // pair_count = (error_pairs * characters.len()) as i64
    LDX     x9, ec_char_count
    scvtf   d1, x9
    ldr     d0, [x19, #ERRORCORRECT.error_pairs]
    fmul    d0, d0, d1
    bl      f64_to_i64
    mov     x24, x9
errorcorrect_build.pair:
    cmp     x24, #0
    b.le    errorcorrect_build.built
    cmp     x23, #2
    b.lo    errorcorrect_build.built
    bl      ec_take
    mov     w20, w9                     // char1
    bl      ec_take
    str     w9, [sp, #56]               // char2
    str     w20, [x22]
    str     w9, [x22, #4]
    add     x22, x22, #8
    LDX     x9, ec_swapped_count
    add     x9, x9, #1
    STX     x9, ec_swapped_count
    mov     w0, w20
    ldr     w1, [sp, #56]
    bl      ec_place
    ldr     w0, [sp, #56]
    mov     w1, w20
    bl      ec_place
    mov     w0, w20
    bl      ec_configure
    ldr     w0, [sp, #56]
    bl      ec_configure
    sub     x24, x24, #1
    b       errorcorrect_build.pair
errorcorrect_build.built:
    STX     xzr, ec_swap_delay
    STX     xzr, ec_swapped_head
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// ec_take -> w9 = all_characters.remove(rng.randrange(0, len)), with
// x21 = the original list and x23 = len (decremented). Vec::remove keeps the
// order, so the k-th remaining element is found in a Fenwick tree of the
// present ones instead of shifting the list.
ec_take:
    PUSH1   x30
    mov     x0, #0
    mov     x1, x23
    bl      rng_randrange
    POP1    x30
    sub     x23, x23, #1
    LDX     x4, ec_fen
    LDX     x5, ec_fen_n
    add     x2, x9, #1
    mov     x9, #0
    LDX     x3, ec_fen_top
ec_take.step:
    cbz     x3, ec_take.found
    add     x10, x9, x3
    cmp     x10, x5
    b.hi    ec_take.half
    ldr     w11, [x4, x10, lsl #2]
    cmp     x11, x2
    b.hs    ec_take.half
    mov     x9, x10
    sub     x2, x2, x11
ec_take.half:
    lsr     x3, x3, #1
    b       ec_take.step
ec_take.found:
    ldr     w2, [x21, x9, lsl #2]       // the element
    add     x10, x9, #1
ec_take.remove:
    cmp     x10, x5
    b.hi    ec_take.done
    ldr     w11, [x4, x10, lsl #2]
    sub     w11, w11, #1
    str     w11, [x4, x10, lsl #2]
    neg     x11, x10
    and     x11, x11, x10
    add     x10, x10, x11
    b       ec_take.remove
ec_take.done:
    mov     w9, w2
    ret

// ec_fen_init(x23=n): the Fenwick tree of ec_take over n present elements.
// Preserves x21-x24.
ec_fen_init:
    PUSH1   x30
    STX     x23, ec_fen_n
    lsl     x0, x23, #2
    add     x0, x0, #4
    bl      alloc
    POP1    x30
    STX     x9, ec_fen
    mov     x3, #1
ec_fen_init.fill:
    cmp     x3, x23
    b.hi    ec_fen_init.top
    neg     x2, x3
    and     x2, x2, x3
    str     w2, [x9, x3, lsl #2]
    add     x3, x3, #1
    b       ec_fen_init.fill
ec_fen_init.top:
    mov     x9, #0
    cbz     x23, ec_fen_init.done
    clz     x3, x23
    mov     x16, #63
    sub     x3, x16, x3
    mov     x9, #1
    lsl     x9, x9, x3
ec_fen_init.done:
    STX     x9, ec_fen_top
    ret

// ec_place(w0=slot, w1=other): the character starts at the other's input
// coordinate with an "input_coord" path home.
ec_place:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     w19, w0
    mov     w0, w1
    bl      char_input_coord
    mov     x1, x9
    mov     w0, w19
    bl      set_coordinate
    LDX     x9, effect_config
    ldr     d0, [x9, #ERRORCORRECT.movement_speed]
    mov     w0, w19
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    mov     w5, #EC_PATH_INPUT
    bl      path_new
    mov     w22, w9
    mov     w0, w19
    bl      char_input_coord
    mov     x1, x9
    mov     w0, w22
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    POP2    x22, x30
    POP2    x19, x21
    ret

// ec_scene_name(w9=scene) -> w9 = its name. Clobbers x3, x16.
ec_scene_name:
    SCENE_PTR x3, x9
    MOV64   x16, SC_NAME
    ldr     w9, [x3, x16]
    ret

// ec_configure(w0=slot): _configure_swapped_character.
ec_configure:
    stp     x19, x20, [sp, #-80]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    // [sp, #56] first name, [sp, #60] last name, [sp, #64] correcting name
    mov     w19, w0
    LDX     x21, effect_config
    // first_block_wipe and last_block_wipe
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w22, w9
    bl      ec_scene_name
    str     w9, [sp, #56]               // first name
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w23, w9
    bl      ec_scene_name
    str     w9, [sp, #60]               // last name
    mov     w20, #0
ec_configure.first:
    mov     w9, #0x2581
    add     w9, w9, w20
    and     w9, w9, #0x3f
    orr     w9, w9, #0x80
    lsl     x9, x9, #16
    MOV64   x1, EC_BLOCK_BASE
    orr     x1, x1, x9
    mov     w0, w22
    mov     w2, #3
    ldr     x3, [x21, #ERRORCORRECT.error_color]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    add     w20, w20, #1
    cmp     w20, #8
    b.lo    ec_configure.first
    // last: U+2587 down to U+2581 in the correct color; under dynamic
    // handling the last block takes the final colors
    mov     w20, #0
ec_configure.last:
    mov     w9, #0x2587
    sub     w9, w9, w20
    and     w9, w9, #0x3f
    orr     w9, w9, #0x80
    lsl     x9, x9, #16
    MOV64   x1, EC_BLOCK_BASE
    orr     x1, x1, x9
    mov     w0, w23
    mov     w2, #3
    ldr     x3, [x21, #ERRORCORRECT.correct_color]
    mov     x4, #NONE
    cmp     w20, #6
    b.ne    ec_configure.last_frame
    LDB     w9, ec_dynamic
    cbz     w9, ec_configure.last_frame
    LDX     x3, ch_user0
    ldr     x3, [x3, x19, lsl #3]
    LDX     x4, ch_user1
    ldr     x4, [x4, x19, lsl #3]
ec_configure.last_frame:
    mov     w5, #0
    bl      scene_add_frame
    add     w20, w20, #1
    cmp     w20, #7
    b.lo    ec_configure.last
    // initial: the input symbol in the error color, activated now
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9
    LDX     x1, ch_sym
    ldr     x1, [x1, x19, lsl #3]
    mov     w0, w24
    mov     w2, #1
    ldr     x3, [x21, #ERRORCORRECT.error_color]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w19
    mov     w1, w24
    bl      scene_activate
    // "error": ten flickers of the block and the white input symbol
    mov     w0, w19
    mov     w1, #EC_SCN_ERROR
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9
    mov     w20, #0
ec_configure.error:
    mov     w0, w24
    MOV64   x1, EC_BLOCK_2593
    mov     w2, #3
    ldr     x3, [x21, #ERRORCORRECT.error_color]
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w24
    LDX     x1, ch_sym
    ldr     x1, [x1, x19, lsl #3]
    mov     w2, #3
    mov     w3, #0xffffff
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    add     w20, w20, #1
    cmp     w20, #10
    b.lo    ec_configure.error
    // correcting: distance-synced full blocks over the correcting gradient
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #SCF_SYNC_DISTANCE
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9
    bl      ec_scene_name
    str     w9, [sp, #64]               // correcting name
    mov     x6, #0
    mov     x7, #0
    mov     w0, w24
    ADRG    x1, ec_full_block
    mov     x2, #1
    mov     w3, #3
    ADRG    x4, ec_correcting
    LDX     x5, ec_correcting_len
    bl      scene_apply_gradient
    // final
    mov     w0, w19
    LDB     w9, ec_dynamic
    cbz     w9, ec_configure.static_final
    bl      ec_dynamic_final
    b       ec_configure.final_named
ec_configure.static_final:
    ADRG    x0, ec_pair_stops
    ldr     x9, [x21, #ERRORCORRECT.correct_color]
    str     x9, [x0]
    LDX     x9, ch_user0
    ldr     x9, [x9, x19, lsl #3]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, ec_ten_steps
    mov     w3, #1
    ADRG    x4, ec_fg_spectrum
    bl      gradient_new
    mov     w24, w9
    mov     w0, w19
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    mov     x6, #0
    mov     x7, #0
    mov     w0, w20
    LDX     x1, ch_sym
    add     x1, x1, x19, lsl #3
    mov     x2, #1
    mov     w3, #3
    ADRG    x4, ec_fg_spectrum
    mov     w5, w24
    bl      scene_apply_gradient
    mov     w9, w20
ec_configure.final_named:
    bl      ec_scene_name
    mov     w20, w9                     // final name
    // events, in Rust's order
    mov     x6, #0
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    mov     w3, #EC_SCN_ERROR
    mov     w4, #ACT_ACTIVATE_SCENE
    ldr     w5, [sp, #56]               // first
    bl      event_register
    mov     x6, #0
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    ldr     w3, [sp, #56]               // first
    mov     w4, #ACT_ACTIVATE_SCENE
    ldr     w5, [sp, #64]               // correcting
    bl      event_register
    mov     x6, #0
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    ldr     w3, [sp, #56]               // first
    mov     w4, #ACT_ACTIVATE_PATH
    mov     w5, #EC_PATH_INPUT
    bl      event_register
    mov     x6, #0
    mov     w0, w19
    mov     w1, #EV_PATH_ACTIVATED
    mov     w2, #CALLER_PATH
    mov     w3, #EC_PATH_INPUT
    mov     w4, #ACT_SET_LAYER
    mov     w5, #1
    bl      event_register
    mov     x6, #0
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w3, #EC_PATH_INPUT
    mov     w4, #ACT_SET_LAYER
    mov     w5, #0
    bl      event_register
    mov     x6, #0
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    mov     w3, #EC_PATH_INPUT
    mov     w4, #ACT_ACTIVATE_SCENE
    ldr     w5, [sp, #60]               // last
    bl      event_register
    mov     x6, #0
    mov     w0, w19
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    ldr     w3, [sp, #60]               // last
    mov     w4, #ACT_ACTIVATE_SCENE
    mov     w5, w20                     // final
    bl      event_register
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #80
    ret

// ec_dynamic_final(w0=slot) -> w9 = scene: _get_dynamic_final_scene, the
// correct color fading to the input colors.
ec_dynamic_final:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    stp     x23, x30, [sp, #32]
    mov     w19, w0
    mov     w1, #AUTO
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w20, w9
    mov     w21, #0                     // fg spectrum length (0 = none)
    mov     w22, #0                     // bg spectrum length
    LDX     x9, effect_config
    ldr     x9, [x9, #ERRORCORRECT.correct_color]
    STX     x9, ec_pair_stops
    LDX     x9, ch_fg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    ec_dynamic_final.bg
    ADRG    x0, ec_pair_stops
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, ec_ten_steps
    mov     w3, #1
    ADRG    x4, ec_fg_spectrum
    bl      gradient_new
    mov     w21, w9
ec_dynamic_final.bg:
    LDX     x9, ch_bg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    ec_dynamic_final.frames
    ADRG    x0, ec_pair_stops
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, ec_ten_steps
    mov     w3, #1
    ADRG    x4, ec_bg_spectrum
    bl      gradient_new
    mov     w22, w9
ec_dynamic_final.frames:
    LDX     x1, ch_sym
    add     x1, x1, x19, lsl #3
    orr     w9, w21, w22
    cbz     w9, ec_dynamic_final.plain
    mov     x4, #0
    cbz     w21, ec_dynamic_final.no_fg
    ADRG    x4, ec_fg_spectrum
ec_dynamic_final.no_fg:
    mov     x6, #0
    cbz     w22, ec_dynamic_final.no_bg
    ADRG    x6, ec_bg_spectrum
ec_dynamic_final.no_bg:
    mov     x7, x22
    mov     w0, w20
    mov     x2, #1
    mov     w3, #3
    mov     w5, w21
    bl      scene_apply_gradient
    b       ec_dynamic_final.done
ec_dynamic_final.plain:
    ldr     x1, [x1]
    mov     w0, w20
    mov     w2, #3
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
ec_dynamic_final.done:
    mov     w9, w20
    ldp     x23, x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// ec_final_colors(w0=slot) -> x9 = fg, x2 = bg: the final color map entry
// (the input colors under dynamic handling, else the gradient mapping).
ec_final_colors:
    LDB     w9, ec_dynamic
    cbz     w9, ec_final_colors.mapped
    LDX     x9, ch_fg
    ldr     x9, [x9, w0, uxtw #3]
    LDX     x2, ch_bg
    ldr     x2, [x2, w0, uxtw #3]
    ret
ec_final_colors.mapped:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, w0, uxtw #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, ec_final_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, w0, uxtw #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, ec_final_map_ptr
    ldr     x9, [x3, x9, lsl #3]
    mov     x2, #NONE
    ret

// ec_final_map: Gradient::new(final stops, final steps) and its coordinate
// mapping over the text rectangle.
ec_final_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #ERRORCORRECT.final_steps]
    ldr     x3, [x19, #ERRORCORRECT.final_step_count]
    ldr     x1, [x19, #ERRORCORRECT.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, ec_final_spectrum
    ldr     x0, [x19, #ERRORCORRECT.final_stops]
    ldr     x1, [x19, #ERRORCORRECT.final_stop_count]
    ldr     x2, [x19, #ERRORCORRECT.final_steps]
    ldr     x3, [x19, #ERRORCORRECT.final_step_count]
    LDX     x4, ec_final_spectrum
    bl      gradient_new
    LDX     x0, ec_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, ec_final_map_width
    ldr     x6, [x19, #ERRORCORRECT.final_direction]
    bl      gradient_map
    STX     x9, ec_final_map_ptr
    POP2    x19, x30
    ret

// errorcorrect_next_frame -> w9 = 1 for a frame, 0 when done.
errorcorrect_next_frame:
    PUSH2   x19, x30
    LDX     x9, ec_swapped_head
    LDX     x10, ec_swapped_count
    cmp     x9, x10
    b.hs    errorcorrect_next_frame.delay
    LDX     x10, ec_swap_delay
    cbnz    x10, errorcorrect_next_frame.delay
    // the next pair starts its error scene
    add     x10, x9, #1
    STX     x10, ec_swapped_head
    LDX     x3, ec_swapped
    add     x19, x3, x9, lsl #3
    ldr     w0, [x19]
    mov     w1, #EC_SCN_ERROR
    bl      scene_activate_name
    ldr     w0, [x19]
    bl      active_insert
    ldr     w0, [x19, #4]
    mov     w1, #EC_SCN_ERROR
    bl      scene_activate_name
    ldr     w0, [x19, #4]
    bl      active_insert
    LDX     x9, effect_config
    ldr     x9, [x9, #ERRORCORRECT.swap_delay]
    STX     x9, ec_swap_delay
    b       errorcorrect_next_frame.step
errorcorrect_next_frame.delay:
    LDX     x9, ec_swap_delay
    cbz     x9, errorcorrect_next_frame.step
    sub     x9, x9, #1
    STX     x9, ec_swap_delay
errorcorrect_next_frame.step:
    bl      active_empty
    cbnz    w9, errorcorrect_next_frame.done
    bl      update
    mov     w9, #1
    POP2    x19, x30
    ret
errorcorrect_next_frame.done:
    mov     w9, #0
    POP2    x19, x30
    ret

    .section .rodata
    .balign 8
ec_ten_steps:       .quad 10
ec_full_block:      .quad EC_BLOCK_2588

    TSTATE
    .balign 8
ec_final_spectrum:  .skip 8
ec_final_map_ptr:   .skip 8
ec_final_map_width: .skip 8
ec_char_count:      .skip 8
ec_swapped:         .skip 8             // (char1, char2) u32 pairs
ec_swapped_count:   .skip 8
ec_swapped_head:    .skip 8
ec_swap_delay:      .skip 8
ec_correcting_len:  .skip 8
ec_pair_stops:      .skip 8 * 2
ec_correcting:      .skip 8 * 16
ec_fg_spectrum:     .skip 8 * 16
ec_bg_spectrum:     .skip 8 * 16
ec_fen:             .skip 8             // Fenwick tree (1-based u32 counts)
ec_fen_n:           .skip 8
ec_fen_top:         .skip 8
ec_dynamic:         .skip 1

    .text
