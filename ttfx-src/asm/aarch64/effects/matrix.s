// effects/matrix.s - matrix (src/effects/matrix.rs).
//
// Rain columns fall until --rain-time seconds of wall clock have passed
// (clock_wall: virtual under --parity-dump / --virtual-clock), then every
// column fills and the text resolves out of it.
//
// RainColumn lives in a 128-byte record. Its pending characters are always a
// suffix of its characters (pending.remove(0) only), so they are an index.
// Its visible characters are pushed at the back at most once per setup, so a
// buffer of the column's length with start/end indices holds them; front
// pops advance the start, and resolve_char / drop_column compact in place.
// pending_columns is a ring (a column is pending at most once), active and
// full are plain arrays of column indices.
//
// Colors are compared (Color ==) by their words; the marshalling tags hex
// stops spelled unlike Color::from_rgb so that equality matches Rust's.

.equ HAVE_matrix, 1

.equ matrix_config.highlight,           0
.equ matrix_config.rain,                8       // rain gradient stops
.equ matrix_config.rain_count,          16
.equ matrix_config.symbols,             24      // packed symbols
.equ matrix_config.symbol_count,        32
.equ matrix_config.fall_min,            40      // rain_fall_delay_range
.equ matrix_config.fall_max,            48
.equ matrix_config.column_min,          56      // rain_column_delay_range
.equ matrix_config.column_max,          64
.equ matrix_config.rain_time,           72
.equ matrix_config.symbol_swap,         80      // f64
.equ matrix_config.color_swap,          88      // f64
.equ matrix_config.resolve_delay,       96
.equ matrix_config.final_stops,         104
.equ matrix_config.final_stop_count,    112
.equ matrix_config.final_steps,         120
.equ matrix_config.final_step_count,    128
.equ matrix_config.final_frames,        136
.equ matrix_config.final_direction,     144
.equ matrix_config_size,                152

// RainColumn record
.equ CO_CHARS,          0               // u32* characters, bottom to top
.equ CO_LEN,            8               // u64 len(characters)
.equ CO_PEND,           16              // u32 first pending character
.equ CO_VSTART,         20              // u32 visible[0]
.equ CO_VEND,           24              // u32 one past the last visible
.equ CO_PHASE,          28              // u32 MX_RAIN / MX_FILL
.equ CO_VIS,            32              // u32* visible buffer
.equ CO_DROP,           40              // f64 column_drop_chance
.equ CO_BASE,           48              // i64 base_rain_fall_delay
.equ CO_DELAY,          56              // i64 active_rain_fall_delay
.equ CO_LENGTH,         64              // i64 length
.equ CO_HOLD,           72              // i64 hold_time
.equ CO_IN_FULL,        80              // u8 in full_columns
.equ CO_SHIFT,          7

.equ MX_RAIN,           0
.equ MX_FILL,           1
.equ MX_RESOLVE,        2

.equ MX_SCENE_RESOLVE,  NAME_LITERAL

// matrix_build's locals
.equ MXB_SCENE,         64              // the resolve scene
.equ MXB_FG_LEN,        72              // fg spectrum length
.equ MXB_BG_LEN,        80              // bg spectrum length / final color
.equ MXB_SYMBOL,        88              // the character's symbol

    .text

// matrix_build: Matrix::build.
matrix_build:
    stp     x19, x20, [sp, #-96]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    LDX     x19, effect_config
    ldr     x9, [x19, #matrix_config.highlight]
    STX     x9, mx_highlight
    ldr     x9, [x19, #matrix_config.symbols]
    STX     x9, mx_symbols
    ldr     x9, [x19, #matrix_config.symbol_count]
    STX     x9, mx_symbol_count
    ldr     x9, [x19, #matrix_config.resolve_delay]
    STX     x9, mx_resolve_delay
    ldr     d0, [x19, #matrix_config.symbol_swap]
    bl      rng_threshold
    STX     x9, mx_symbol_swap
    ldr     d0, [x19, #matrix_config.color_swap]
    bl      rng_threshold
    STX     x9, mx_color_swap
    ADRG    x10, mx_swap_pairs
    str     x9, [x10, #8]
    str     x9, [x10, #24]
    LDX     x9, mx_symbol_swap
    str     x9, [x10]
    str     x9, [x10, #16]
    ldr     x9, [x19, #matrix_config.rain_time]
    scvtf   d0, x9
    STD     d0, mx_rain_time
    // rain_colors = Gradient(*rain_color_gradient, steps=6).spectrum
    ADRG    x0, mx_six
    mov     w3, #1
    ldr     x1, [x19, #matrix_config.rain_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, mx_rain
    mov     x4, x9
    ldr     x0, [x19, #matrix_config.rain]
    ldr     w1, [x19, #matrix_config.rain_count]
    ADRG    x2, mx_six
    mov     w3, #1
    bl      gradient_new
    STX     x9, mx_rain_len
    // the final gradient and its coordinate mapping
    ldr     x0, [x19, #matrix_config.final_steps]
    ldr     x3, [x19, #matrix_config.final_step_count]
    ldr     x1, [x19, #matrix_config.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    mov     x21, x9
    ldr     x0, [x19, #matrix_config.final_stops]
    ldr     w1, [x19, #matrix_config.final_stop_count]
    ldr     x2, [x19, #matrix_config.final_steps]
    ldr     w3, [x19, #matrix_config.final_step_count]
    mov     x4, x21
    bl      gradient_new
    mov     w22, w9
    // build_coordinate_color_mapping(text_bottom, text_top, text_left, text_right)
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    cmp     x3, #1
    b.lt    matrix_build.bad_bounds
    cmp     x5, #1
    b.lt    matrix_build.bad_bounds
    cmp     x2, #1
    b.lt    matrix_build.bad_bounds
    cmp     x4, #1
    b.lt    matrix_build.bad_bounds
    cmp     x2, x3
    b.gt    matrix_build.bad_order
    cmp     x4, x5
    b.gt    matrix_build.bad_order
    mov     x0, x21
    mov     w1, w22
    ldr     x6, [x19, #matrix_config.final_direction]
    bl      gradient_map
    mov     x23, x9                     // the map
    LDX     x24, text_right
    LDX     x9, text_left
    sub     x24, x24, x9
    add     x24, x24, #1                // its width
    // resolve scenes for the input characters, top to bottom
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x20, x9
    mov     x21, x2
    mov     x22, #0
matrix_build.character:
    cmp     x22, x21
    b.hs    matrix_build.columns
    ldr     w19, [x20, x22, lsl #2]
    mov     w0, w19
    mov     w1, #MX_SCENE_RESOLVE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     w9, [sp, #MXB_SCENE]        // the scene
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.eq    matrix_build.dynamic
    // final color from the mapping; Gradient(highlight, final, steps=8)
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x19, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    mul     x9, x9, x24
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x19, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    ldr     x9, [x23, x9, lsl #3]
    // a frame per gradient color, the visuals shared by symbol and final
    // color (the gradient only made for a new pair)
    str     x9, [sp, #MXB_BG_LEN]
    LDX     x0, ch_sym
    ldr     x0, [x0, x19, lsl #3]
    mov     x1, x9
    mov     x2, #NONE
    bl      visual_run_find
    cbnz    x9, matrix_build.frames
    ldr     x9, [sp, #MXB_BG_LEN]
    bl      resolve_gradient_fg
    LDX     x0, ch_sym
    ldr     x0, [x0, x19, lsl #3]
    ADRG    x1, mx_fg_spectrum
    mov     w2, w9
    mov     x3, #NONE
    ldr     x4, [sp, #MXB_BG_LEN]
    bl      visual_run
matrix_build.frames:
    ldr     w0, [sp, #MXB_SCENE]
    mov     x1, x9
    LDX     x3, effect_config
    ldr     w3, [x3, #matrix_config.final_frames]
    bl      visual_frames
    b       matrix_build.next_character
matrix_build.dynamic:
    // ColorPair(input fg, input bg): gradients from the highlight to each
    str     xzr, [sp, #MXB_FG_LEN]      // fg spectrum length
    str     xzr, [sp, #MXB_BG_LEN]      // bg spectrum length
    LDX     x9, ch_fg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    matrix_build.dynamic_bg
    bl      resolve_gradient_fg
    str     x9, [sp, #MXB_FG_LEN]
matrix_build.dynamic_bg:
    LDX     x9, ch_bg
    ldr     x9, [x9, x19, lsl #3]
    cmn     x9, #1                      // NONE
    b.eq    matrix_build.dynamic_frames
    ADRG    x0, mx_pair
    LDX     x3, mx_highlight
    str     x3, [x0]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, mx_eight
    mov     w3, #1
    ADRG    x4, mx_bg_spectrum
    bl      gradient_new
    str     x9, [sp, #MXB_BG_LEN]
matrix_build.dynamic_frames:
    LDX     x9, ch_sym
    ldr     x9, [x9, x19, lsl #3]
    str     x9, [sp, #MXB_SYMBOL]
    LDX     x9, effect_config
    ldr     w3, [x9, #matrix_config.final_frames]
    ldr     x9, [sp, #MXB_FG_LEN]
    ldr     x10, [sp, #MXB_BG_LEN]
    orr     x9, x9, x10
    cbz     x9, matrix_build.dynamic_plain
    ldr     w0, [sp, #MXB_SCENE]
    add     x1, sp, #MXB_SYMBOL
    mov     w2, #1
    mov     x4, #0
    ldr     x5, [sp, #MXB_FG_LEN]
    cbz     x5, matrix_build.no_fg
    ADRG    x4, mx_fg_spectrum
matrix_build.no_fg:
    mov     x6, #0
    ldr     x7, [sp, #MXB_BG_LEN]
    cbz     x7, matrix_build.no_bg
    ADRG    x6, mx_bg_spectrum
matrix_build.no_bg:
    bl      scene_apply_gradient
    b       matrix_build.next_character
matrix_build.dynamic_plain:
    ldr     w0, [sp, #MXB_SCENE]
    ldr     x1, [sp, #MXB_SYMBOL]
    mov     w2, w3
    mov     x3, #NONE
    mov     x4, #NONE
    mov     w5, #0
    bl      scene_add_frame
matrix_build.next_character:
    add     x22, x22, #1
    b       matrix_build.character
matrix_build.columns:
    // one RainColumn per canvas column, left to right, bottom to top
    mov     w0, #(FILTER_INPUT | FILTER_INNER_FILL | FILTER_OUTER_FILL)
    mov     w1, #GROUP_COLUMN_L2R
    bl      get_characters_grouped
    mov     x20, x9
    mov     x21, x2
    STX     x2, mx_column_count
    lsl     x0, x21, #CO_SHIFT
    bl      alloc
    STX     x9, mx_columns
    lsl     x0, x21, #2
    add     x0, x0, #4
    bl      alloc
    STX     x9, mx_pending
    lsl     x0, x21, #2
    add     x0, x0, #4
    bl      alloc
    STX     x9, mx_active
    lsl     x0, x21, #2
    add     x0, x0, #4
    bl      alloc
    STX     x9, mx_full
    mov     x22, #0
matrix_build.column:
    cmp     x22, x21
    b.hs    matrix_build.shuffle
    LDX     x19, mx_columns
    add     x19, x19, x22, lsl #CO_SHIFT
    add     x0, x20, x22, lsl #4
    ldr     x1, [x0, #8]                // count
    ldr     x0, [x0]                    // slots
    str     x0, [x19, #CO_CHARS]
    str     x1, [x19, #CO_LEN]
    // column_chars.reverse()
    add     x2, x0, x1, lsl #2
    sub     x2, x2, #4
matrix_build.reverse:
    cmp     x0, x2
    b.hs    matrix_build.reversed
    ldr     w9, [x0]
    ldr     w3, [x2]
    str     w3, [x0]
    str     w9, [x2]
    add     x0, x0, #4
    sub     x2, x2, #4
    b       matrix_build.reverse
matrix_build.reversed:
    lsl     x0, x1, #2
    bl      alloc
    str     x9, [x19, #CO_VIS]
    LDX     x9, mx_drop_chance
    str     x9, [x19, #CO_DROP]
    mov     x0, x19
    mov     w1, #MX_RAIN
    bl      setup_column
    LDX     x9, mx_pending
    str     w22, [x9, x22, lsl #2]
    add     x22, x22, #1
    b       matrix_build.column
matrix_build.shuffle:
    STX     x21, mx_pending_count
    LDX     x0, mx_pending
    mov     x1, x21
    bl      rng_shuffle32
    // rain_start = time.time(), after build
    bl      clock_wall
    STD     d0, mx_rain_start
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #96
    ret
matrix_build.bad_bounds:
    FAIL    msg_mx_bounds
matrix_build.bad_order:
    FAIL    msg_mx_order

// resolve_gradient_fg(x9=color) -> x9 = length of
// Gradient(highlight, color, steps=8) in mx_fg_spectrum.
resolve_gradient_fg:
    ADRG    x0, mx_pair
    LDX     x3, mx_highlight
    str     x3, [x0]
    str     x9, [x0, #8]
    mov     w1, #2
    ADRG    x2, mx_eight
    mov     w3, #1
    ADRG    x4, mx_fg_spectrum
    b       gradient_new

// setup_column(x0=column, w1=MX_RAIN/MX_FILL): RainColumn.setup_column.
setup_column:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     x19, x0
    str     w1, [x19, #CO_PHASE]
    mov     x21, #0
setup_column.hide:
    ldr     x9, [x19, #CO_LEN]
    cmp     x21, x9
    b.hs    setup_column.hidden
    ldr     x9, [x19, #CO_CHARS]
    ldr     w22, [x9, x21, lsl #2]
    mov     w0, w22
    mov     w1, #0
    bl      set_visibility
    mov     w0, w22
    bl      char_input_coord
    mov     x1, x9
    mov     w0, w22
    bl      set_coordinate
    add     x21, x21, #1
    b       setup_column.hide
setup_column.hidden:
    str     wzr, [x19, #CO_PEND]
    str     wzr, [x19, #CO_VSTART]
    str     wzr, [x19, #CO_VEND]
    LDX     x9, effect_config
    ldr     x0, [x9, #matrix_config.fall_min]
    ldr     x1, [x9, #matrix_config.fall_max]
    ldr     w9, [x19, #CO_PHASE]
    cmp     w9, #MX_FILL
    b.ne    setup_column.delay
    // max(floor_div(bound, 3), 1); the bounds are positive
    mov     x3, #3
    sdiv    x9, x0, x3
    mov     x0, #1
    cmp     x9, x0
    csel    x0, x9, x0, gt
    sdiv    x9, x1, x3
    mov     x1, #1
    cmp     x9, x1
    csel    x1, x9, x1, gt
setup_column.delay:
    bl      rng_randint
    str     x9, [x19, #CO_BASE]
    str     xzr, [x19, #CO_DELAY]
    ldr     x1, [x19, #CO_LEN]
    mov     x9, x1
    ldr     w3, [x19, #CO_PHASE]
    cmp     w3, #MX_RAIN
    b.ne    setup_column.length
    // randint(max(1, int(len * 0.1)), len)
    scvtf   d0, x1
    LDD     d1, mx_tenth
    fmul    d0, d0, d1
    fcvtzs  x0, d0
    mov     x9, #1
    cmp     x0, x9
    csel    x0, x9, x0, lt
    bl      rng_randint
setup_column.length:
    str     x9, [x19, #CO_LENGTH]
    str     xzr, [x19, #CO_HOLD]
    ldr     x3, [x19, #CO_LEN]
    cmp     x9, x3
    b.ne    setup_column.done
    mov     x0, #20
    mov     x1, #45
    bl      rng_randint
    str     x9, [x19, #CO_HOLD]
setup_column.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// recolor(w0=slot, x1=fg): set_appearance(slot, current symbol, (fg, None)).
recolor:
    str     x30, [sp, #-16]!
    LDX     x9, ch_handle
    ldr     w9, [x9, w0, uxtw #2]
    bl      visual_meta                 // keeps x0, x1
    ldr     x30, [sp], #16
    mov     x2, x1
    ldr     x1, [x9, #VH_SYMBOL]
    mov     x3, #NONE
    b       set_appearance

// rain_choice -> x9 = random.choice(rain_colors).
rain_choice:
    str     x30, [sp, #-16]!
    LDX     x0, mx_rain_len
    bl      rng_below
    LDX     x3, mx_rain
    ldr     x9, [x3, x9, lsl #3]
    ldr     x30, [sp], #16
    ret

// trim_column(x19=column): RainColumn.trim_column. Preserves x19.
trim_column:
    str     x30, [sp, #-16]!            // [sp, #8]: the tail start
    ldr     w9, [x19, #CO_VSTART]
    ldr     w3, [x19, #CO_VEND]
    cmp     w9, w3
    b.eq    trim_column.done
    ldr     x3, [x19, #CO_VIS]
    ldr     w0, [x3, x9, lsl #2]
    add     w9, w9, #1
    str     w9, [x19, #CO_VSTART]
    mov     w1, #0
    bl      set_visibility
    ldr     w9, [x19, #CO_VEND]
    ldr     w3, [x19, #CO_VSTART]
    sub     w9, w9, w3
    cmp     w9, #1
    b.ls    trim_column.done
    // fade_last_character: random.choice(rain_colors[-3:]) at 0.65
    LDX     x0, mx_rain_len
    sub     x1, x0, #3
    cmp     x1, #0
    csel    x1, xzr, x1, lt             // tail start
    sub     x0, x0, x1
    str     x1, [sp, #8]
    bl      rng_below
    ldr     x1, [sp, #8]
    add     x9, x9, x1
    LDX     x3, mx_rain
    ldr     x0, [x3, x9, lsl #3]
    LDD     d0, mx_fade
    bl      adjust_color_brightness
    mov     x1, x9
    ldr     w9, [x19, #CO_VSTART]
    ldr     x3, [x19, #CO_VIS]
    ldr     w0, [x3, x9, lsl #2]
    ldr     x30, [sp], #16
    b       recolor
trim_column.done:
    ldr     x30, [sp], #16
    ret

// drop_column(x19=column): RainColumn.drop_column - every visible character
// moves down a row; those leaving the canvas are hidden and dropped.
drop_column:
    PUSH2   x21, x22
    PUSH2   x23, x30
    ldr     w21, [x19, #CO_VSTART]      // read
    mov     w22, w21                    // write
drop_column.each:
    ldr     w9, [x19, #CO_VEND]
    cmp     w21, w9
    b.hs    drop_column.done
    ldr     x9, [x19, #CO_VIS]
    ldr     w23, [x9, x21, lsl #2]
    LDX     x9, ch_row
    ldr     w1, [x9, x23, lsl #2]
    sub     w1, w1, #1
    lsl     x1, x1, #32
    LDX     x9, ch_col
    ldr     w9, [x9, x23, lsl #2]
    orr     x1, x1, x9
    mov     w0, w23
    bl      set_coordinate
    LDX     x9, ch_row
    ldr     w9, [x9, x23, lsl #2]
    cmp     w9, #1                      // canvas.bottom
    b.ge    drop_column.keep
    mov     w0, w23
    mov     w1, #0
    bl      set_visibility
    b       drop_column.next
drop_column.keep:
    ldr     x9, [x19, #CO_VIS]
    str     w23, [x9, x22, lsl #2]
    add     w22, w22, #1
drop_column.next:
    add     w21, w21, #1
    b       drop_column.each
drop_column.done:
    str     w22, [x19, #CO_VEND]
    POP2    x23, x30
    POP2    x21, x22
    ret

// resolve_char(x19=column) -> w9 = slot: RainColumn.resolve_char.
resolve_char:
    str     x30, [sp, #-16]!
    ldr     w0, [x19, #CO_VEND]
    ldr     w3, [x19, #CO_VSTART]
    sub     w0, w0, w3
    bl      rng_below                   // randint(0, len - 1)
    ldr     w3, [x19, #CO_VSTART]
    add     w3, w3, w9                  // the removed position
    ldr     x2, [x19, #CO_VIS]
    ldr     w9, [x2, x3, lsl #2]
    // close the gap from the tail
    ldr     w1, [x19, #CO_VEND]
    sub     w1, w1, #1
    str     w1, [x19, #CO_VEND]
resolve_char.shift:
    cmp     w3, w1
    b.hs    resolve_char.done
    add     x4, x2, x3, lsl #2
    ldr     w0, [x4, #4]
    str     w0, [x4]
    add     w3, w3, #1
    b       resolve_char.shift
resolve_char.done:
    ldr     x30, [sp], #16
    ret

// column_tick(x0=column): RainColumn.tick.
column_tick:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     x19, x0
    ldr     x9, [x19, #CO_DELAY]
    cbnz    x9, column_tick.wait
    ldr     w9, [x19, #CO_PEND]
    ldr     x3, [x19, #CO_LEN]
    cmp     x9, x3
    b.hs    column_tick.no_pending
    // the next pending character appears, highlighted
    ldr     x3, [x19, #CO_CHARS]
    ldr     w21, [x3, x9, lsl #2]
    add     w9, w9, #1
    str     w9, [x19, #CO_PEND]
    LDX     x0, mx_symbol_count
    bl      rng_below
    LDX     x3, mx_symbols
    ldr     x1, [x3, x9, lsl #3]
    mov     w0, w21
    LDX     x2, mx_highlight
    mov     x3, #NONE
    bl      set_appearance
    // the previous bottom character loses the highlight
    ldr     w9, [x19, #CO_VEND]
    ldr     w3, [x19, #CO_VSTART]
    cmp     w9, w3
    b.eq    column_tick.show
    bl      rain_choice
    mov     x1, x9
    ldr     w9, [x19, #CO_VEND]
    ldr     x3, [x19, #CO_VIS]
    add     x3, x3, x9, lsl #2
    ldur    w0, [x3, #-4]
    bl      recolor
column_tick.show:
    mov     w0, w21
    bl      set_visible
    ldr     w9, [x19, #CO_VEND]
    ldr     x3, [x19, #CO_VIS]
    str     w21, [x3, x9, lsl #2]
    add     w9, w9, #1
    str     w9, [x19, #CO_VEND]
    b       column_tick.trim
column_tick.no_pending:
    ldr     w9, [x19, #CO_VEND]
    ldr     w3, [x19, #CO_VSTART]
    cmp     w9, w3
    b.eq    column_tick.trim
    // the bottom character loses the highlight
    ldr     x3, [x19, #CO_VIS]
    add     x3, x3, x9, lsl #2
    ldur    w21, [x3, #-4]
    LDX     x9, ch_handle
    ldr     w9, [x9, x21, lsl #2]
    bl      visual_meta
    ldr     x3, [x9, #VH_FG]
    LDX     x10, mx_highlight
    cmp     x3, x10
    b.ne    column_tick.hold
    bl      rain_choice
    mov     x1, x9
    mov     w0, w21
    bl      recolor
column_tick.hold:
    ldr     x9, [x19, #CO_HOLD]
    cbz     x9, column_tick.drain
    sub     x9, x9, #1
    str     x9, [x19, #CO_HOLD]
    b       column_tick.trim
column_tick.drain:
    ldr     w9, [x19, #CO_PHASE]
    cmp     w9, #MX_RAIN
    b.ne    column_tick.trim
    bl      rng_random
    ldr     d1, [x19, #CO_DROP]
    fcmp    d0, d1
    b.ge    column_tick.no_drop
    bl      drop_column
column_tick.no_drop:
    bl      trim_column
column_tick.trim:
    ldr     w9, [x19, #CO_VEND]
    ldr     w3, [x19, #CO_VSTART]
    sub     w9, w9, w3
    ldr     x3, [x19, #CO_LENGTH]
    cmp     x9, x3
    b.ls    column_tick.rearm
    bl      trim_column
column_tick.rearm:
    ldr     x9, [x19, #CO_BASE]
    str     x9, [x19, #CO_DELAY]
    b       column_tick.swap
column_tick.wait:
    sub     x9, x9, #1
    str     x9, [x19, #CO_DELAY]
column_tick.swap:
    // randomly change the symbol and/or color of the visible characters
    ldr     w21, [x19, #CO_VSTART]
column_tick.each:
    ldr     w9, [x19, #CO_VEND]
    cmp     w21, w9
    b.hs    column_tick.done
    // Skip, straight from the RNG batch, the characters whose two draws both
    // miss: they draw nothing else and change nothing. The draw sequence is
    // the same; only the per-draw bookkeeping goes.
    LDX     x3, rng_pos
    mov     x2, #(RNG_BATCH - 1)
    subs    x2, x2, x3                  // draws left in the batch, minus one
    b.le    column_tick.single          // fewer than a pair: draw one by one
    ldr     w4, [x19, #CO_VEND]
    sub     w4, w4, w21                 // characters left
    ADRG    x5, rng_buf
    add     x5, x5, x3, lsl #3          // next draw
    add     x10, x5, x2, lsl #3         // last pair starts before this
    add     x4, x5, x4, lsl #4          // end of the characters' draws
    cmp     x4, x10
    csel    x4, x10, x4, hi
    LDX     x10, mx_symbol_swap
    LDX     x11, mx_color_swap
    mov     x0, x5
column_tick.skip:
    cmp     x0, x4
    b.hs    column_tick.skipped
    ldr     x9, [x0]
    lsr     x9, x9, #11
    cmp     x9, x10
    b.lo    column_tick.skipped
    ldr     x9, [x0, #8]
    lsr     x9, x9, #11
    cmp     x9, x11
    b.lo    column_tick.skipped
    add     x0, x0, #16
    b       column_tick.skip
column_tick.skipped:
    sub     x0, x0, x5
    lsr     x0, x0, #3                  // draws skipped
    add     x3, x3, x0
    STX     x3, rng_pos
    lsr     w0, w0, #1
    add     w21, w21, w0                // characters skipped
    ldr     w9, [x19, #CO_VEND]
    cmp     w21, w9
    b.hs    column_tick.done
    cbz     x0, column_tick.single
    b       column_tick.each            // batch ran out, or a hit next
column_tick.single:
    mov     x22, #0                     // next symbol, 0 = none
    mov     x23, #NONE                  // next color, NONE = none
    RNG_BITS53
    LDX     x3, mx_symbol_swap
    cmp     x9, x3
    b.hs    column_tick.color
    LDX     x0, mx_symbol_count
    bl      rng_below
    LDX     x3, mx_symbols
    ldr     x22, [x3, x9, lsl #3]
column_tick.color:
    RNG_BITS53
    LDX     x3, mx_color_swap
    cmp     x9, x3
    b.hs    column_tick.chosen
    bl      rain_choice
    mov     x23, x9
column_tick.chosen:
    cbnz    x22, column_tick.compare
    cmn     x23, #1                     // NONE
    b.eq    column_tick.next
column_tick.compare:
    ldr     x9, [x19, #CO_VIS]
    ldr     w24, [x9, x21, lsl #2]
    LDX     x9, ch_handle
    ldr     w9, [x9, x24, lsl #2]
    bl      visual_meta
    ldr     x1, [x9, #VH_SYMBOL]
    ldr     x2, [x9, #VH_FG]
    mov     w20, #0                     // nonzero once something differs
    cbz     x22, column_tick.same_symbol
    cmp     x22, x1
    cset    w20, ne
    mov     x1, x22
column_tick.same_symbol:
    cmn     x23, #1                     // NONE
    b.eq    column_tick.same_color
    cmp     x23, x2
    cset    w9, ne
    orr     w20, w20, w9
    mov     x2, x23
column_tick.same_color:
    cbz     w20, column_tick.next
    mov     w0, w24
    mov     x3, #NONE
    bl      set_appearance
column_tick.next:
    add     w21, w21, #1
    b       column_tick.each
column_tick.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// MX_COLUMN_PTR: x9 = the record of column index w9 (upper half zero).
.macro MX_COLUMN_PTR
    LDX     x16, mx_columns
    add     x9, x16, x9, lsl #CO_SHIFT
.endm

// pending_pop -> w9 = pending_columns.pop(0). Clobbers x3, x2.
pending_pop:
    LDX     x3, mx_pending
    LDX     x2, mx_pending_head
    ldr     w9, [x3, x2, lsl #2]
    add     x2, x2, #1
    LDX     x16, mx_column_count
    cmp     x2, x16
    b.lo    pending_pop.keep
    mov     x2, #0
pending_pop.keep:
    STX     x2, mx_pending_head
    LDX     x3, mx_pending_count
    sub     x3, x3, #1
    STX     x3, mx_pending_count
    ret

// pending_push(w0=column index). Clobbers x9, x3, x2.
pending_push:
    LDX     x9, mx_pending_head
    LDX     x3, mx_pending_count
    add     x9, x9, x3
    LDX     x2, mx_column_count
    cmp     x9, x2
    b.lo    pending_push.put
    sub     x9, x9, x2
pending_push.put:
    LDX     x3, mx_pending
    str     w0, [x3, x9, lsl #2]
    LDX     x3, mx_pending_count
    add     x3, x3, #1
    STX     x3, mx_pending_count
    ret

// activate_pending: active_columns.append(pending_columns.pop(0)).
activate_pending:
    str     x30, [sp, #-16]!
    bl      pending_pop
    ldr     x30, [sp], #16
    LDX     x3, mx_active
    LDX     x2, mx_active_count
    str     w9, [x3, x2, lsl #2]
    add     x2, x2, #1
    STX     x2, mx_active_count
    ret

// retain_visible(x0=u32 column list, x1=count) -> x9 = new count: keep
// the columns that still have visible characters, in order.
retain_visible:
    mov     x9, #0
    mov     x3, #0
retain_visible.each:
    cmp     x3, x1
    b.hs    retain_visible.done
    ldr     w2, [x0, x3, lsl #2]
    LDX     x4, mx_columns
    add     x4, x4, x2, lsl #CO_SHIFT
    ldr     w5, [x4, #CO_VEND]
    ldr     w10, [x4, #CO_VSTART]
    cmp     w5, w10
    b.eq    retain_visible.drop
    str     w2, [x0, x9, lsl #2]
    add     x9, x9, #1
    b       retain_visible.next
retain_visible.drop:
    strb    wzr, [x4, #CO_IN_FULL]
retain_visible.next:
    add     x3, x3, #1
    b       retain_visible.each
retain_visible.done:
    ret

// matrix_next_frame -> w9: Matrix::next_frame.
matrix_next_frame:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    LDX     x9, mx_phase
    cmp     x9, #MX_RESOLVE
    b.eq    matrix_next_frame.resolve
    // columns join the rain
    LDX     x9, mx_column_delay
    cbnz    x9, matrix_next_frame.column_wait
    LDX     x9, mx_phase
    cmp     x9, #MX_RAIN
    b.ne    matrix_next_frame.fill_all
    mov     x0, #1
    mov     x1, #3
    bl      rng_randint
    mov     x21, x9
matrix_next_frame.join:
    cmp     x21, #0
    b.le    matrix_next_frame.joined
    LDX     x9, mx_pending_count
    cbz     x9, matrix_next_frame.join_next
    bl      activate_pending
matrix_next_frame.join_next:
    sub     x21, x21, #1
    b       matrix_next_frame.join
matrix_next_frame.joined:
    LDX     x9, effect_config
    ldr     x0, [x9, #matrix_config.column_min]
    ldr     x1, [x9, #matrix_config.column_max]
    bl      rng_randint
    STX     x9, mx_column_delay
    b       matrix_next_frame.tick_active
matrix_next_frame.fill_all:
    LDX     x9, mx_pending_count
    cbz     x9, matrix_next_frame.fill_joined
    bl      activate_pending
    b       matrix_next_frame.fill_all
matrix_next_frame.fill_joined:
    mov     x9, #1
    STX     x9, mx_column_delay
    b       matrix_next_frame.tick_active
matrix_next_frame.column_wait:
    sub     x9, x9, #1
    STX     x9, mx_column_delay
matrix_next_frame.tick_active:
    mov     x21, #0
    LDX     x22, mx_active_count
matrix_next_frame.active:
    cmp     x21, x22
    b.hs    matrix_next_frame.active_done
    LDX     x9, mx_active
    ldr     w9, [x9, x21, lsl #2]
    mov     w19, w9
    MX_COLUMN_PTR
    mov     x0, x9
    bl      column_tick
    mov     w9, w19
    MX_COLUMN_PTR
    ldr     w3, [x9, #CO_PEND]
    ldr     x2, [x9, #CO_LEN]
    cmp     x3, x2
    b.lo    matrix_next_frame.active_next
    ldr     w3, [x9, #CO_PHASE]
    cmp     w3, #MX_FILL
    b.ne    matrix_next_frame.maybe_reset
    ldrb    w3, [x9, #CO_IN_FULL]
    cbnz    w3, matrix_next_frame.maybe_reset
    mov     w3, #1
    strb    w3, [x9, #CO_IN_FULL]
    LDX     x3, mx_full
    LDX     x2, mx_full_count
    str     w19, [x3, x2, lsl #2]
    add     x2, x2, #1
    STX     x2, mx_full_count
    b       matrix_next_frame.active_next
matrix_next_frame.maybe_reset:
    ldr     w3, [x9, #CO_VEND]
    ldr     w2, [x9, #CO_VSTART]
    cmp     w3, w2
    b.ne    matrix_next_frame.active_next
    mov     x0, x9
    LDW     w1, mx_phase                // column_phase_for (rain or fill)
    bl      setup_column
    mov     w0, w19
    bl      pending_push
matrix_next_frame.active_next:
    add     x21, x21, #1
    b       matrix_next_frame.active
matrix_next_frame.active_done:
    LDX     x0, mx_active
    LDX     x1, mx_active_count
    bl      retain_visible
    STX     x9, mx_active_count
    // fill done: every column is full
    LDX     x9, mx_phase
    cmp     x9, #MX_FILL
    b.ne    matrix_next_frame.deadline
    LDX     x9, mx_pending_count
    cbnz    x9, matrix_next_frame.deadline
    mov     x21, #0
matrix_next_frame.all_full:
    LDX     x9, mx_active_count
    cmp     x21, x9
    b.hs    matrix_next_frame.to_resolve
    LDX     x9, mx_active
    ldr     w9, [x9, x21, lsl #2]
    MX_COLUMN_PTR
    ldr     w3, [x9, #CO_PEND]
    ldr     x2, [x9, #CO_LEN]
    cmp     x3, x2
    b.lo    matrix_next_frame.deadline
    ldr     w3, [x9, #CO_PHASE]
    cmp     w3, #MX_FILL
    b.ne    matrix_next_frame.deadline
    add     x21, x21, #1
    b       matrix_next_frame.all_full
matrix_next_frame.to_resolve:
    mov     x9, #MX_RESOLVE
    STX     x9, mx_phase
    STX     xzr, mx_active_count
matrix_next_frame.deadline:
    // effect_matrix.py:549 - the rain deadline on the wall clock
    LDX     x9, mx_phase
    cmp     x9, #MX_RAIN
    b.ne    matrix_next_frame.emit
    LDX     x9, effect_config
    ldr     x9, [x9, #matrix_config.rain_time]
    cmp     x9, #0
    b.le    matrix_next_frame.emit
    bl      clock_wall
    LDD     d1, mx_rain_start
    fsub    d0, d0, d1
    LDD     d1, mx_rain_time
    fcmp    d0, d1
    b.le    matrix_next_frame.emit      // also unordered
    mov     w9, #1
    STB     w9, mx_rain_complete
    mov     x9, #MX_FILL
    STX     x9, mx_phase
    mov     x21, #0
matrix_next_frame.drain_active:
    LDX     x9, mx_active_count
    cmp     x21, x9
    b.hs    matrix_next_frame.fill_pending
    LDX     x9, mx_active
    ldr     w9, [x9, x21, lsl #2]
    MX_COLUMN_PTR
    str     xzr, [x9, #CO_HOLD]
    LDX     x3, mx_one
    str     x3, [x9, #CO_DROP]
    add     x21, x21, #1
    b       matrix_next_frame.drain_active
matrix_next_frame.fill_pending:
    mov     x21, #0
matrix_next_frame.fill_each:
    LDX     x9, mx_pending_count
    cmp     x21, x9
    b.hs    matrix_next_frame.emit
    LDX     x9, mx_pending_head
    add     x9, x9, x21
    LDX     x3, mx_column_count
    cmp     x9, x3
    b.lo    matrix_next_frame.fill_index
    sub     x9, x9, x3
matrix_next_frame.fill_index:
    LDX     x3, mx_pending
    ldr     w9, [x3, x9, lsl #2]
    MX_COLUMN_PTR
    mov     x0, x9
    mov     w1, #MX_FILL
    bl      setup_column
    add     x21, x21, #1
    b       matrix_next_frame.fill_each

matrix_next_frame.resolve:
    mov     x21, #0
    LDX     x22, mx_full_count
matrix_next_frame.full:
    cmp     x21, x22
    b.hs    matrix_next_frame.full_done
    LDX     x9, mx_full
    ldr     w9, [x9, x21, lsl #2]
    MX_COLUMN_PTR
    mov     x19, x9
    mov     x0, x9
    bl      column_tick
    ldr     w9, [x19, #CO_VEND]
    ldr     w3, [x19, #CO_VSTART]
    cmp     w9, w3
    b.eq    matrix_next_frame.full_next
    LDX     x9, mx_resolve_delay
    cbnz    x9, matrix_next_frame.resolve_wait
    mov     x0, #1
    mov     x1, #4
    bl      rng_randint
    mov     x23, x9
matrix_next_frame.resolve_each:
    cmp     x23, #0
    b.le    matrix_next_frame.resolved
    ldr     w9, [x19, #CO_VEND]
    ldr     w3, [x19, #CO_VSTART]
    cmp     w9, w3
    b.eq    matrix_next_frame.resolve_next
    bl      resolve_char
    mov     w24, w9
    LDX     x3, ch_sym
    ldr     x3, [x3, x24, lsl #3]
    MOV64   x2, ((1 << 32) | 0x20)      // ' '
    cmp     x3, x2
    b.ne    matrix_next_frame.activate
    LDX     x3, ch_fg
    ldr     x3, [x3, x24, lsl #3]
    cmn     x3, #1                      // NONE
    b.ne    matrix_next_frame.activate
    LDX     x3, ch_bg
    ldr     x3, [x3, x24, lsl #3]
    cmn     x3, #1                      // NONE
    b.ne    matrix_next_frame.activate
    mov     w0, w24
    mov     w1, #0
    bl      set_visibility
    b       matrix_next_frame.resolve_next
matrix_next_frame.activate:
    mov     w0, w24
    mov     w1, #MX_SCENE_RESOLVE
    bl      scene_activate_name
    mov     w0, w24
    bl      active_insert
matrix_next_frame.resolve_next:
    sub     x23, x23, #1
    b       matrix_next_frame.resolve_each
matrix_next_frame.resolved:
    LDX     x9, effect_config
    ldr     x9, [x9, #matrix_config.resolve_delay]
    STX     x9, mx_resolve_delay
    b       matrix_next_frame.full_next
matrix_next_frame.resolve_wait:
    sub     x9, x9, #1
    STX     x9, mx_resolve_delay
matrix_next_frame.full_next:
    add     x21, x21, #1
    b       matrix_next_frame.full
matrix_next_frame.full_done:
    LDX     x0, mx_full
    LDX     x1, mx_full_count
    bl      retain_visible
    STX     x9, mx_full_count

matrix_next_frame.emit:
    LDX     x9, mx_full_count
    cbnz    x9, matrix_next_frame.frame
    LDX     x9, mx_active_count
    cbnz    x9, matrix_next_frame.frame
    LDX     x9, mx_pending_count
    cbnz    x9, matrix_next_frame.frame
    LDB     w9, mx_rain_complete
    cbz     w9, matrix_next_frame.frame
    bl      active_empty
    cbz     w9, matrix_next_frame.frame
    LDB     w9, mx_final_shown
    cbnz    w9, matrix_next_frame.finished
    mov     w9, #1
    STB     w9, mx_final_shown
matrix_next_frame.frame:
    bl      update
    mov     w9, #1
    b       matrix_next_frame.out
matrix_next_frame.finished:
    mov     w9, #0
matrix_next_frame.out:
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

    .section .rodata
    .balign 8
mx_six:         .quad 6
mx_eight:       .quad 8
mx_tenth:       .double 0.1
mx_fade:        .double 0.65
mx_drop_chance: .double 0.08
mx_one:         .double 1.0
STRING msg_mx_bounds, "max_row and max_column must be greater than 0."
STRING msg_mx_order, "min_row and min_column must be less than or equal to max_row and max_column."

    TSTATE
    .balign 8
mx_highlight:       .skip 8
mx_symbols:         .skip 8
mx_symbol_count:    .skip 8
mx_rain:            .skip 8
mx_rain_len:        .skip 8
mx_rain_time:       .skip 8             // f64
mx_rain_start:      .skip 8             // f64
mx_symbol_swap:     .skip 8             // rng_threshold of symbol_swap_chance
mx_color_swap:      .skip 8             // rng_threshold of color_swap_chance
mx_swap_pairs:      .skip 8 * 4         // symbol, color, symbol, color
mx_columns:         .skip 8
mx_column_count:    .skip 8
mx_pending:         .skip 8
mx_pending_head:    .skip 8
mx_pending_count:   .skip 8
mx_active:          .skip 8
mx_active_count:    .skip 8
mx_full:            .skip 8
mx_full_count:      .skip 8
mx_column_delay:    .skip 8
mx_resolve_delay:   .skip 8
mx_phase:           .skip 8
mx_pair:            .skip 8 * 2
mx_fg_spectrum:     .skip 8 * 16
mx_bg_spectrum:     .skip 8 * 16
mx_rain_complete:   .skip 1
mx_final_shown:     .skip 1

    .text
