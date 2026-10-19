// effects/spotlights.s - "Search the text with spotlights that converge in
// the center" (src/effects/spotlights.rs).
//
// Config (src/asm/effects.rs, the Spotlights arm).
//
// The spotlights are added characters that are never shown; each walks its
// eleven chained bezier paths ("0".."10", looping) until the search ends and
// then its "center" path. Every frame the input characters inside any
// spotlight's ellipse show their bright colors, dimmed towards the beam's
// edge; characters that left every beam fall back to their dark colors.
//
// The illuminated set is a list plus a per-character frame stamp (spl_marks).
// Order does not matter: a frame draws nothing from the RNG and only sets
// appearances.
//
// Speed: each character keeps its bright and dark visual handles; a
// character well inside the beam's edge by exact integer distance skips the
// hypot fold; beam distances come from a memo of hypot over (|dx|, |dy|);
// dimmed visuals are memoized per character (its last factor) and by
// (bright handle, brightness factor bits), which determines the symbol and
// both adjusted colors. That memo holds ~190K entries over a run (probes
// average 1.15 slots), so its lookups are cache misses; a frame's lit
// characters go through it in batches whose slots are prefetched first.

.equ HAVE_spotlights, 1

.equ SPOTLIGHTS.beam_width_ratio,   0   // f64
.equ SPOTLIGHTS.beam_falloff,       8   // f64
.equ SPOTLIGHTS.search_duration,    16
.equ SPOTLIGHTS.speed_lo,           24  // f64
.equ SPOTLIGHTS.speed_hi,           32  // f64
.equ SPOTLIGHTS.count,              40
.equ SPOTLIGHTS.final_stops,        48  // *const u64
.equ SPOTLIGHTS.final_stop_count,   56
.equ SPOTLIGHTS.final_steps,        64  // *const i64
.equ SPOTLIGHTS.final_step_count,   72
.equ SPOTLIGHTS.final_direction,    80
.equ SPOTLIGHTS_size,               88

// per input character (spl_recs + slot * SPL_REC_size)
.equ SPL_REC.fg,        0               // bright pair
.equ SPL_REC.bg,        8
.equ SPL_REC.bright,    16              // visual handles
.equ SPL_REC.dark,      20
.equ SPL_REC.mark,      24              // (unused: spl_marks)
.equ SPL_REC.flags,     28              // SPLF_*
.equ SPL_REC.factor,    32              // the last brightness factor's bits
.equ SPL_REC.dimmed,    40              // and its visual (0: none yet)
.equ SPL_REC.pad,       44
.equ SPL_REC_size,      48

// a memo batch entry (spl_batch)
.equ SPL_BE.visual,     0               // 0 until known
.equ SPL_BE.slot,       4               // memo slot to probe from, then the empty one
.equ SPL_BE.factor,     8               // brightness factor bits
.equ SPL_BE_size,       16

.equ SPLF_LIT,          1               // _is_spotlightable
.equ SPLF_UNCACHED,     2               // shows its input colors (always)
.equ SPLF_OVERRIDE,     4               // _get_expand_color_override applies

// path names: "0".."10" are NAME_LITERAL + k
.equ SPL_CENTER,        NAME_LITERAL + 11
.equ SPL_IN_OUT_SINE,   3
.equ SPL_IN_OUT_QUAD,   6

.equ SPL_MEMO_BITS,     19
.equ SPL_MEMO_SIZE,     (1 << SPL_MEMO_BITS)
.equ SPL_BATCH,         16              // characters per memo batch

    .text

// spotlights_build: Spotlights::build.
spotlights_build:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    bl      spl_make_spotlights
    bl      spl_final_color_map
    LDX     x9, cfg_existing_colors
    cmp     x9, #1
    cset    w9, eq
    STB     w9, spl_dynamic
    // per-character records and the two illuminated lists
    LDW     w19, char_count
    mov     x9, #SPL_REC_size
    mul     x0, x19, x9
    add     x0, x0, #64
    bl      alloc
    STX     x9, spl_recs
    lsl     x0, x19, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, spl_lit
    lsl     x0, x19, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, spl_next
    // per character: the frame stamp it was lit in, or all ones when it
    // cannot be lit (so "stamp >= this frame's" skips both)
    lsl     x0, x19, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, spl_marks
    mov     x0, x9
    mov     w3, w19
    mov     w9, #-1
    REP_STOSD
    // memo tables
    mov     x0, #(SPL_MEMO_SIZE * 16)
    bl      reserve
    STX     x9, spl_memo
    LDX     x9, canvas_right
    add     x9, x9, #1
    STX     x9, spl_hyp_w
    LDX     x3, canvas_top
    add     x3, x3, #1
    STX     x3, spl_hyp_h
    mul     x9, x9, x3
    lsl     x0, x9, #3
    add     x0, x0, #64
    bl      reserve
    STX     x9, spl_hyp
    // every input character: colors, visible, dark appearance
    mov     w0, #FILTER_INPUT
    mov     w1, #SORT_TOP_TO_BOTTOM_L2R
    bl      get_characters
    mov     x21, x9
    mov     x22, x2
    mov     x24, #0
spotlights_build.char:
    cmp     x24, x22
    b.hs    spotlights_build.range
    ldr     w20, [x21, x24, lsl #2]
    mov     x9, #SPL_REC_size
    LDX     x23, spl_recs
    madd    x23, x20, x9, x23
    LDX     x9, ch_fg
    ldr     x19, [x9, x20, lsl #3]      // input fg
    LDX     x9, ch_bg
    ldr     x3, [x9, x20, lsl #3]       // input bg
    // flags
    mov     w2, #0
    LDX     x9, ch_sym
    ldr     x9, [x9, x20, lsl #3]
    LDX     x10, spl_space
    cmp     x9, x10
    b.ne    spotlights_build.lit
    cmn     x19, #1                     // NONE
    b.ne    spotlights_build.lit
    cmn     x3, #1
    b.eq    spotlights_build.unlit
spotlights_build.lit:
    orr     w2, w2, #SPLF_LIT
spotlights_build.unlit:
    LDX     x9, cfg_existing_colors
    cbnz    x9, spotlights_build.override
    LDX     x9, ch_flags
    ldrh    w9, [x9, x20, lsl #1]
    tst     w9, #CF_PREEXISTING
    b.eq    spotlights_build.override
    orr     w2, w2, #SPLF_UNCACHED
spotlights_build.override:
    LDB     w9, spl_dynamic
    cbz     w9, spotlights_build.flags
    cmn     x19, #1
    b.ne    spotlights_build.flags      // fg present: no override
    orr     w2, w2, #SPLF_OVERRIDE      // (None, bg) or no input colors
spotlights_build.flags:
    str     w2, [x23, #SPL_REC.flags]
    tbz     w2, #0, spotlights_build.unmarked   // SPLF_LIT
    LDX     x9, spl_marks
    str     wzr, [x9, x20, lsl #2]
spotlights_build.unmarked:
    LDB     w9, spl_dynamic
    cbz     w9, spotlights_build.gradient
    // dynamic: the input colors, gray standing in for a missing fg
    cmn     x19, #1
    b.ne    spotlights_build.dyn_pair
    MOV64   x19, 0x808080
spotlights_build.dyn_pair:
    str     x19, [x23, #SPL_REC.fg]
    str     x3, [x23, #SPL_REC.bg]
    b       spotlights_build.pairs
spotlights_build.gradient:
    LDX     x9, ch_irow
    ldrsw   x9, [x9, x20, lsl #2]
    LDX     x10, text_bottom
    sub     x9, x9, x10
    LDX     x10, spl_map_width
    mul     x9, x9, x10
    LDX     x3, ch_icol
    ldrsw   x3, [x3, x20, lsl #2]
    add     x9, x9, x3
    LDX     x10, text_left
    sub     x9, x9, x10
    LDX     x3, spl_map
    ldr     x9, [x3, x9, lsl #3]
    str     x9, [x23, #SPL_REC.fg]
    mov     x9, #NONE
    str     x9, [x23, #SPL_REC.bg]
spotlights_build.pairs:
    // bright and dark (0.2) visuals
    mov     w0, w20
    ldr     x2, [x23, #SPL_REC.fg]
    ldr     x3, [x23, #SPL_REC.bg]
    bl      spl_visual
    str     w9, [x23, #SPL_REC.bright]
    LDD     d0, spl_dim
    bl      spl_adjusted
    str     w9, [x23, #SPL_REC.dark]
    mov     w0, w20
    mov     w1, #1
    bl      set_visibility
    mov     w0, w20
    ldr     w9, [x23, #SPL_REC.dark]
    SET_HANDLE
    add     x24, x24, #1
    b       spotlights_build.char
spotlights_build.range:
    // illuminate_range = max(int(min(floor(smallest / ratio), smallest)), 1)
    LDX     x9, canvas_right
    LDX     x10, canvas_top
    cmp     x9, x10
    csel    x9, x10, x9, gt
    scvtf   d1, x9
    fmov    d0, d1
    LDX     x3, effect_config
    ldr     d2, [x3, #SPOTLIGHTS.beam_width_ratio]
    fdiv    d0, d0, d2
    FLOORSD d0
    fminnm  d0, d0, d1                  // f64::min, as the oracle compiles it
    F64_TO_I64
    mov     x3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    STX     x9, spl_range
    LDX     x3, effect_config
    ldr     x9, [x3, #SPOTLIGHTS.search_duration]
    STX     x9, spl_search_left
    mov     w9, #1
    STB     w9, spl_searching
    // every spotlight starts on path "0"
    mov     x19, #0
spotlights_build.start:
    LDX     x9, spl_live
    cmp     x19, x9
    b.hs    spotlights_build.done
    LDX     x9, spl_slots
    ldr     w20, [x9, x19, lsl #2]
    mov     w0, w20
    mov     w1, #NAME_LITERAL
    bl      path_activate_name
    mov     w0, w20
    bl      active_insert
    add     x19, x19, #1
    b       spotlights_build.start
spotlights_build.done:
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// spl_make_spotlights: SpotlightsIterator.make_spotlights, draw for draw.
spl_make_spotlights:
    sub     sp, sp, #208
    stp     x19, x20, [sp, #144]
    stp     x21, x22, [sp, #160]
    stp     x23, x24, [sp, #176]
    str     x30, [sp, #192]
    // [sp] bezier control, [sp, #8] 11 targets, [sp, #96] 11 names
    LDX     x19, effect_config
    ldr     x0, [x19, #SPOTLIGHTS.count]
    STX     x0, spl_live
    lsl     x0, x0, #2
    add     x0, x0, #64
    bl      alloc
    STX     x9, spl_slots
    ldr     x0, [x19, #SPOTLIGHTS.count]
    lsl     x0, x0, #4
    add     x0, x0, #64
    bl      alloc
    STX     x9, spl_coords
    // minimum_distance = canvas.right // 4
    LDX     x0, canvas_right
    mov     x1, #4
    bl      floor_div
    scvtf   d0, x9
    STD     d0, spl_min_distance
    mov     w9, #0
    add     x10, sp, #96
spl_make_spotlights.name:
    orr     w3, w9, #NAME_LITERAL
    str     w3, [x10, w9, uxtw #2]
    add     w9, w9, #1
    cmp     w9, #11
    b.lo    spl_make_spotlights.name
    mov     x21, #0                     // spotlight index
spl_make_spotlights.spotlight:
    ldr     x9, [x19, #SPOTLIGHTS.count]
    cmp     x21, x9
    b.hs    spl_make_spotlights.done
    mov     w0, #1
    mov     w1, #0
    bl      canvas_random_coord
    mov     x1, x9
    LDX     x0, spl_symbol
    bl      add_character
    mov     w22, w9                     // slot
    LDX     x3, spl_slots
    str     w9, [x3, x21, lsl #2]
    // the targets: a random coordinate, then ten more at minimum distance
    mov     w0, #0
    mov     w1, #0
    bl      canvas_random_coord
    str     x9, [sp, #8]
    mov     w23, #1
spl_make_spotlights.target:
    mov     w0, #0
    mov     w1, #0
    bl      canvas_random_coord
    mov     x20, x9
    ldr     x0, [sp, x23, lsl #3]       // the previous target
    mov     x1, x20
    mov     w2, #0
    bl      find_length_of_line
    LDD     d1, spl_min_distance
    fcmp    d0, d1
    b.lt    spl_make_spotlights.target  // less or unordered
    add     x9, sp, #8
    str     x20, [x9, x23, lsl #3]
    add     w23, w23, #1
    cmp     w23, #11
    b.lo    spl_make_spotlights.target
    // one bezier path per target
    mov     w23, #0
spl_make_spotlights.path:
    ldr     d0, [x19, #SPOTLIGHTS.speed_lo]
    ldr     d1, [x19, #SPOTLIGHTS.speed_hi]
    bl      rng_uniform
    mov     w0, w22
    mov     w1, #SPL_IN_OUT_QUAD
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    orr     w5, w23, #NAME_LITERAL
    bl      path_new
    mov     w24, w9
    mov     w0, #1
    mov     w1, #0
    bl      canvas_random_coord
    str     x9, [sp]
    mov     w0, w24
    add     x9, sp, #8
    ldr     x1, [x9, x23, lsl #3]
    mov     x2, sp
    mov     w3, #1
    mov     w4, #AUTO
    bl      path_new_waypoint
    add     w23, w23, #1
    cmp     w23, #11
    b.lo    spl_make_spotlights.path
    mov     w0, w22
    add     x1, sp, #96
    mov     x2, #11
    mov     w3, #1
    bl      chain_paths
    // "center": straight to the canvas center
    mov     w0, w22
    LDD     d0, spl_half
    mov     w1, #SPL_IN_OUT_SINE
    MOV64   x2, NONE_I64
    mov     x3, #0
    mov     w4, #0
    MOV64   w5, SPL_CENTER
    bl      path_new
    mov     w0, w9
    LDX     x1, center_row
    LDW     w3, center_col
    orr     x1, x3, x1, lsl #32
    mov     x2, #0
    mov     w3, #0
    mov     w4, #AUTO
    bl      path_new_waypoint
    add     x21, x21, #1
    b       spl_make_spotlights.spotlight
spl_make_spotlights.done:
    ldr     x30, [sp, #192]
    ldp     x23, x24, [sp, #176]
    ldp     x21, x22, [sp, #160]
    ldp     x19, x20, [sp, #144]
    add     sp, sp, #208
    ret

// spl_final_color_map: Gradient::new(final stops, final steps) and its
// coordinate mapping over the text rectangle.
spl_final_color_map:
    PUSH2   x19, x21
    PUSH1   x30
    LDX     x19, effect_config
    ldr     x0, [x19, #SPOTLIGHTS.final_steps]
    ldr     x3, [x19, #SPOTLIGHTS.final_step_count]
    ldr     x1, [x19, #SPOTLIGHTS.final_stop_count]
    bl      gradient_capacity
    lsl     x0, x9, #3
    bl      alloc
    mov     x21, x9
    ldr     x0, [x19, #SPOTLIGHTS.final_stops]
    ldr     x1, [x19, #SPOTLIGHTS.final_stop_count]
    ldr     x2, [x19, #SPOTLIGHTS.final_steps]
    ldr     x3, [x19, #SPOTLIGHTS.final_step_count]
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
    STX     x9, spl_map_width
    ldr     x6, [x19, #SPOTLIGHTS.final_direction]
    bl      gradient_map
    STX     x9, spl_map
    POP1    x30
    POP2    x19, x21
    ret

// spl_visual(w0=slot, x2=fg or NONE, x3=bg or NONE) -> w9 = the visual
// Animation.set_appearance(input_symbol, ..., colors) makes: under
// --existing-color-handling always, a character using its input colors
// shows those and its bold instead. Clobbers what visual_make does.
spl_visual:
    LDX     x9, ch_sym
    ldr     x10, [x9, w0, uxtw #3]
    mov     w5, #0
    LDX     x9, cfg_existing_colors
    cbnz    x9, spl_visual.make
    LDX     x9, ch_flags
    ldrh    w9, [x9, w0, uxtw #1]
    tst     w9, #CF_PREEXISTING
    b.eq    spl_visual.make
    LDX     x2, ch_fg
    ldr     x2, [x2, w0, uxtw #3]
    LDX     x3, ch_bg
    ldr     x3, [x3, w0, uxtw #3]
    tst     w9, #CF_BOLD
    b.eq    spl_visual.make
    mov     w5, #ATTR_BOLD
spl_visual.make:
    mov     x0, x2
    mov     x1, x3
    mov     x2, x10
    mov     w3, w5
    b       visual_make

// spl_adjust_pair(x23=record, d0=brightness) -> x9 = fg, x2 = bg:
// _adjust_color_pair_brightness(bright pair, brightness).
// Clobbers C except x19-x24.
spl_adjust_pair:
    stp     x19, x30, [sp, #-32]!
    str     d0, [sp, #16]
    mov     x19, #NONE
    ldr     x0, [x23, #SPL_REC.fg]
    cmn     x0, #1
    b.eq    spl_adjust_pair.bg
    bl      adjust_color_brightness
    mov     x19, x9
spl_adjust_pair.bg:
    mov     x2, #NONE
    ldr     x0, [x23, #SPL_REC.bg]
    cmn     x0, #1
    b.eq    spl_adjust_pair.done
    ldr     d0, [sp, #16]
    bl      adjust_color_brightness
    mov     x2, x9
spl_adjust_pair.done:
    mov     x9, x19
    ldp     x19, x30, [sp], #32
    ret

// spl_adjusted(w20=slot, x23=record, d0=brightness) -> w9 = the visual
// of _adjust_color_pair_brightness(bright pair, brightness).
// Clobbers C except x19-x24.
spl_adjusted:
    PUSH1   x30
    bl      spl_adjust_pair
    POP1    x30
    mov     x3, x2
    mov     x2, x9
    mov     w0, w20
    b       spl_visual

// spl_override(w20=slot) -> w9 = the visual of the expand override
// (None, input bg): (None, bg) for a bg-only character, (None, None) for one
// without input colors, whose bg is NONE.
spl_override:
    mov     w0, w20
    mov     x2, #NONE
    LDX     x3, ch_bg
    ldr     x3, [x3, x20, lsl #3]
    b       spl_visual

// spl_dimmed(w20=slot, x23=record, d0=brightness factor) -> w9: the
// memoized spl_adjusted, or 0 with w2 = the factor's memo slot (prefetched)
// and x4 = the factor bits, for spl_memo_probe. The character's last factor
// and visual come first; then a memo keyed by (bright handle, factor bits),
// emptied when half full. Characters showing their input colors (always)
// are not memoized. Clobbers C except x19-x24.
spl_dimmed:
    ldr     w9, [x23, #SPL_REC.flags]
    tbnz    w9, #1, spl_adjusted        // SPLF_UNCACHED
    fmov    x9, d0
    ldr     x3, [x23, #SPL_REC.factor]
    cmp     x3, x9
    b.ne    spl_dimmed.memo
    ldr     w3, [x23, #SPL_REC.dimmed]
    cbz     w3, spl_dimmed.memo
    mov     w9, w3
    ret
spl_dimmed.memo:
    str     x9, [x23, #SPL_REC.factor]
    mov     x4, x9
    ldr     w2, [x23, #SPL_REC.bright]
    MOV64   x3, 0x9e3779b97f4a7c15
    mul     x3, x3, x2
    eor     x3, x3, x9
    MOV64   x1, 0xff51afd7ed558ccd
    mul     x3, x3, x1
    lsr     x3, x3, #(64 - SPL_MEMO_BITS)
    mov     w2, w3
    lsl     x3, x3, #4
    LDX     x1, spl_memo
    add     x3, x3, x1
    prfm    pldl1keep, [x3]
    mov     w9, #0
    ret

// spl_memo_probe(x23=record, w2=memo slot, x9=factor bits) -> w9: spl_dimmed's
// memo lookup from its slot: the visual, or 0 and w3 = the empty entry's
// slot where spl_memo_store keeps it. Clobbers x3, x2, x1, x0, x4.
spl_memo_probe:
    mov     w3, w2
    ldr     w2, [x23, #SPL_REC.bright]
    LDX     x1, spl_memo
spl_memo_probe.probe:
    add     x0, x1, x3, lsl #4
    ldr     w4, [x0, #8]
    cbz     w4, spl_memo_probe.miss
    cmp     w4, w2
    b.ne    spl_memo_probe.next
    ldr     x4, [x0]
    cmp     x4, x9
    b.ne    spl_memo_probe.next
    ldr     w9, [x0, #12]
    ret
spl_memo_probe.next:
    add     w3, w3, #1
    and     w3, w3, #(SPL_MEMO_SIZE - 1)
    b       spl_memo_probe.probe
spl_memo_probe.miss:
    mov     w9, #0
    ret

// spl_memo_store(x23=record, w2=empty slot, x3=factor bits, w9=visual):
// keep (bright, factor) -> visual; when the memo is half full, empty it
// instead (the entry is not kept either). Preserves x9. Clobbers x3, x2,
// x0.
spl_memo_store:
    LDX     x0, spl_memo_count
    cmp     x0, #(SPL_MEMO_SIZE / 2)
    b.lo    spl_memo_store.store
    mov     x2, x9
    LDX     x0, spl_memo
    mov     x3, #(SPL_MEMO_SIZE * 2)
    mov     x9, #0
    REP_STOSQ
    STX     xzr, spl_memo_count
    mov     x9, x2
    ret
spl_memo_store.store:
    add     x0, x0, #1
    STX     x0, spl_memo_count
    LDX     x0, spl_memo
    add     x2, x0, w2, uxtw #4
    str     x3, [x2]
    ldr     w3, [x23, #SPL_REC.bright]
    str     w3, [x2, #8]
    str     w9, [x2, #12]
    ret

// spl_distance(w20=slot) -> d0 = the smallest find_length_of_line(
// spotlight, input coordinate, double_row_diff) over the live spotlights
// (spl_coords), folded from +inf with f64::min. hypot is memoized over
// (|column delta|, |row delta|); glibc's hypot takes absolute values first.
// Clobbers C except x19-x24.
spl_distance:
    sub     sp, sp, #64
    stp     x19, x21, [sp, #16]
    stp     x22, x23, [sp, #32]
    stp     x24, x30, [sp, #48]
    LDX     x9, ch_icol
    ldrsw   x21, [x9, x20, lsl #2]
    LDX     x9, ch_irow
    ldrsw   x22, [x9, x20, lsl #2]
    LDX     x9, spl_inf
    str     x9, [sp]                    // the running minimum
    mov     x19, #0
    LDX     x24, spl_coords
spl_distance.spotlight:
    LDX     x9, spl_live
    cmp     x19, x9
    b.hs    spl_distance.done
    ldr     x9, [x24]
    sub     x1, x21, x9                 // column delta
    cmp     x1, #0
    cneg    x1, x1, lt
    ldr     x9, [x24, #8]
    sub     x0, x22, x9                 // row delta
    cmp     x0, #0
    cneg    x0, x0, lt
    mov     x23, #-1                    // memo index, or -1
    LDX     x9, spl_hyp_w
    cmp     x1, x9
    b.hs    spl_distance.hypot
    LDX     x10, spl_hyp_h
    cmp     x0, x10
    b.hs    spl_distance.hypot
    madd    x23, x0, x9, x1
    LDX     x9, spl_hyp
    ldr     d0, [x9, x23, lsl #3]
    fmov    x9, d0
    cbnz    x9, spl_distance.min
spl_distance.hypot:
    scvtf   d0, x1
    scvtf   d1, x0
    fadd    d1, d1, d1
    CCALL   hypot
    tbnz    x23, #63, spl_distance.min
    LDX     x9, spl_hyp
    str     d0, [x9, x23, lsl #3]
spl_distance.min:
    ldr     d1, [sp]
    fminnm  d1, d1, d0                  // f64::min, as the oracle compiles it
    str     d1, [sp]
    add     x24, x24, #16
    add     x19, x19, #1
    b       spl_distance.spotlight
spl_distance.done:
    ldr     d0, [sp]
    ldp     x24, x30, [sp, #48]
    ldp     x22, x23, [sp, #32]
    ldp     x19, x21, [sp, #16]
    add     sp, sp, #64
    ret

// spl_illuminate: SpotlightsIterator.illuminate_chars(illuminate_range).
// Each spotlightable character met in an ellipse for the first time this
// frame is stamped and listed, then the listed ones get their colors (a
// frame's colors don't depend on visiting order); then the previously
// illuminated characters without this frame's stamp go dark. The ellipse is
// coords_in_circle's, walked column by column over the canvas cells only
// (the input-coordinate map holds nothing outside them).
spl_illuminate:
    sub     sp, sp, #160
    stp     x19, x20, [sp, #96]
    stp     x21, x22, [sp, #112]
    stp     x23, x24, [sp, #128]
    str     x30, [sp, #144]
    // [sp] the ellipse's a^2/b^2 (CIRCLE_ITER), [sp, #72] last column
    // the live spotlights' coordinates; the frame repeats the last one
    // exactly when they, the range and the expand phase are unchanged
    // (spotlights move under a cell a frame), and then nothing is redone
    LDX     x9, spl_range
    LDX     x10, spl_prev_range
    cmp     x9, x10
    cset    w21, ne                     // nonzero once something changed
    STX     x9, spl_prev_range
    LDX     x9, spl_live
    LDX     x10, spl_prev_live
    cmp     x9, x10
    cset    w3, ne
    orr     w21, w21, w3
    STX     x9, spl_prev_live
    LDB     w9, spl_expanding
    LDB     w10, spl_prev_expanding
    cmp     w9, w10
    cset    w3, ne
    orr     w21, w21, w3
    STB     w9, spl_prev_expanding
    mov     x19, #0
spl_illuminate.coords:
    LDX     x9, spl_live
    cmp     x19, x9
    b.hs    spl_illuminate.changed
    LDX     x9, spl_slots
    ldr     w3, [x9, x19, lsl #2]
    LDX     x2, spl_coords
    add     x2, x2, x19, lsl #4
    LDX     x9, ch_col
    ldrsw   x9, [x9, x3, lsl #2]
    ldr     x10, [x2]
    cmp     x10, x9
    cset    w1, ne
    orr     w21, w21, w1
    str     x9, [x2]
    LDX     x9, ch_row
    ldrsw   x9, [x9, x3, lsl #2]
    ldr     x10, [x2, #8]
    cmp     x10, x9
    cset    w1, ne
    orr     w21, w21, w1
    str     x9, [x2, #8]
    add     x19, x19, #1
    b       spl_illuminate.coords
spl_illuminate.changed:
    LDB     w9, spl_lit_once
    cbz     w9, spl_illuminate.fresh
    cbz     w21, spl_illuminate.same
spl_illuminate.fresh:
    mov     w9, #1
    STB     w9, spl_lit_once
    LDW     w9, spl_stamp
    add     w9, w9, #1
    STW     w9, spl_stamp
    STX     xzr, spl_next_count
spl_illuminate.gather:
    mov     x24, #0                     // spotlight index
spl_illuminate.ellipse:
    LDX     x9, spl_live
    cmp     x24, x9
    b.hs    spl_illuminate.shine
    LDX     x1, spl_coords
    add     x1, x1, x24, lsl #4
    ldr     x9, [x1, #8]
    ldr     w3, [x1]
    orr     x1, x3, x9, lsl #32
    mov     x0, sp
    LDX     x2, spl_range
    bl      coords_in_circle_init
    ldr     x22, [sp, #CIRCLE_ITER.x]   // column
    ldr     x9, [sp, #CIRCLE_ITER.x_end]
    str     x9, [sp, #72]
spl_illuminate.column:
    ldr     x9, [sp, #72]
    cmp     x22, x9
    b.gt    spl_illuminate.next_ellipse
    cmp     x22, #1
    b.lt    spl_illuminate.next_column
    LDX     x9, canvas_right
    cmp     x22, x9
    b.gt    spl_illuminate.next_ellipse
    mov     x0, x22
    ldr     x1, [sp, #CIRCLE_ITER.h]
    ldr     x2, [sp, #CIRCLE_ITER.k]
    ldr     d2, [sp, #CIRCLE_ITER.a_squared]
    ldr     d3, [sp, #CIRCLE_ITER.b_squared]
    bl      circle_column_range
    mov     x3, #1
    cmp     x9, #1
    csel    x9, x3, x9, lt
    LDX     x10, canvas_top
    cmp     x2, x10
    csel    x2, x10, x2, gt
    sub     x21, x2, x9
    add     x21, x21, #1                // rows (<= 0: none)
    sub     x19, x9, #1
    LDX     x10, canvas_right
    mul     x19, x19, x10
    add     x19, x19, x22
    sub     x19, x19, #1                // coord_map_index
spl_illuminate.cell:
    cmp     x21, #0
    b.le    spl_illuminate.next_column
    sub     x21, x21, #1
    LDX     x9, coord_map
    ldr     w20, [x9, x19, lsl #2]
    LDX     x10, canvas_right
    add     x19, x19, x10
    cmn     w20, #1                     // NONE
    b.eq    spl_illuminate.cell
    LDX     x3, spl_marks
    LDW     w9, spl_stamp
    ldr     w10, [x3, x20, lsl #2]
    cmp     w10, w9
    b.hs    spl_illuminate.cell         // lit already, or never
    str     w9, [x3, x20, lsl #2]
    LDX     x9, spl_next
    LDX     x3, spl_next_count
    str     w20, [x9, x3, lsl #2]
    add     x3, x3, #1
    STX     x3, spl_next_count
    b       spl_illuminate.cell
spl_illuminate.next_column:
    add     x22, x22, #1
    b       spl_illuminate.column
spl_illuminate.next_ellipse:
    add     x24, x24, #1
    b       spl_illuminate.ellipse
spl_illuminate.shine:
    // the newly lit characters' colors, SPL_BATCH at a time: first each
    // one's visual or, when it needs the memo, its memo slot (prefetched)
    mov     x24, #0                     // batch start
spl_illuminate.batch:
    LDX     x21, spl_next_count
    subs    x21, x21, x24
    b.le    spl_illuminate.dark
    cmp     x21, #SPL_BATCH
    b.ls    spl_illuminate.prepare
    mov     x21, #SPL_BATCH
spl_illuminate.prepare:
    mov     x19, #0
spl_illuminate.prepare_char:
    add     x9, x24, x19
    LDX     x3, spl_next
    ldr     w20, [x3, x9, lsl #2]
    mov     x9, #SPL_REC_size
    LDX     x23, spl_recs
    madd    x23, x20, x9, x23
    bl      spl_shine
    ADRG    x1, spl_batch
    add     x1, x1, x19, lsl #4
    str     w9, [x1, #SPL_BE.visual]    // the visual, or 0: look it up
    str     w2, [x1, #SPL_BE.slot]      // its memo slot
    str     x4, [x1, #SPL_BE.factor]    // and factor bits
    add     x19, x19, #1
    cmp     x19, x21
    b.lo    spl_illuminate.prepare_char
    // the memo lookups, whose cache misses now overlap; then each
    // character shows its visual, in order
    mov     x19, #0
spl_illuminate.resolve_char:
    add     x9, x24, x19
    LDX     x3, spl_next
    ldr     w20, [x3, x9, lsl #2]
    mov     x9, #SPL_REC_size
    LDX     x23, spl_recs
    madd    x23, x20, x9, x23
    ADRG    x22, spl_batch
    add     x22, x22, x19, lsl #4       // the batch entry
    ldr     w9, [x22, #SPL_BE.visual]
    cbnz    w9, spl_illuminate.show
    ldr     w2, [x22, #SPL_BE.slot]
    ldr     x9, [x22, #SPL_BE.factor]
    bl      spl_memo_probe
    cbnz    w9, spl_illuminate.found
    // a miss: make the visual and keep it
    str     w3, [x22, #SPL_BE.slot]
    ldr     d0, [x22, #SPL_BE.factor]
    bl      spl_adjusted
    ldr     w2, [x22, #SPL_BE.slot]
    ldr     x3, [x22, #SPL_BE.factor]
    bl      spl_memo_store
spl_illuminate.found:
    str     w9, [x23, #SPL_REC.dimmed]
spl_illuminate.show:
    bl      spl_expand_override
    mov     w0, w20
    SET_HANDLE
    add     x19, x19, #1
    cmp     x19, x21
    b.lo    spl_illuminate.resolve_char
    add     x24, x24, x21
    b       spl_illuminate.batch
spl_illuminate.dark:
    // characters that left every beam go dark
    LDX     x21, spl_lit
    LDX     x22, spl_lit_count
    LDW     w24, spl_stamp
    mov     x19, #0
spl_illuminate.dark_char:
    cmp     x19, x22
    b.hs    spl_illuminate.swap
    ldr     w20, [x21, x19, lsl #2]
    add     x19, x19, #1
    LDX     x9, spl_marks
    ldr     w9, [x9, x20, lsl #2]
    cmp     w9, w24
    b.eq    spl_illuminate.dark_char
    mov     x9, #SPL_REC_size
    LDX     x23, spl_recs
    madd    x23, x20, x9, x23
    ldr     w9, [x23, #SPL_REC.dark]
    bl      spl_expand_override
    mov     w0, w20
    SET_HANDLE
    b       spl_illuminate.dark_char
spl_illuminate.swap:
    LDX     x9, spl_lit
    LDX     x3, spl_next
    STX     x3, spl_lit
    STX     x9, spl_next
    LDX     x9, spl_next_count
    STX     x9, spl_lit_count
spl_illuminate.same:
    ldr     x30, [sp, #144]
    ldp     x23, x24, [sp, #128]
    ldp     x21, x22, [sp, #112]
    ldp     x19, x20, [sp, #96]
    add     sp, sp, #160
    ret

// spl_shine(w20=slot, x23=record) -> w9 = the visual of an illuminated
// character (or 0, w2 and x4 as spl_dimmed's for spl_memo_probe): its
// bright pair, dimmed past the beam's edge by
// max(1 - (distance - edge) / (range * falloff), 0.2). A character whose
// nearest spotlight is well inside the edge by exact integer distance
// (dx^2 + (2 dy)^2 against edge^2 less a margin far above hypot's error)
// skips the hypot fold: its distance cannot exceed the edge.
// Clobbers C except x19-x24.
spl_shine:
    LDX     x9, ch_icol
    ldrsw   x1, [x9, x20, lsl #2]
    LDX     x9, ch_irow
    ldrsw   x0, [x9, x20, lsl #2]
    LDX     x4, spl_coords
    LDX     x5, spl_live
    mov     x10, #0x7fffffffffffffff    // smallest squared distance
spl_shine.nearest:
    ldr     x9, [x4]
    sub     x9, x1, x9
    mul     x9, x9, x9
    ldr     x3, [x4, #8]
    sub     x3, x0, x3
    add     x3, x3, x3
    mul     x3, x3, x3
    add     x9, x9, x3
    cmp     x9, x10
    csel    x10, x9, x10, lt
    add     x4, x4, #16
    subs    x5, x5, #1
    b.ne    spl_shine.nearest
    scvtf   d0, x10
    LDD     d1, spl_core2
    fcmp    d0, d1
    b.ge    spl_shine.measure
    ldr     w9, [x23, #SPL_REC.bright]
    ret
spl_shine.measure:
    PUSH1   x30
    bl      spl_distance
    POP1    x30
    ldr     w9, [x23, #SPL_REC.bright]
    LDD     d1, spl_edge
    fcmp    d0, d1
    b.le    spl_shine.done              // <= or unordered
    fsub    d0, d0, d1
    LDD     d1, spl_falloff_width
    fdiv    d0, d0, d1
    LDD     d1, spl_one
    fsub    d1, d1, d0
    LDD     d2, spl_dim
    fmaxnm  d1, d1, d2                  // f64::max: NaN takes 0.2
    fmov    d0, d1
    b       spl_dimmed
spl_shine.done:
    ret

// spl_expand_override(w9=handle, w20=slot, x23=record) -> w9: the
// _get_expand_color_override visual in place of w9 while expanding under
// dynamic color handling. Preserves x19-x24.
spl_expand_override:
    LDB     w3, spl_expanding
    cbz     w3, spl_expand_override.keep
    ldr     w3, [x23, #SPL_REC.flags]
    tbz     w3, #2, spl_expand_override.keep    // SPLF_OVERRIDE
    b       spl_override
spl_expand_override.keep:
    ret

// spotlights_next_frame -> w9 = 1 for a frame, 0 when done.
spotlights_next_frame:
    PUSH2   x19, x30
    LDB     w9, spl_complete
    cbnz    w9, spotlights_next_frame.finished
    // this frame's beam edge: range * (1 - falloff), and range * falloff
    LDX     x9, effect_config
    LDX     x10, spl_range
    scvtf   d0, x10
    ldr     d2, [x9, #SPOTLIGHTS.beam_falloff]
    LDD     d1, spl_one
    fsub    d1, d1, d2
    fmul    d1, d1, d0
    STD     d1, spl_edge
    fmul    d0, d0, d2
    STD     d0, spl_falloff_width
    // edge^2 less a relative 1e-9 when the edge is positive, else -1
    LDD     d0, spl_minus_one
    fcmp    d1, #0.0
    b.le    spotlights_next_frame.core  // <= or unordered
    fmul    d1, d1, d1
    LDD     d2, spl_margin
    fmul    d1, d1, d2
    fmov    d0, d1
spotlights_next_frame.core:
    STD     d0, spl_core2
    bl      spl_illuminate
    LDB     w9, spl_searching
    cbz     w9, spotlights_next_frame.paths
    LDX     x9, spl_search_left
    subs    x9, x9, #1
    STX     x9, spl_search_left
    b.ne    spotlights_next_frame.paths
    mov     x19, #0
spotlights_next_frame.center:
    LDX     x9, spl_live
    cmp     x19, x9
    b.hs    spotlights_next_frame.searched
    LDX     x9, spl_slots
    ldr     w0, [x9, x19, lsl #2]
    MOV64   w1, SPL_CENTER
    bl      path_activate_name
    add     x19, x19, #1
    b       spotlights_next_frame.center
spotlights_next_frame.searched:
    STB     wzr, spl_searching
spotlights_next_frame.paths:
    // any live spotlight still on a path?
    mov     x19, #0
spotlights_next_frame.any:
    LDX     x9, spl_live
    cmp     x19, x9
    b.hs    spotlights_next_frame.expand
    LDX     x9, spl_slots
    ldr     w3, [x9, x19, lsl #2]
    LDX     x9, ch_path
    ldr     w3, [x9, x3, lsl #2]
    cmn     w3, #1                      // NONE
    b.ne    spotlights_next_frame.update
    add     x19, x19, #1
    b       spotlights_next_frame.any
spotlights_next_frame.expand:
    mov     x9, #1
    STX     x9, spl_live
    mov     w9, #1
    STB     w9, spl_expanding
    LDX     x9, spl_range
    add     x9, x9, #1
    STX     x9, spl_range
    LDX     x9, canvas_right
    LDX     x10, canvas_top
    cmp     x9, x10
    csel    x9, x10, x9, lt
    scvtf   d0, x9
    LDD     d1, spl_one_half
    fdiv    d0, d0, d1
    FLOORSD d0
    LDX     x9, spl_range
    scvtf   d1, x9
    fcmp    d1, d0
    b.le    spotlights_next_frame.update    // <= or unordered
    mov     w9, #1
    STB     w9, spl_complete
spotlights_next_frame.update:
    bl      update
    mov     w9, #1
    b       spotlights_next_frame.done
spotlights_next_frame.finished:
    mov     w9, #0
spotlights_next_frame.done:
    POP2    x19, x30
    ret

    .section .rodata
    .balign 8
spl_space:      .quad 0x20 | (1 << 32)  // " "
spl_symbol:     .quad 'O' | (1 << 32)
spl_dim:        .double 0.2
spl_half:       .double 0.5
spl_one:        .double 1.0
spl_one_half:   .double 1.5
spl_inf:        .quad 0x7ff0000000000000
spl_minus_one:  .double -1.0
spl_margin:     .double 0.999999999

    TSTATE
    .balign 8
spl_slots:          .skip 8         // the spotlights (u32 slots)
spl_live:           .skip 8         // len(self.spotlights)
spl_coords:         .skip 8         // (column, row) i64 pairs, this frame
spl_min_distance:   .skip 8         // f64
spl_map:            .skip 8
spl_map_width:      .skip 8
spl_recs:           .skip 8
spl_lit:            .skip 8         // illuminated_chars
spl_lit_count:      .skip 8
spl_next_count:     .skip 8
spl_next:           .skip 8         // illuminated_scratch
spl_memo:           .skip 8
spl_marks:          .skip 8         // u32 per character (see the build)
spl_memo_count:     .skip 8
spl_hyp:            .skip 8
spl_hyp_w:          .skip 8
spl_hyp_h:          .skip 8
spl_range:          .skip 8         // illuminate_range
spl_search_left:    .skip 8
spl_edge:           .skip 8         // f64
spl_falloff_width:  .skip 8         // f64
spl_core2:          .skip 8         // f64: squared distances below are lit
spl_prev_range:     .skip 8         // the last illumination's inputs
spl_prev_live:      .skip 8
spl_stamp:          .skip 4
spl_prev_expanding: .skip 1
spl_lit_once:       .skip 1
    .balign 16
spl_batch:          .skip SPL_BATCH * 16    // SPL_BE entries
spl_searching:      .skip 1
spl_expanding:      .skip 1
spl_complete:       .skip 1
spl_dynamic:        .skip 1

    .text
