// effects/unstable.s - "Spawn characters jumbled, explode them to the edge
// of the canvas, then reassemble them in the correct layout"
// (src/effects/unstable.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Unstable):
//
// Build walks the characters top to bottom, left to right, drawing each
// one's edge target and then its jumbled start, in Rust's order. The jumbled
// start is Vec::remove(randint(0, len - 1)) on the remaining input
// coordinates; an order-statistic Fenwick tree picks the same element without
// the O(n) shifts. That character order never changes, so it is fetched once.
//
// Rust renders the offset rumble frames mid-next_frame and then moves the
// characters back. Here the engine renders after next_frame returns, so the
// move back is deferred to the start of the next call.

.equ HAVE_unstable, 1

.equ UNSTABLE.unstable_color,   0
.equ UNSTABLE.explosion_ease,   8
.equ UNSTABLE.explosion_speed,  16      // f64
.equ UNSTABLE.reassembly_ease,  24
.equ UNSTABLE.reassembly_speed, 32      // f64
.equ UNSTABLE.final_stops,      40      // *const u64
.equ UNSTABLE.final_stop_count, 48
.equ UNSTABLE.final_steps,      56      // *const i64
.equ UNSTABLE.final_step_count, 64
.equ UNSTABLE.final_direction,  72
.equ UNSTABLE_size,             80

// per-character record (un_recs + slot * UN_REC)
.equ UR_JUMBLED,        0               // packed coordinate
.equ UR_TARGET,         8               // the explosion waypoint (packed)
.equ UR_EXPLOSION,      16              // path index
.equ UR_REASSEMBLY,     20              // path index
.equ UR_FINAL,          24              // scene index
.equ UN_REC,            32

// phases
.equ UN_RUMBLE,         0
.equ UN_EXPLOSION,      1
.equ UN_REASSEMBLY,     2

// path names
.equ UN_P_EXPLOSION,    NAME_LITERAL + 0
.equ UN_P_REASSEMBLY,   NAME_LITERAL + 1
// scene names
.equ UN_S_RUMBLE,       NAME_LITERAL + 0
.equ UN_S_FINAL,        NAME_LITERAL + 1

.equ UN_NEUTRAL_GRAY,   0x808080
.equ UN_MAX_RUMBLE,     150

    .text

// unstable_build: Unstable::build.
// Locals: [sp, #56] the edge draw.
unstable_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    bl      un_final_color_map
    mov     x9, #NONE
    STX     x9, un_last_fg
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    STX     x9, un_chars
    STX     x2, un_count
    mov     x22, x9
    mov     x23, x2
    LDW     w0, char_count
    lsl     x0, x0, #5                  // * UN_REC
    bl      alloc
    STX     x9, un_recs
    mov     x0, x23
    bl      un_fen_init
    LDX     x9, cfg_existing_colors
    cbz     x9, unstable_build.no_cache // input colors enter the frames
    bl      un_cache_init
unstable_build.no_cache:
    LDX     x24, effect_config
    mov     x19, #0
unstable_build.char:
    cmp     x19, x23
    b.hs    unstable_build.built
    ldr     w21, [x22, x19, lsl #2]
    LDX     x9, un_recs
    add     x20, x9, x21, lsl #5
    // the edge target: randint(0, 3) picks left, right, bottom or top
    mov     x0, #0
    mov     x1, #3
    bl      rng_randint
    str     x9, [sp, #56]
    cmp     x9, #1
    b.hi    unstable_build.vertical
    mov     w0, #0
    bl      canvas_random_row
    mov     w3, #1                      // left
    ldr     x10, [sp, #56]
    cbz     x10, unstable_build.target
    LDX     x3, canvas_right
    b       unstable_build.target
unstable_build.vertical:
    mov     w0, #0
    bl      canvas_random_column
    mov     w3, w9                      // column
    mov     w9, #1                      // bottom
    ldr     x10, [sp, #56]
    cmp     x10, #2
    b.eq    unstable_build.target
    LDX     x9, canvas_top
unstable_build.target:
    lsl     x9, x9, #32                 // x9 = row, w3 = column
    mov     w3, w3
    orr     x9, x9, x3
    str     x9, [x20, #UR_TARGET]
    // jumbled = character_coords.remove(randint(0, len - 1))
    mov     x0, #0
    sub     x1, x23, x19
    sub     x1, x1, #1
    bl      rng_randint
    mov     x0, x9
    bl      un_fen_take
    ldr     w0, [x22, x9, lsl #2]
    bl      char_input_coord
    str     x9, [x20, #UR_JUMBLED]
    mov     w0, w21
    mov     x1, x9
    bl      set_coordinate
    // explosion path to the edge, reassembly path home
    mov     w0, w21
    ldr     d0, [x24, #UNSTABLE.explosion_speed]
    ldr     w1, [x24, #UNSTABLE.explosion_ease]
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, UN_P_EXPLOSION
    bl      path_new
    str     w9, [x20, #UR_EXPLOSION]
    mov     w0, w9
    ldr     x1, [x20, #UR_TARGET]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    mov     w0, w21
    ldr     d0, [x24, #UNSTABLE.reassembly_speed]
    ldr     w1, [x24, #UNSTABLE.reassembly_ease]
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, UN_P_REASSEMBLY
    bl      path_new
    str     w9, [x20, #UR_REASSEMBLY]
    mov     w0, w21
    bl      char_input_coord
    mov     x1, x9
    ldr     w0, [x20, #UR_REASSEMBLY]
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    // scenes
    LDX     x9, ch_sym
    ldr     x9, [x9, x21, lsl #3]
    STX     x9, un_symbol
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    unstable_build.dynamic_new
    bl      un_mapped_color
    // a character with an earlier one's (symbol, color) clones its scenes
    bl      un_cache_find
    cbz     x9, unstable_build.static
    ldr     w4, [x9, #16]
    cmn     w4, #1                      // NONE
    b.eq    unstable_build.static_fill
    PUSH1   x4                          // the earlier character
    mov     w0, w4
    MOV64   w1, UN_S_RUMBLE
    bl      scene_find
    mov     w0, w21
    mov     w1, w9
    MOV64   w2, UN_S_RUMBLE
    bl      scene_copy
    STX     x9, un_rumble_scene
    POP1    x4
    mov     w0, w4
    MOV64   w1, UN_S_FINAL
    bl      scene_find
    mov     w0, w21
    mov     w1, w9
    MOV64   w2, UN_S_FINAL
    bl      scene_copy
    str     w9, [x20, #UR_FINAL]
    b       unstable_build.activate_rumble
unstable_build.static_fill:
    str     w21, [x9, #16]
unstable_build.static:
    mov     w0, w21
    MOV64   w1, UN_S_RUMBLE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    STX     x9, un_rumble_scene
    bl      un_static_spectra
    LDW     w0, un_rumble_scene
    mov     w3, #10
    ADRG    x4, un_rumble_spec
    LDX     x5, un_rumble_len
    bl      un_apply_fg
    mov     w0, w21
    MOV64   w1, UN_S_FINAL
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     w9, [x20, #UR_FINAL]
    mov     w0, w9
    mov     w3, #3
    ADRG    x4, un_final_spec
    LDX     x5, un_final_len
    bl      un_apply_fg
unstable_build.activate_rumble:
    mov     w0, w21
    LDW     w1, un_rumble_scene
    bl      scene_activate
    b       unstable_build.visible
unstable_build.dynamic_new:
    mov     w0, w21
    MOV64   w1, UN_S_RUMBLE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    STX     x9, un_rumble_scene
    bl      un_dynamic_scenes
unstable_build.visible:
    mov     w0, w21
    bl      set_visible
    add     x19, x19, #1
    b       unstable_build.char
unstable_build.built:
    STX     xzr, un_phase               // UN_RUMBLE
    STX     xzr, un_rumble_steps
    mov     x9, #18
    STX     x9, un_mod_delay
    mov     x9, #30
    STX     x9, un_hold
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// un_cache_init: the (symbol, color) -> character table of unstable_build,
// 32-byte entries (symbol 0 = empty), at least twice the character count.
un_cache_init:
    PUSH1   x30
    LDW     w9, char_count
    add     x9, x9, x9
    mov     x3, #16
    mov     w2, #0
un_cache_init.size:
    cmp     x3, x9
    b.hs    un_cache_init.sized
    add     x3, x3, x3
    add     w2, w2, #1
    b       un_cache_init.size
un_cache_init.sized:
    sub     x9, x3, #1
    STX     x9, un_cache_mask
    add     w2, w2, #4
    mov     w9, #64
    sub     w9, w9, w2
    STX     x9, un_cache_shift
    lsl     x3, x3, #5
    add     x0, x3, #64
    bl      alloc
    STX     x9, un_cache
    POP1    x30
    ret

// un_cache_find -> x9 = the entry of ([un_symbol], [un_fg]), created with
// character NONE when new, or 0 when the cache is off. Clobbers x3, x2,
// x1, x0.
un_cache_find:
    LDX     x1, un_cache
    cbz     x1, un_cache_find.off
    LDX     x0, un_symbol
    LDX     x9, un_fg
    MOV64   x3, 0x9E3779B97F4A7C15
    mul     x9, x9, x3
    eor     x9, x9, x0
    MOV64   x3, 0xBF58476D1CE4E5B9
    mul     x9, x9, x3
    LDX     x3, un_cache_shift
    lsr     x9, x9, x3
un_cache_find.probe:
    add     x2, x1, x9, lsl #5
    ldr     x3, [x2]
    cbz     x3, un_cache_find.new
    cmp     x3, x0
    b.ne    un_cache_find.next
    LDX     x3, un_fg
    ldr     x16, [x2, #8]
    cmp     x16, x3
    b.eq    un_cache_find.found
un_cache_find.next:
    add     x9, x9, #1
    LDX     x3, un_cache_mask
    and     x9, x9, x3
    b       un_cache_find.probe
un_cache_find.new:
    str     x0, [x2]
    LDX     x3, un_fg
    str     x3, [x2, #8]
    mov     w3, #NONE
    str     w3, [x2, #16]
un_cache_find.found:
    mov     x9, x2
    ret
un_cache_find.off:
    mov     x9, #0
    ret

// un_mapped_color (w21 = slot): [un_fg] = final_gradient_mapping[input_coord].
un_mapped_color:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x21, lsl #2]
    LDX     x3, text_bottom
    sub     x9, x9, x3
    LDX     x3, un_map_width
    mul     x9, x9, x3
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x21, lsl #2]
    add     x9, x9, x3
    LDX     x3, text_left
    sub     x9, x9, x3
    LDX     x3, un_map
    ldr     x9, [x3, x9, lsl #3]
    STX     x9, un_fg
    ret

// un_static_spectra: the rumble ([fg, unstable]) and final ([unstable, fg])
// spectra of [un_fg], 12 steps each. Consecutive characters mostly share
// their color, so the last one's spectra are kept.
un_static_spectra:
    PUSH1   x30
    LDX     x9, un_fg
    LDX     x3, un_last_fg
    cmp     x9, x3
    b.eq    un_static_spectra.done
    STX     x9, un_last_fg
    mov     x0, x9
    LDX     x9, effect_config
    ldr     x1, [x9, #UNSTABLE.unstable_color]
    ADRG    x2, un_rumble_spec
    bl      un_pair_gradient
    STX     x9, un_rumble_len
    LDX     x9, effect_config
    ldr     x0, [x9, #UNSTABLE.unstable_color]
    LDX     x1, un_last_fg
    ADRG    x2, un_final_spec
    bl      un_pair_gradient
    STX     x9, un_final_len
un_static_spectra.done:
    POP1    x30
    ret

// un_pair_gradient(x0=from, x1=to, x2=out) -> x9 = length:
// Gradient::with_steps(&[from, to], 12, false).
un_pair_gradient:
    ADRG    x3, un_pair
    stp     x0, x1, [x3]
    mov     x4, x2
    mov     x0, x3
    mov     w1, #2
    ADRG    x2, un_twelve
    mov     w3, #1
    b       gradient_new

// un_apply_fg(w0=scene, w3=duration, x4=fg spectrum, x5=count):
// apply_gradient_to_symbols(&[input symbol], duration, Some(fg), None).
un_apply_fg:
    ADRG    x1, un_symbol
    mov     x2, #1
    mov     x6, #0
    mov     x7, #0
    b       scene_apply_gradient

// un_apply(w0=scene, w3=duration, x4=fg spectrum/0, x5=fg count,
// x10=bg spectrum/0, x11=bg count).
un_apply:
    ADRG    x1, un_symbol
    mov     x2, #1
    mov     x6, x10
    mov     x7, x11
    b       scene_apply_gradient

// un_dynamic_scenes (w21 = slot, x20 = record): the rumble and final scenes
// under --existing-color-handling dynamic, the rumble activation and the
// start appearance. The rumble scene is [un_rumble_scene].
un_dynamic_scenes:
    PUSH2   x19, x22
    PUSH2   x23, x30
    LDX     x9, ch_fg
    ldr     x22, [x9, x21, lsl #3]      // input fg / NONE
    LDX     x9, ch_bg
    ldr     x23, [x9, x21, lsl #3]      // input bg / NONE
    // rumble: [start fg, unstable] and [bg, unstable], 10 ticks each
    mov     x0, x22
    cmn     x0, #1                      // NONE
    b.ne    un_dynamic_scenes.start_fg
    MOV64   w0, UN_NEUTRAL_GRAY
un_dynamic_scenes.start_fg:
    LDX     x9, effect_config
    ldr     x1, [x9, #UNSTABLE.unstable_color]
    ADRG    x2, un_rumble_spec
    bl      un_pair_gradient
    STX     x9, un_rumble_len
    mov     x10, #0
    mov     x11, #0
    cmn     x23, #1                     // NONE
    b.eq    un_dynamic_scenes.rumble
    mov     x0, x23
    LDX     x9, effect_config
    ldr     x1, [x9, #UNSTABLE.unstable_color]
    ADRG    x2, un_bg_spec
    bl      un_pair_gradient
    mov     x11, x9
    ADRG    x10, un_bg_spec
un_dynamic_scenes.rumble:
    LDW     w0, un_rumble_scene
    mov     w3, #10
    ADRG    x4, un_rumble_spec
    LDX     x5, un_rumble_len
    bl      un_apply
    // final
    mov     w0, w21
    MOV64   w1, UN_S_FINAL
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     w9, [x20, #UR_FINAL]
    mov     w19, w9
    cmn     x22, #1                     // NONE
    b.ne    un_dynamic_scenes.final_gradients
    cmn     x23, #1
    b.ne    un_dynamic_scenes.final_gradients
    // no input colors: [unstable, gray], then the plain symbol
    LDX     x9, effect_config
    ldr     x0, [x9, #UNSTABLE.unstable_color]
    MOV64   w1, UN_NEUTRAL_GRAY
    ADRG    x2, un_final_spec
    bl      un_pair_gradient
    mov     x5, x9
    mov     w0, w19
    mov     w3, #3
    ADRG    x4, un_final_spec
    bl      un_apply_fg
    mov     w0, w19
    LDX     x1, un_symbol
    mov     w2, #3
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
    b       un_dynamic_scenes.activate
un_dynamic_scenes.final_gradients:
    STX     xzr, un_final_len
    STX     xzr, un_bg_len
    cmn     x22, #1                     // NONE
    b.eq    un_dynamic_scenes.final_bg
    LDX     x9, effect_config
    ldr     x0, [x9, #UNSTABLE.unstable_color]
    mov     x1, x22
    ADRG    x2, un_final_spec
    bl      un_pair_gradient
    STX     x9, un_final_len
un_dynamic_scenes.final_bg:
    cmn     x23, #1                     // NONE
    b.eq    un_dynamic_scenes.final_apply
    LDX     x9, effect_config
    ldr     x0, [x9, #UNSTABLE.unstable_color]
    mov     x1, x23
    ADRG    x2, un_bg_spec
    bl      un_pair_gradient
    STX     x9, un_bg_len
un_dynamic_scenes.final_apply:
    mov     x4, #0
    mov     x5, #0
    cmn     x22, #1                     // NONE
    b.eq    un_dynamic_scenes.no_fg
    ADRG    x4, un_final_spec
    LDX     x5, un_final_len
un_dynamic_scenes.no_fg:
    mov     x10, #0
    mov     x11, #0
    cmn     x23, #1                     // NONE
    b.eq    un_dynamic_scenes.no_bg
    ADRG    x10, un_bg_spec
    LDX     x11, un_bg_len
un_dynamic_scenes.no_bg:
    mov     w0, w19
    mov     w3, #3
    bl      un_apply
    cmn     x22, #1                     // NONE
    b.ne    un_dynamic_scenes.activate
    mov     w0, w19
    LDX     x1, un_symbol
    mov     w2, #3
    mov     x3, #NONE
    mov     x4, x23
    mov     w5, #0
    bl      scene_add_frame
un_dynamic_scenes.activate:
    mov     w0, w21
    LDW     w1, un_rumble_scene
    bl      scene_activate
    // set_appearance(input symbol, start colors)
    mov     x2, x22
    cmn     x2, #1                      // NONE
    b.ne    un_dynamic_scenes.appear
    MOV64   w2, UN_NEUTRAL_GRAY
un_dynamic_scenes.appear:
    mov     w0, w21
    mov     x1, #0
    mov     x3, x23
    bl      set_appearance
    POP2    x23, x30
    POP2    x19, x22
    ret

// un_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
un_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #UNSTABLE.final_steps]
    ldr     x3, [x19, #UNSTABLE.final_step_count]
    ldr     x1, [x19, #UNSTABLE.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, un_spectrum
    ldr     x0, [x19, #UNSTABLE.final_stops]
    ldr     x1, [x19, #UNSTABLE.final_stop_count]
    ldr     x2, [x19, #UNSTABLE.final_steps]
    ldr     x3, [x19, #UNSTABLE.final_step_count]
    LDX     x4, un_spectrum
    bl      gradient_new
    LDX     x0, un_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, un_map_width
    ldr     x6, [x19, #UNSTABLE.final_direction]
    bl      gradient_map
    STX     x9, un_map
    POP2    x19, x30
    ret

// un_fen_init(x0=n): a Fenwick tree over n present elements.
un_fen_init:
    PUSH2   x19, x30
    mov     x19, x0
    STX     x0, un_fen_n
    lsl     x0, x0, #2
    add     x0, x0, #4
    bl      alloc
    STX     x9, un_fen
    mov     x3, #1
un_fen_init.fill:
    cmp     x3, x19
    b.hi    un_fen_init.top
    neg     x2, x3
    and     x2, x2, x3
    str     w2, [x9, x3, lsl #2]
    add     x3, x3, #1
    b       un_fen_init.fill
un_fen_init.top:
    mov     x9, #0
    cbz     x19, un_fen_init.done
    clz     x3, x19
    mov     x9, #63
    sub     x3, x9, x3                  // the highest set bit
    mov     x9, #1
    lsl     x9, x9, x3
un_fen_init.done:
    STX     x9, un_fen_top
    POP2    x19, x30
    ret

// un_fen_take(x0=k) -> x9 = the index of the k-th (0-based) remaining
// element, which is removed. Clobbers x2-x5, x10, x11.
un_fen_take:
    LDX     x4, un_fen
    LDX     x5, un_fen_n
    add     x2, x0, #1
    mov     x9, #0
    LDX     x3, un_fen_top
un_fen_take.step:
    cbz     x3, un_fen_take.found
    add     x10, x9, x3
    cmp     x10, x5
    b.hi    un_fen_take.half
    ldr     w11, [x4, x10, lsl #2]
    cmp     x11, x2
    b.hs    un_fen_take.half
    mov     x9, x10
    sub     x2, x2, x11
un_fen_take.half:
    lsr     x3, x3, #1
    b       un_fen_take.step
un_fen_take.found:
    add     x10, x9, #1
un_fen_take.remove:
    cmp     x10, x5
    b.hi    un_fen_take.done
    ldr     w11, [x4, x10, lsl #2]
    sub     w11, w11, #1
    str     w11, [x4, x10, lsl #2]
    neg     x11, x10
    and     x11, x11, x10
    add     x10, x10, x11
    b       un_fen_take.remove
un_fen_take.done:
    ret

// un_restore: move every character back to its jumbled coordinate after an
// offset rumble frame was rendered.
un_restore:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     x19, #0
un_restore.char:
    LDX     x9, un_count
    cmp     x19, x9
    b.hs    un_restore.done
    LDX     x9, un_chars
    ldr     w21, [x9, x19, lsl #2]
    LDX     x22, un_recs
    add     x22, x22, x21, lsl #5
    mov     w0, w21
    ldr     x1, [x22, #UR_JUMBLED]
    bl      set_coordinate
    add     x19, x19, #1
    b       un_restore.char
un_restore.done:
    STB     wzr, un_restore_pending
    POP2    x22, x30
    POP2    x19, x21
    ret

// UN_BIT_POP xdest, xbits: xdest = the lowest set bit's index, cleared from
// xbits (xbits != 0). Clobbers x3.
.macro UN_BIT_POP xdest, xbits
    rbit    \xdest, \xbits
    clz     \xdest, \xdest
    sub     x3, \xbits, #1
    and     \xbits, \xbits, x3
.endm

// un_tick_active: tick every active character in ascending slot order (no
// callbacks here, so the live set is its own snapshot). Walks the bitmap a
// word at a time; a tick only touches its own slot's bit.
un_tick_active:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDW     w22, char_count
    add     x22, x22, #63
    lsr     x22, x22, #6
    mov     x19, #0
un_tick_active.word:
    cmp     x19, x22
    b.hs    un_tick_active.done
    LDX     x9, active_bits
    ldr     x21, [x9, x19, lsl #3]
un_tick_active.bit:
    cbz     x21, un_tick_active.next
    UN_BIT_POP x0, x21
    add     x0, x0, x19, lsl #6
    bl      tick
    b       un_tick_active.bit
un_tick_active.next:
    add     x19, x19, #1
    b       un_tick_active.word
un_tick_active.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// un_retain(w0=0 explosion, 1 reassembly): drop the active characters that
// reached the phase's waypoint (and, reassembling, finished their scene).
un_retain:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     w21, w0
    LDW     w24, char_count
    add     x24, x24, #63
    lsr     x24, x24, #6
    mov     x20, #0                     // word index
un_retain.word:
    cmp     x20, x24
    b.hs    un_retain.done
    LDX     x9, active_bits
    ldr     x23, [x9, x20, lsl #3]
un_retain.slot:
    cbz     x23, un_retain.next_word
    UN_BIT_POP x19, x23
    add     x19, x19, x20, lsl #6
    mov     w0, w19
    bl      char_coord
    mov     x22, x9
    cbnz    w21, un_retain.home
    LDX     x9, un_recs
    add     x9, x9, x19, lsl #5
    ldr     x3, [x9, #UR_TARGET]
    cmp     x22, x3
    b.ne    un_retain.slot
    b       un_retain.remove
un_retain.home:
    mov     w0, w19
    bl      char_input_coord
    cmp     x22, x9
    b.ne    un_retain.slot
    mov     w0, w19
    bl      scene_is_complete
    cbz     w9, un_retain.slot
un_retain.remove:
    mov     w0, w19
    bl      active_remove
    b       un_retain.slot
un_retain.next_word:
    add     x20, x20, #1
    b       un_retain.word
un_retain.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// unstable_next_frame -> w9 = 1 for a frame, 0 when done.
unstable_next_frame:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDB     w9, un_restore_pending
    cbz     w9, unstable_next_frame.phase
    bl      un_restore
unstable_next_frame.phase:
    LDX     x9, un_phase
    cmp     x9, #UN_RUMBLE
    b.ne    unstable_next_frame.explosion
    LDX     x9, un_rumble_steps
    cmp     x9, #UN_MAX_RUMBLE
    b.ge    unstable_next_frame.explode
    cmp     x9, #30
    b.le    unstable_next_frame.plain_rumble
    LDX     x10, un_mod_delay
    sdiv    x2, x9, x10
    msub    x2, x2, x10, x9
    cbnz    x2, unstable_next_frame.plain_rumble
    // offset every character by one random (row, column) step
    mov     x0, #3
    bl      rng_below
    sub     x23, x9, #1                 // row offset
    mov     x0, #3
    bl      rng_below
    sub     x24, x9, #1                 // column offset
    mov     x19, #0
unstable_next_frame.offset:
    LDX     x9, un_count
    cmp     x19, x9
    b.hs    unstable_next_frame.offset_done
    LDX     x9, un_chars
    ldr     w21, [x9, x19, lsl #2]
    mov     w0, w21
    bl      char_coord
    asr     x1, x9, #32
    add     x1, x1, x23
    lsl     x1, x1, #32
    add     w9, w9, w24
    orr     x1, x1, x9
    mov     w0, w21
    bl      set_coordinate
    mov     w0, w21
    bl      step_animation
    add     x19, x19, #1
    b       unstable_next_frame.offset
unstable_next_frame.offset_done:
    mov     w9, #1
    STB     w9, un_restore_pending
    LDX     x9, un_mod_delay
    sub     x9, x9, #1
    mov     x3, #1
    cmp     x9, x3
    csel    x9, x3, x9, lt
    STX     x9, un_mod_delay
    b       unstable_next_frame.rumbled
unstable_next_frame.plain_rumble:
    mov     x19, #0
unstable_next_frame.step:
    LDX     x9, un_count
    cmp     x19, x9
    b.hs    unstable_next_frame.rumbled
    LDX     x9, un_chars
    ldr     w0, [x9, x19, lsl #2]
    bl      step_animation
    add     x19, x19, #1
    b       unstable_next_frame.step
unstable_next_frame.rumbled:
    LDX     x9, un_rumble_steps
    add     x9, x9, #1
    STX     x9, un_rumble_steps
    b       unstable_next_frame.frame
unstable_next_frame.explode:
    mov     x9, #UN_EXPLOSION
    STX     x9, un_phase
    mov     x19, #0
unstable_next_frame.activate_explosion:
    LDX     x9, un_count
    cmp     x19, x9
    b.hs    unstable_next_frame.activated
    LDX     x9, un_chars
    ldr     w21, [x9, x19, lsl #2]
    LDX     x9, un_recs
    add     x9, x9, x21, lsl #5
    mov     w0, w21
    ldr     w1, [x9, #UR_EXPLOSION]
    bl      path_activate
    add     x19, x19, #1
    b       unstable_next_frame.activate_explosion
unstable_next_frame.activated:
    bl      active_clear
    mov     x19, #0
unstable_next_frame.insert:
    LDX     x9, un_count
    cmp     x19, x9
    b.hs    unstable_next_frame.explosion
    LDX     x9, un_chars
    ldr     w0, [x9, x19, lsl #2]
    bl      active_insert
    add     x19, x19, #1
    b       unstable_next_frame.insert
unstable_next_frame.explosion:
    LDX     x9, un_phase
    cmp     x9, #UN_EXPLOSION
    b.ne    unstable_next_frame.reassembly
    bl      active_empty
    cbnz    w9, unstable_next_frame.exploded
    bl      un_tick_active
    mov     w0, #0
    bl      un_retain
    b       unstable_next_frame.frame
unstable_next_frame.exploded:
    LDX     x9, un_hold
    cbz     x9, unstable_next_frame.reassemble
    sub     x9, x9, #1
    STX     x9, un_hold
    b       unstable_next_frame.frame
unstable_next_frame.reassemble:
    mov     x9, #UN_REASSEMBLY
    STX     x9, un_phase
    mov     x19, #0
unstable_next_frame.home:
    LDX     x9, un_count
    cmp     x19, x9
    b.hs    unstable_next_frame.reassembly
    LDX     x9, un_chars
    ldr     w21, [x9, x19, lsl #2]
    LDX     x22, un_recs
    add     x22, x22, x21, lsl #5
    mov     w0, w21
    ldr     w1, [x22, #UR_FINAL]
    bl      scene_activate
    mov     w0, w21
    bl      active_insert
    mov     w0, w21
    ldr     w1, [x22, #UR_REASSEMBLY]
    bl      path_activate
    add     x19, x19, #1
    b       unstable_next_frame.home
unstable_next_frame.reassembly:
    LDX     x9, un_phase
    cmp     x9, #UN_REASSEMBLY
    b.ne    unstable_next_frame.finished
    bl      active_empty
    cbnz    w9, unstable_next_frame.finished
    bl      un_tick_active
    mov     w0, #1
    bl      un_retain
unstable_next_frame.frame:
    mov     w9, #1
    b       unstable_next_frame.out
unstable_next_frame.finished:
    mov     w9, #0
unstable_next_frame.out:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

    .section .rodata
    .balign 8
un_twelve:          .quad 12

    TSTATE
    .balign 8
un_spectrum:        .skip 8
un_map:             .skip 8
un_map_width:       .skip 8
un_chars:           .skip 8         // u32 slots, top to bottom, left to right
un_count:           .skip 8
un_recs:            .skip 8
un_fen:             .skip 8
un_fen_n:           .skip 8
un_fen_top:         .skip 8
un_phase:           .skip 8
un_rumble_steps:    .skip 8
un_mod_delay:       .skip 8
un_hold:            .skip 8
un_fg:              .skip 8
un_last_fg:         .skip 8
un_symbol:          .skip 8
un_rumble_scene:    .skip 8
un_pair:            .skip 8 * 2
un_rumble_spec:     .skip 8 * 16
un_rumble_len:      .skip 8
un_final_spec:      .skip 8 * 16
un_final_len:       .skip 8
un_bg_spec:         .skip 8 * 16
un_bg_len:          .skip 8
un_cache:           .skip 8         // (symbol, fg) -> character, or 0
un_cache_mask:      .skip 8
un_cache_shift:     .skip 8
un_restore_pending: .skip 1

    .text
