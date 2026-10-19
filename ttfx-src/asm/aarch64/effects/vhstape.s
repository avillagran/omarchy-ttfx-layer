// effects/vhstape.s - "Lines of characters glitch left and right and lose
// detail like an old VHS tape" (src/effects/vhstape.rs).
//
// Config (src/asm/effects.rs, EffectCommand::Vhstape). --glitch-wave-colors
// is accepted by the CLI but never read by the effect, so it is not passed.
//
// Lines are the row groups of get_characters_grouped (bottom to top); a line
// index is a group index. The glitch wave and glitch line lists hold at most
// three indices each.

.equ HAVE_vhstape, 1

.equ VHSTAPE.line_colors,       0       // *const u64 glitch_line_colors
.equ VHSTAPE.line_count,        8
.equ VHSTAPE.noise_colors,      16      // *const u64
.equ VHSTAPE.noise_count,       24
.equ VHSTAPE.line_chance,       32      // f64 glitch_line_chance
.equ VHSTAPE.noise_chance,      40      // f64
.equ VHSTAPE.total_time,        48      // total_glitch_time (frames)
.equ VHSTAPE.final_stops,       56      // *const u64
.equ VHSTAPE.final_stop_count,  64
.equ VHSTAPE.final_steps,       72      // *const i64
.equ VHSTAPE.final_step_count,  80
.equ VHSTAPE.final_direction,   88
.equ VHSTAPE_size,              96

// path names (each path's single waypoint shares its path's name)
.equ VHS_GLITCH,            NAME_LITERAL + 0
.equ VHS_RESTORE,           NAME_LITERAL + 1
.equ VHS_WAVE_MID,          NAME_LITERAL + 2
.equ VHS_WAVE_END,          NAME_LITERAL + 3
// scene names
.equ VHS_BASE,              NAME_LITERAL + 4
.equ VHS_GLITCH_FWD,        NAME_LITERAL + 5
.equ VHS_GLITCH_BWD,        NAME_LITERAL + 6
.equ VHS_SNOW,              NAME_LITERAL + 7
.equ VHS_FINAL_SNOW,        NAME_LITERAL + 8
.equ VHS_FINAL_REDRAW,      NAME_LITERAL + 9

// ch_user0: the glitch path index (low half) and the base scene index (high
// half). A character's paths and scenes are created back to back, so the
// others follow at fixed distances.
.equ VHS_P_RESTORE,         1
.equ VHS_P_MID,             2
.equ VHS_P_END,             3
.equ VHS_S_SNOW,            3
.equ VHS_S_FINAL_SNOW,      4
.equ VHS_S_FINAL_REDRAW,    5

.equ VHS_PH_GLITCHING,      0
.equ VHS_PH_NOISE,          1
.equ VHS_PH_REDRAW,         2
.equ VHS_PH_COMPLETE,       3

    .text

// vhstape_build: VhsTape::build.
vhstape_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    bl      vhs_final_color_map
    LDX     x19, effect_config
    // memo of (noise color, snow symbol) -> handle
    ldr     x0, [x19, #VHSTAPE.noise_count]
    lsl     x0, x0, #4
    bl      alloc
    STX     x9, vhs_snow_memo
    // the glitch line colors, reversed (GLITCH_BWD's frames)
    ldr     x0, [x19, #VHSTAPE.line_count]
    lsl     x0, x0, #3
    add     x0, x0, #8
    bl      alloc
    STX     x9, vhs_line_colors_rev
    ldr     x3, [x19, #VHSTAPE.line_count]
    ldr     x1, [x19, #VHSTAPE.line_colors]
    mov     x2, #0
vhstape_build.reverse:
    cbz     x3, vhstape_build.reversed
    sub     x3, x3, #1
    ldr     x4, [x1, x3, lsl #3]
    str     x4, [x9, x2, lsl #3]
    add     x2, x2, #1
    b       vhstape_build.reverse
vhstape_build.reversed:
    mov     x21, #0
vhstape_build.snow_memo:
    ldr     x9, [x19, #VHSTAPE.noise_count]
    cmp     x21, x9
    b.hs    vhstape_build.snow_memo_done
    mov     x22, #0
vhstape_build.snow_memo_symbol:
    ldr     x9, [x19, #VHSTAPE.noise_colors]
    ldr     x0, [x9, x21, lsl #3]
    mov     x1, #NONE
    ADRG    x9, vhs_snow_symbols
    ldrb    w2, [x9, x22]
    orr     x2, x2, #0x100000000        // one byte long
    mov     w3, #0
    bl      visual_make
    add     x3, x22, x21, lsl #2
    LDX     x2, vhs_snow_memo
    str     w9, [x2, x3, lsl #2]
    add     x22, x22, #1
    cmp     x22, #4
    b.lo    vhstape_build.snow_memo_symbol
    add     x21, x21, #1
    b       vhstape_build.snow_memo
vhstape_build.snow_memo_done:
    // choice(noise_colors)'s draw: the top max(bit_length(n - 1), 1) bits,
    // i.e. a shift of 64 - that = min(clz(n - 1), 63)
    ldr     x9, [x19, #VHSTAPE.noise_count]
    sub     x9, x9, #1
    clz     x9, x9
    mov     x3, #63
    cmp     x9, x3
    csel    x9, x9, x3, lo
    STB     w9, vhs_noise_shift
    // the redraw block: "█" in white
    mov     w0, #0x2588
    bl      utf8_pack
    mov     x2, x9
    mov     x0, #0xffffff
    mov     x1, #NONE
    mov     w3, #0
    bl      visual_make
    STW     w9, vhs_block_visual
    // one Line per row, bottom to top
    mov     w0, #FILTER_INPUT
    mov     w1, #GROUP_ROW_BOTTOM_TO_TOP
    bl      get_characters_grouped
    STX     x9, vhs_lines
    STX     x2, vhs_line_count
    mov     x19, #0
vhstape_build.line:
    LDX     x9, vhs_line_count
    cmp     x19, x9
    b.hs    vhstape_build.lines_built
    mov     x0, x19
    bl      vhs_line_slots
    mov     x0, x9
    mov     x1, x2
    bl      vhs_build_line_effects
    add     x19, x19, #1
    b       vhstape_build.line
vhstape_build.lines_built:
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    mov     x19, #0
vhstape_build.show:
    cmp     x19, x22
    b.hs    vhstape_build.shown
    ldr     w0, [x21, x19, lsl #2]
    mov     w20, w0
    bl      set_visible
    mov     w0, w20
    LDX     x9, ch_user0
    add     x9, x9, x20, lsl #3
    ldr     w1, [x9, #4]                // base
    bl      scene_activate
    add     x19, x19, #1
    b       vhstape_build.show
vhstape_build.shown:
    mov     w9, #VHS_PH_GLITCHING
    STB     w9, vhs_phase
    MOV64   x9, NONE_I64
    STX     x9, vhs_wave_top
    LDX     x9, vhs_line_count
    STX     x9, vhs_to_redraw
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// vhs_build_line_effects(x0=slots, x1=count): Line.build_line_effects - the
// offset, direction and hold time draws, then per character its paths,
// scenes (snow draws 25 x 2, final_snow 30 x 2) and events.
.equ VHS_L_DX,          0               // offset * direction
.equ VHS_L_HOLD,        8
.equ VHS_L_STABLE_FG,   16
.equ VHS_L_STABLE_BG,   24
.equ VHS_L_FINAL_FG,    32
.equ VHS_L_FINAL_BG,    40
.equ VHS_L_COORD,       48              // input coordinate
.equ VHS_L_FINAL_SNOW,  56
.equ VHS_L_SIZE,        64              // saved registers above

// VHS_SHIFTED delta: x1 = the input coordinate moved delta columns (a w
// register or an immediate). Clobbers x9.
.macro VHS_SHIFTED delta
    ldr     x1, [sp, #VHS_L_COORD]
    add     w9, w1, \delta
    bfi     x1, x9, #0, #32
.endm

// VHS_NEW_PATH name: path_new with speed 2.0, no easing or layer, and hold
// time x3.
.macro VHS_NEW_PATH name
    mov     w0, w20
    LDD     d0, vhs_two
    mov     w1, #NONE
    MOV64   x2, NONE_I64
    mov     w4, #0
    MOV64   w5, \name
    bl      path_new
.endm

// VHS_ADD_WAYPOINT name: path w9 gets its waypoint at x1.
.macro VHS_ADD_WAYPOINT name
    mov     w0, w9
    mov     x2, #0
    mov     w3, #0
    MOV64   w4, \name
    bl      path_new_waypoint
.endm

// VHS_EVENT event, caller kind, caller name, action, target (arg1 = 0)
.macro VHS_EVENT event, kind, caller, action, target
    mov     w0, w20
    mov     w1, #\event
    mov     w2, #\kind
    MOV64   w3, \caller
    mov     w4, #\action
    MOV64   w5, \target
    mov     x6, #0
    bl      event_register
.endm

vhs_build_line_effects:
    sub     sp, sp, #(VHS_L_SIZE + 64)
    stp     x19, x20, [sp, #VHS_L_SIZE]
    stp     x21, x22, [sp, #(VHS_L_SIZE + 16)]
    stp     x23, x24, [sp, #(VHS_L_SIZE + 32)]
    str     x30, [sp, #(VHS_L_SIZE + 48)]
    mov     x21, x0
    mov     x22, x1
    mov     x0, #4
    mov     x1, #25
    bl      rng_randint
    mov     x19, x9                     // offset
    mov     x0, #2
    bl      rng_below                   // choice([-1, 1])
    lsl     x9, x9, #1
    sub     x9, x9, #1
    mul     x19, x19, x9
    str     x19, [sp, #VHS_L_DX]
    mov     x0, #1
    mov     x1, #50
    bl      rng_randint
    str     x9, [sp, #VHS_L_HOLD]
    mov     x19, #0
vhs_build_line_effects.char:
    cmp     x19, x22
    b.hs    vhs_build_line_effects.done
    ldr     w20, [x21, x19, lsl #2]
    // stable and final colors
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    b.ne    vhs_build_line_effects.gradient
    // dynamic: the input colors, fg falling back to DYNAMIC_NEUTRAL_GRAY
    LDX     x9, ch_fg
    ldr     x9, [x9, x20, lsl #3]
    LDX     x3, ch_bg
    ldr     x3, [x3, x20, lsl #3]
    str     x9, [sp, #VHS_L_FINAL_FG]
    str     x3, [sp, #VHS_L_FINAL_BG]
    str     x3, [sp, #VHS_L_STABLE_BG]
    cmn     x9, #1                      // NONE
    b.ne    vhs_build_line_effects.dynamic_fg
    MOV64   x9, 0x808080
vhs_build_line_effects.dynamic_fg:
    str     x9, [sp, #VHS_L_STABLE_FG]
    b       vhs_build_line_effects.coord
vhs_build_line_effects.gradient:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x20, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, vhs_final_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x20, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, vhs_final_map
    ldr     x9, [x3, x9, lsl #3]
    str     x9, [sp, #VHS_L_STABLE_FG]
    str     x9, [sp, #VHS_L_FINAL_FG]
    mov     x9, #NONE
    str     x9, [sp, #VHS_L_STABLE_BG]
    str     x9, [sp, #VHS_L_FINAL_BG]
vhs_build_line_effects.coord:
    LDX     x9, ch_irow
    ldr     w9, [x9, x20, lsl #2]
    LDX     x3, ch_icol
    ldr     w3, [x3, x20, lsl #2]
    orr     x9, x3, x9, lsl #32
    str     x9, [sp, #VHS_L_COORD]
    // --- paths: glitch, restore, glitch_wave_mid, glitch_wave_end
    ldr     x3, [sp, #VHS_L_HOLD]
    VHS_NEW_PATH VHS_GLITCH
    LDX     x3, ch_user0
    add     x3, x3, x20, lsl #3
    str     w9, [x3]
    mov     w23, w9
    ldr     w12, [sp, #VHS_L_DX]
    VHS_SHIFTED w12
    mov     w9, w23
    VHS_ADD_WAYPOINT VHS_GLITCH
    mov     x3, #0
    VHS_NEW_PATH VHS_RESTORE
    ldr     x1, [sp, #VHS_L_COORD]
    VHS_ADD_WAYPOINT VHS_RESTORE
    mov     x3, #0
    VHS_NEW_PATH VHS_WAVE_MID
    mov     w23, w9
    VHS_SHIFTED #8
    mov     w9, w23
    VHS_ADD_WAYPOINT VHS_WAVE_MID
    mov     x3, #0
    VHS_NEW_PATH VHS_WAVE_END
    mov     w23, w9
    VHS_SHIFTED #14
    mov     w9, w23
    VHS_ADD_WAYPOINT VHS_WAVE_END
    // --- scenes
    mov     w0, w20
    MOV64   w1, VHS_BASE
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    LDX     x3, ch_user0
    add     x3, x3, x20, lsl #3
    str     w9, [x3, #4]
    mov     w0, w9
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    mov     w2, #1
    ldr     x3, [sp, #VHS_L_STABLE_FG]
    ldr     x4, [sp, #VHS_L_STABLE_BG]
    mov     w5, #0
    bl      scene_add_frame
    // the input symbol in each glitch line color
    LDX     x9, effect_config
    LDX     x0, ch_sym
    ldr     x0, [x0, x20, lsl #3]
    ldr     x1, [x9, #VHSTAPE.line_colors]
    ldr     x2, [x9, #VHSTAPE.line_count]
    mov     x3, #NONE
    mov     x4, #0                      // one color list
    bl      visual_run
    STX     x9, vhs_line_handles
    mov     w0, w20
    MOV64   w1, VHS_GLITCH_FWD
    mov     w2, #SCF_SYNC_STEP
    mov     w3, #NONE
    bl      scene_new
    mov     w0, w9
    LDX     x1, vhs_line_handles
    LDX     x9, effect_config
    ldr     x2, [x9, #VHSTAPE.line_count]
    mov     w3, #1
    bl      visual_frames
    // backward: the same visuals in reverse
    mov     w0, w20
    MOV64   w1, VHS_GLITCH_BWD
    mov     w2, #SCF_SYNC_STEP
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9
    LDX     x9, effect_config
    LDX     x0, ch_sym
    ldr     x0, [x0, x20, lsl #3]
    LDX     x1, vhs_line_colors_rev
    ldr     x2, [x9, #VHSTAPE.line_count]
    mov     x3, #NONE
    mov     x4, #1                      // the reversed list
    bl      visual_run
    mov     w0, w24
    mov     x1, x9
    mov     w3, #1
    bl      visual_frames
    mov     w0, w20
    MOV64   w1, VHS_SNOW
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9
    mov     w0, w9
    mov     w1, #25
    bl      vhs_snow_frames
    mov     w0, w24
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    mov     w2, #1
    ldr     x3, [sp, #VHS_L_STABLE_FG]
    ldr     x4, [sp, #VHS_L_STABLE_BG]
    mov     w5, #0
    bl      scene_add_frame
    mov     w0, w20
    MOV64   w1, VHS_FINAL_SNOW
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    str     x9, [sp, #VHS_L_FINAL_SNOW]
    mov     w0, w20
    MOV64   w1, VHS_FINAL_REDRAW
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w24, w9
    mov     w0, w9
    LDW     w1, vhs_block_visual
    mov     w2, #6
    bl      scene_add_frame_visual
    mov     w0, w24
    LDX     x1, ch_sym
    ldr     x1, [x1, x20, lsl #3]
    mov     w2, #1
    ldr     x3, [sp, #VHS_L_FINAL_FG]
    ldr     x4, [sp, #VHS_L_FINAL_BG]
    mov     w5, #0
    bl      scene_add_frame
    ldr     w0, [sp, #VHS_L_FINAL_SNOW]
    mov     w1, #30
    bl      vhs_snow_frames
    // --- events
    VHS_EVENT EV_PATH_COMPLETE, CALLER_PATH, VHS_GLITCH, ACT_ACTIVATE_PATH, VHS_RESTORE
    VHS_EVENT EV_PATH_ACTIVATED, CALLER_PATH, VHS_GLITCH, ACT_ACTIVATE_SCENE, VHS_GLITCH_FWD
    VHS_EVENT EV_PATH_ACTIVATED, CALLER_PATH, VHS_RESTORE, ACT_ACTIVATE_SCENE, VHS_GLITCH_BWD
    VHS_EVENT EV_PATH_ACTIVATED, CALLER_PATH, VHS_WAVE_MID, ACT_ACTIVATE_SCENE, VHS_GLITCH_FWD
    VHS_EVENT EV_PATH_ACTIVATED, CALLER_PATH, VHS_WAVE_END, ACT_ACTIVATE_SCENE, VHS_GLITCH_FWD
    VHS_EVENT EV_SCENE_COMPLETE, CALLER_SCENE, VHS_GLITCH_BWD, ACT_ACTIVATE_SCENE, VHS_BASE
    add     x19, x19, #1
    b       vhs_build_line_effects.char
vhs_build_line_effects.done:
    ldr     x30, [sp, #(VHS_L_SIZE + 48)]
    ldp     x23, x24, [sp, #(VHS_L_SIZE + 32)]
    ldp     x21, x22, [sp, #(VHS_L_SIZE + 16)]
    ldp     x19, x20, [sp, #VHS_L_SIZE]
    add     sp, sp, #(VHS_L_SIZE + 64)
    ret

// vhs_snow_frames(w0=scene, w1=count <= 32): count frames of 2 ticks,
// each choice(snow_chars) then choice(noise_colors), drawn in that order
// (vhs_snow_memo holds every pair's visual). snow_chars has four
// symbols, so its draw is the top two bits and never rejects.
// [sp, #0..127]: the handles.
vhs_snow_frames:
    sub     sp, sp, #192
    stp     x19, x20, [sp, #128]
    stp     x21, x22, [sp, #144]
    stp     x23, x24, [sp, #160]
    str     x30, [sp, #176]
    mov     w20, w0
    mov     w23, w1
    LDX     x9, effect_config
    ldr     x24, [x9, #VHSTAPE.noise_count]
    LDX     x4, vhs_snow_memo
    LDB     w3, vhs_noise_shift
    RNG_OPEN x21, x22
    mov     x19, #0
vhs_snow_frames.draw:
    RNG_TAKE x9, x21, x22
    lsr     x9, x9, #62                 // symbol index
vhs_snow_frames.color:
    RNG_TAKE x2, x21, x22
    lsr     x2, x2, x3
    cmp     x2, x24
    b.hs    vhs_snow_frames.color
    add     x9, x9, x2, lsl #2
    ldr     w9, [x4, x9, lsl #2]
    str     w9, [sp, x19, lsl #2]
    add     x19, x19, #1
    cmp     w19, w23
    b.lo    vhs_snow_frames.draw
    RNG_CLOSE x21
    mov     w0, w20
    mov     x1, sp
    mov     w2, w23
    mov     w3, #2
    bl      visual_frames
    ldr     x30, [sp, #176]
    ldp     x23, x24, [sp, #160]
    ldp     x21, x22, [sp, #144]
    ldp     x19, x20, [sp, #128]
    add     sp, sp, #192
    ret

// vhs_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
vhs_final_color_map:
    PUSH2   x19, x30
    LDX     x19, effect_config
    ldr     x0, [x19, #VHSTAPE.final_steps]
    ldr     x3, [x19, #VHSTAPE.final_step_count]
    ldr     x1, [x19, #VHSTAPE.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    STX     x9, vhs_final_spectrum
    ldr     x0, [x19, #VHSTAPE.final_stops]
    ldr     x1, [x19, #VHSTAPE.final_stop_count]
    ldr     x2, [x19, #VHSTAPE.final_steps]
    ldr     x3, [x19, #VHSTAPE.final_step_count]
    LDX     x4, vhs_final_spectrum
    bl      gradient_new
    LDX     x0, vhs_final_spectrum
    mov     w1, w9
    LDX     x2, text_bottom
    LDX     x3, text_top
    LDX     x4, text_left
    LDX     x5, text_right
    sub     x9, x5, x4
    add     x9, x9, #1
    STX     x9, vhs_final_map_width
    ldr     x6, [x19, #VHSTAPE.final_direction]
    bl      gradient_map
    STX     x9, vhs_final_map
    POP2    x19, x30
    ret

// ------------------------------------------------------------ vhs_lines

// vhs_line_slots(x0=line) -> x9 = slots, x2 = count. Clobbers x0.
vhs_line_slots:
    LDX     x9, vhs_lines
    add     x0, x9, x0, lsl #4
    ldp     x9, x2, [x0]
    ret

// vhs_line_complete(x0=line) -> w9 = 1 when no character of the line has an
// active path (Line.line_movement_complete). Clobbers x3, x2, x0, x4.
vhs_line_complete:
    str     x30, [sp, #-16]!
    bl      vhs_line_slots
    ldr     x30, [sp], #16
    LDX     x3, ch_path
vhs_line_complete.next:
    cbz     x2, vhs_line_complete.yes
    sub     x2, x2, #1
    ldr     w4, [x9, x2, lsl #2]
    ldr     w4, [x3, x4, lsl #2]
    cmn     w4, #1                      // NONE
    b.eq    vhs_line_complete.next
    mov     w9, #0
    ret
vhs_line_complete.yes:
    mov     w9, #1
    ret

// vhs_lines_complete(x0=line list, x1=count) -> w9 = 1 when every listed line
// has completed its movement (vacuously for none). Clobbers C.
vhs_lines_complete:
    PUSH2   x19, x21
    PUSH2   x22, x30
    mov     x21, x0
    mov     x22, x1
    mov     x19, #0
vhs_lines_complete.next:
    cmp     x19, x22
    b.hs    vhs_lines_complete.yes
    ldr     x0, [x21, x19, lsl #3]
    bl      vhs_line_complete
    cbz     w9, vhs_lines_complete.done
    add     x19, x19, #1
    b       vhs_lines_complete.next
vhs_lines_complete.yes:
    mov     w9, #1
vhs_lines_complete.done:
    POP2    x22, x30
    POP2    x19, x21
    ret

// vhs_list_contains(x0=line list, x1=count, x2=line) -> w9 = 1 when listed.
// Clobbers x1, x12.
vhs_list_contains:
    mov     w9, #0
vhs_list_contains.next:
    cbz     x1, vhs_list_contains.done
    sub     x1, x1, #1
    ldr     x12, [x0, x1, lsl #3]
    cmp     x12, x2
    b.ne    vhs_list_contains.next
    mov     w9, #1
vhs_list_contains.done:
    ret

// vhs_line_insert(x0=line): every character of the line joins the active set.
// Clobbers x9, x3, x2, x0, x4, x5 (and x16).
vhs_line_insert:
    str     x30, [sp, #-16]!
    bl      vhs_line_slots
    mov     x4, x9
    mov     x5, x2
vhs_line_insert.next:
    cbz     x5, vhs_line_insert.done
    sub     x5, x5, #1
    ldr     w0, [x4, x5, lsl #2]
    bl      active_insert               // clobbers x9, x3, x2 (x16) only
    b       vhs_line_insert.next
vhs_line_insert.done:
    ldr     x30, [sp], #16
    ret

// vhs_line_set_hold(x0=line, x1=hold): Line.set_hold_time on the glitch paths.
vhs_line_set_hold:
    str     x30, [sp, #-16]!
    bl      vhs_line_slots
    ldr     x30, [sp], #16
    LDX     x3, ch_user0
vhs_line_set_hold.next:
    cbz     x2, vhs_line_set_hold.done
    sub     x2, x2, #1
    ldr     w0, [x9, x2, lsl #2]
    add     x12, x3, x0, lsl #3
    ldr     w0, [x12]                   // glitch path
    PATH_PTR x0, x0
    str     x1, [x0, #PA_HOLD]
    b       vhs_line_set_hold.next
vhs_line_set_hold.done:
    ret

// VHS_RANDOM_SPEED path offset: the path at that distance from the glitch
// path of slot w20 gets speed 40 / randint(20, 40). Clobbers C except
// callee-saved.
.macro VHS_RANDOM_SPEED offset
    mov     x0, #20
    mov     x1, #40
    bl      rng_randint
    scvtf   d1, x9
    LDD     d0, vhs_forty
    fdiv    d0, d0, d1
    LDX     x9, ch_user0
    add     x9, x9, x20, lsl #3
    ldr     w9, [x9]
    add     w9, w9, #\offset
    PATH_PTR x9, x9
    str     d0, [x9, #PA_SPEED]
.endm

// VHS_LINE_LOOP fn / VHS_LINE_NEXT fn: walk line x0's characters in order
// with w20 = slot (x19, x21, x22 hold the walk), through the function's
// .next/.done labels. The caller has saved x19-x23 and x30.
.macro VHS_LINE_LOOP fn
    bl      vhs_line_slots
    mov     x21, x9
    mov     x22, x2
    mov     x19, #0
\fn\().next:
    cmp     x19, x22
    b.hs    \fn\().done
    ldr     w20, [x21, x19, lsl #2]
.endm

.macro VHS_LINE_NEXT fn
    add     x19, x19, #1
    b       \fn\().next
\fn\().done:
.endm

.macro VHS_LINE_PROLOGUE
    PUSH2   x19, x20
    PUSH2   x21, x22
    PUSH2   x23, x30
.endm

.macro VHS_LINE_EPILOGUE
    POP2    x23, x30
    POP2    x21, x22
    POP2    x19, x20
    ret
.endm

// vhs_line_glitch(x0=line): Line.glitch(final=False) - new glitch and restore
// speeds (drawn in that order), then the glitch path.
vhs_line_glitch:
    VHS_LINE_PROLOGUE
    VHS_LINE_LOOP vhs_line_glitch
    VHS_RANDOM_SPEED 0
    VHS_RANDOM_SPEED VHS_P_RESTORE
    mov     w0, w20
    LDX     x9, ch_user0
    add     x9, x9, x20, lsl #3
    ldr     w1, [x9]
    bl      path_activate
    VHS_LINE_NEXT vhs_line_glitch
    VHS_LINE_EPILOGUE

// vhs_line_restore(x0=line): Line.restore - a new restore speed, then the
// restore path.
vhs_line_restore:
    VHS_LINE_PROLOGUE
    VHS_LINE_LOOP vhs_line_restore
    VHS_RANDOM_SPEED VHS_P_RESTORE
    mov     w0, w20
    LDX     x9, ch_user0
    add     x9, x9, x20, lsl #3
    ldr     w1, [x9]
    add     w1, w1, #VHS_P_RESTORE
    bl      path_activate
    VHS_LINE_NEXT vhs_line_restore
    VHS_LINE_EPILOGUE

// vhs_line_activate_path(x0=line, w1=path offset from the glitch path):
// Line.activate_path.
vhs_line_activate_path:
    VHS_LINE_PROLOGUE
    mov     w23, w1
    VHS_LINE_LOOP vhs_line_activate_path
    mov     w0, w20
    LDX     x9, ch_user0
    add     x9, x9, x20, lsl #3
    ldr     w1, [x9]
    add     w1, w1, w23
    bl      path_activate
    VHS_LINE_NEXT vhs_line_activate_path
    VHS_LINE_EPILOGUE

// vhs_line_scene(x0=line, w1=scene offset from the base scene): activate that
// scene for every character (Line.snow, and the redraw phase).
vhs_line_scene:
    VHS_LINE_PROLOGUE
    mov     w23, w1
    VHS_LINE_LOOP vhs_line_scene
    mov     w0, w20
    LDX     x9, ch_user0
    add     x9, x9, x20, lsl #3
    ldr     w1, [x9, #4]
    add     w1, w1, w23
    bl      scene_activate
    VHS_LINE_NEXT vhs_line_scene
    VHS_LINE_EPILOGUE

// vhs_glitch_wave: VHSTapeIterator.glitch_wave. The caller has established that
// every wave line completed its movement, which is the only other condition.
vhs_glitch_wave:
    PUSH2   x19, x21
    PUSH2   x22, x30
    LDX     x9, vhs_wave_top
    MOV64   x3, NONE_I64
    cmp     x9, x3
    b.eq    vhs_glitch_wave.choose
    cbnz    x9, vhs_glitch_wave.have_top
vhs_glitch_wave.choose:
    // a wave top in the top half of the text, or at least 3 rows up
    LDX     x19, text_top
    LDX     x10, text_bottom
    sub     x19, x19, x10
    add     x19, x19, #1                // text_height
    cmp     x19, #3
    b.lt    vhs_glitch_wave.out
    scvtf   d0, x19
    fmov    d1, #0.5
    fmul    d0, d0, d1
    bl      round_half_even
    mov     x3, #3
    cmp     x9, x3
    csel    x9, x3, x9, lt
    mov     x0, x9
    mov     x1, x19
    bl      rng_randint
    LDX     x10, text_bottom
    add     x9, x9, x10
    STX     x9, vhs_wave_top
vhs_glitch_wave.have_top:
    LDX     x9, vhs_wave_count
    cbz     x9, vhs_glitch_wave.lines
    // move 30% of the time, up 30% of those
    bl      rng_random
    mov     x19, #0
    LDD     d1, vhs_point3
    fcmp    d0, d1
    b.ge    vhs_glitch_wave.clamp
    bl      rng_random
    mov     x19, #-1
    mov     x3, #1
    LDD     d1, vhs_point3
    fcmp    d0, d1
    csel    x19, x3, x19, mi
vhs_glitch_wave.clamp:
    LDX     x9, vhs_wave_top
    add     x19, x19, x9
    LDX     x9, text_top
    cmp     x19, x9
    csel    x19, x9, x19, gt
    mov     x3, #2
    cmp     x19, x3
    csel    x19, x3, x19, lt
    STX     x19, vhs_wave_top
vhs_glitch_wave.lines:
    // the vhs_lines of rows vhs_wave_top - 2 ..= vhs_wave_top
    mov     x21, #0
    LDX     x19, vhs_wave_top
    sub     x19, x19, #2
vhs_glitch_wave.row:
    LDX     x9, vhs_wave_top
    cmp     x19, x9
    b.gt    vhs_glitch_wave.old
    LDX     x10, text_bottom
    sub     x9, x19, x10
    adds    x9, x9, #1
    b.mi    vhs_glitch_wave.skip
    LDX     x10, vhs_line_count
    cmp     x9, x10
    b.hs    vhs_glitch_wave.skip
    ADRG    x3, vhs_new_lines
    str     x9, [x3, x21, lsl #3]
    add     x21, x21, #1
vhs_glitch_wave.skip:
    add     x19, x19, #1
    b       vhs_glitch_wave.row
vhs_glitch_wave.old:
    // restore the vhs_lines that left the wave
    mov     x19, #0
vhs_glitch_wave.old_line:
    LDX     x9, vhs_wave_count
    cmp     x19, x9
    b.hs    vhs_glitch_wave.replace
    ADRG    x9, vhs_wave_lines
    ldr     x22, [x9, x19, lsl #3]
    ADRG    x0, vhs_new_lines
    mov     x1, x21
    mov     x2, x22
    bl      vhs_list_contains
    cbnz    w9, vhs_glitch_wave.kept
    mov     x0, x22
    bl      vhs_line_restore
    mov     x0, x22
    bl      vhs_line_insert
vhs_glitch_wave.kept:
    add     x19, x19, #1
    b       vhs_glitch_wave.old_line
vhs_glitch_wave.replace:
    STX     x21, vhs_wave_count
    ADRG    x9, vhs_new_lines
    ADRG    x3, vhs_wave_lines
    ldp     x12, x13, [x9]
    stp     x12, x13, [x3]
    ldp     x12, x13, [x9, #16]
    stp     x12, x13, [x3, #16]
    LDX     x9, text_bottom
    add     x9, x9, #2
    LDX     x10, vhs_wave_top
    cmp     x10, x9
    b.ge    vhs_glitch_wave.advance
    // the wave reached the bottom: restore its vhs_lines
    mov     x19, #0
vhs_glitch_wave.bottom:
    LDX     x9, vhs_wave_count
    cmp     x19, x9
    b.hs    vhs_glitch_wave.ended
    ADRG    x9, vhs_wave_lines
    ldr     x22, [x9, x19, lsl #3]
    mov     x0, x22
    bl      vhs_line_restore
    mov     x0, x22
    bl      vhs_line_insert
    add     x19, x19, #1
    b       vhs_glitch_wave.bottom
vhs_glitch_wave.ended:
    STX     xzr, vhs_wave_count
    MOV64   x9, NONE_I64
    STX     x9, vhs_wave_top
    b       vhs_glitch_wave.out
vhs_glitch_wave.advance:
    // mid, end, mid
    mov     x19, #0
vhs_glitch_wave.wave_line:
    LDX     x9, vhs_wave_count
    cmp     x19, x9
    b.hs    vhs_glitch_wave.out
    ADRG    x9, vhs_wave_lines
    ldr     x22, [x9, x19, lsl #3]
    ADRG    x9, vhs_wave_paths
    ldrb    w1, [x9, x19]
    mov     x0, x22
    bl      vhs_line_activate_path
    mov     x0, x22
    bl      vhs_line_insert
    add     x19, x19, #1
    b       vhs_glitch_wave.wave_line
vhs_glitch_wave.out:
    POP2    x22, x30
    POP2    x19, x21
    ret

// vhstape_next_frame -> w9 = 1 when a frame should be rendered, 0 when done.
vhstape_next_frame:
    PUSH2   x19, x20
    PUSH2   x21, x22
    PUSH2   x23, x30
    LDB     w9, vhs_phase
    cmp     w9, #VHS_PH_GLITCHING
    b.eq    vhstape_next_frame.glitching
    cmp     w9, #VHS_PH_NOISE
    b.eq    vhstape_next_frame.noise
    cmp     w9, #VHS_PH_REDRAW
    b.eq    vhstape_next_frame.redraw
    bl      active_empty
    cbnz    w9, vhstape_next_frame.finished
    b       vhstape_next_frame.update
vhstape_next_frame.glitching:
    // move the wave once its vhs_lines have settled
    ADRG    x0, vhs_wave_lines
    LDX     x1, vhs_wave_count
    bl      vhs_lines_complete
    cbz     w9, vhstape_next_frame.prune
    bl      vhs_glitch_wave
vhstape_next_frame.prune:
    // drop glitch vhs_lines that completed their movement (order kept)
    mov     x19, #0
    mov     x21, #0
vhstape_next_frame.keep:
    LDX     x9, vhs_glitch_count
    cmp     x19, x9
    b.hs    vhstape_next_frame.kept
    ADRG    x9, vhs_glitch_lines
    ldr     x0, [x9, x19, lsl #3]
    bl      vhs_line_complete
    cbnz    w9, vhstape_next_frame.drop
    ADRG    x9, vhs_glitch_lines
    ldr     x3, [x9, x19, lsl #3]
    str     x3, [x9, x21, lsl #3]
    add     x21, x21, #1
vhstape_next_frame.drop:
    add     x19, x19, #1
    b       vhstape_next_frame.keep
vhstape_next_frame.kept:
    STX     x21, vhs_glitch_count
    // randomly glitch a new line
    bl      rng_random
    LDX     x9, effect_config
    ldr     d1, [x9, #VHSTAPE.line_chance]
    fcmp    d0, d1
    b.ge    vhstape_next_frame.noise_roll
    LDX     x9, vhs_glitch_count
    cmp     x9, #3
    b.hs    vhstape_next_frame.noise_roll
    LDX     x0, vhs_line_count
    bl      rng_below
    mov     x19, x9
    ADRG    x0, vhs_wave_lines
    LDX     x1, vhs_wave_count
    mov     x2, x19
    bl      vhs_list_contains
    cbnz    w9, vhstape_next_frame.noise_roll
    ADRG    x0, vhs_glitch_lines
    LDX     x1, vhs_glitch_count
    mov     x2, x19
    bl      vhs_list_contains
    cbnz    w9, vhstape_next_frame.noise_roll
    mov     x0, #20
    mov     x1, #75
    bl      rng_randint
    mov     x0, x19
    mov     x1, x9
    bl      vhs_line_set_hold
    LDX     x9, vhs_glitch_count
    ADRG    x3, vhs_glitch_lines
    str     x19, [x3, x9, lsl #3]
    add     x9, x9, #1
    STX     x9, vhs_glitch_count
    mov     x0, x19
    bl      vhs_line_glitch
    mov     x0, x19
    bl      vhs_line_insert
vhstape_next_frame.noise_roll:
    // randomly add noise to all vhs_lines
    bl      rng_random
    LDX     x9, effect_config
    ldr     d1, [x9, #VHSTAPE.noise_chance]
    fcmp    d0, d1
    b.ge    vhstape_next_frame.elapsed
    mov     x19, #0
vhstape_next_frame.snow:
    LDX     x9, vhs_line_count
    cmp     x19, x9
    b.hs    vhstape_next_frame.elapsed
    mov     x0, x19
    mov     w1, #VHS_S_SNOW
    bl      vhs_line_scene
    ADRG    x0, vhs_wave_lines
    LDX     x1, vhs_wave_count
    mov     x2, x19
    bl      vhs_list_contains
    cbnz    w9, vhstape_next_frame.snowed
    ADRG    x0, vhs_glitch_lines
    LDX     x1, vhs_glitch_count
    mov     x2, x19
    bl      vhs_list_contains
    cbnz    w9, vhstape_next_frame.snowed
    mov     x0, x19
    bl      vhs_line_insert
vhstape_next_frame.snowed:
    add     x19, x19, #1
    b       vhstape_next_frame.snow
vhstape_next_frame.elapsed:
    LDX     x9, vhs_elapsed
    add     x9, x9, #1
    STX     x9, vhs_elapsed
    LDX     x3, effect_config
    ldr     x3, [x3, #VHSTAPE.total_time]
    cmp     x9, x3
    b.lt    vhstape_next_frame.update
    // time is up: restore the wave vhs_lines, then the glitch vhs_lines
    mov     x19, #0
vhstape_next_frame.restore_wave:
    LDX     x9, vhs_wave_count
    cmp     x19, x9
    b.hs    vhstape_next_frame.restore_glitch
    ADRG    x9, vhs_wave_lines
    ldr     x0, [x9, x19, lsl #3]
    bl      vhs_line_restore
    add     x19, x19, #1
    b       vhstape_next_frame.restore_wave
vhstape_next_frame.restore_glitch:
    mov     x19, #0
vhstape_next_frame.restore_line:
    LDX     x9, vhs_glitch_count
    cmp     x19, x9
    b.hs    vhstape_next_frame.to_noise
    ADRG    x9, vhs_glitch_lines
    ldr     x0, [x9, x19, lsl #3]
    bl      vhs_line_restore
    add     x19, x19, #1
    b       vhstape_next_frame.restore_line
vhstape_next_frame.to_noise:
    mov     w9, #VHS_PH_NOISE
    STB     w9, vhs_phase
    b       vhstape_next_frame.update
vhstape_next_frame.noise:
    // once everything settled, final snow for every character
    bl      active_empty
    cbz     w9, vhstape_next_frame.update
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    mov     x19, #0
vhstape_next_frame.final_snow:
    cmp     x19, x22
    b.hs    vhstape_next_frame.to_redraw
    ldr     w20, [x21, x19, lsl #2]
    mov     w0, w20
    LDX     x9, ch_user0
    add     x9, x9, x20, lsl #3
    ldr     w1, [x9, #4]
    add     w1, w1, #VHS_S_FINAL_SNOW
    bl      scene_activate
    mov     w0, w20
    bl      active_insert
    add     x19, x19, #1
    b       vhstape_next_frame.final_snow
vhstape_next_frame.to_redraw:
    mov     w9, #VHS_PH_REDRAW
    STB     w9, vhs_phase
    b       vhstape_next_frame.update
vhstape_next_frame.redraw:
    // redraw vhs_lines one by one, top line first
    LDB     w9, vhs_redrawing
    cbnz    w9, vhstape_next_frame.draw
    bl      active_empty
    cbz     w9, vhstape_next_frame.update
vhstape_next_frame.draw:
    mov     w9, #1
    STB     w9, vhs_redrawing
    LDX     x19, vhs_to_redraw
    cbz     x19, vhstape_next_frame.complete
    sub     x19, x19, #1
    STX     x19, vhs_to_redraw
    mov     x0, x19
    mov     w1, #VHS_S_FINAL_REDRAW
    bl      vhs_line_scene
    mov     x0, x19
    bl      vhs_line_insert
    b       vhstape_next_frame.update
vhstape_next_frame.complete:
    mov     w9, #VHS_PH_COMPLETE
    STB     w9, vhs_phase
vhstape_next_frame.update:
    bl      update
    mov     w9, #1
    b       vhstape_next_frame.out
vhstape_next_frame.finished:
    mov     w9, #0
vhstape_next_frame.out:
    POP2    x23, x30
    POP2    x21, x22
    POP2    x19, x20
    ret

    .section .rodata
    .balign 8
vhs_two:        .double 2.0
vhs_forty:      .double 40.0
vhs_half:       .double 0.5
vhs_point3:     .double 0.3
vhs_snow_symbols:   .ascii "#*.:"
vhs_wave_paths:     .byte VHS_P_MID, VHS_P_END, VHS_P_MID

    TSTATE
    .balign 8
vhs_lines:          .skip 8                 // groups: (u32 *slots, u64 count)
vhs_line_count:     .skip 8
vhs_snow_memo:      .skip 8
vhs_line_handles:   .skip 8
vhs_line_colors_rev: .skip 8
vhs_final_spectrum: .skip 8
vhs_final_map:      .skip 8
vhs_final_map_width: .skip 8
vhs_wave_top:       .skip 8                 // NONE_I64 = None
vhs_wave_count:     .skip 8
vhs_wave_lines:     .skip 8 * 4
vhs_new_lines:      .skip 8 * 4
vhs_glitch_count:   .skip 8
vhs_glitch_lines:   .skip 8 * 3
vhs_elapsed:        .skip 8
vhs_to_redraw:      .skip 8
vhs_block_visual:   .skip 4
vhs_noise_shift:    .skip 1
vhs_phase:          .skip 1
vhs_redrawing:      .skip 1

    .text
