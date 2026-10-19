// engine/scene.s - Scene, Frame and the scene half of Animation
// (src/engine/animation.rs; stepping from src/engine/ctx.rs).
//
// A scene's frame queue is its head index: frames before the head have
// played, frames from it on remain. That is exactly Rust's frames /
// played_frames pair, because frames retire in order, reset_scene restores the
// original order, and synced/eased stepping index the remaining queue or the
// whole frame list without reordering either. Only the head frame of a plain
// scene has nonzero ticks_elapsed, so one counter per scene suffices, and the
// record caches the head frame's handle and duration.
//
// Scenes belong to a character's scene map (ch_scenes, linked through
// SC_NEXT in insertion order) and are addressed by index into [scenes].
//
// The record's cold half lies SCENE_COLD past its hot half, beyond any load
// offset: SCN_COLD makes a pointer to it, and the SCC_* offsets address the
// cold fields from there.

.equ SCC_NAME,          SC_NAME - SCENE_COLD
.equ SCC_NEXT,          SC_NEXT - SCENE_COLD
.equ SCC_EASE,          SC_EASE - SCENE_COLD
.equ SCC_EASE_TOTAL,    SC_EASE_TOTAL - SCENE_COLD
.equ SCC_EASE_STEP,     SC_EASE_STEP - SCENE_COLD
.equ SCC_OWNER,         SC_OWNER - SCENE_COLD
.equ SCC_CURSOR,        SC_CURSOR - SCENE_COLD
.equ SCC_CURSOR_START,  SC_CURSOR_START - SCENE_COLD

// SCN_COLD xdest, xrec: xdest = the cold half of the record at xrec
// (xrec + SCENE_COLD). xdest must differ from xrec.
.macro SCN_COLD xdest, xrec
    MOV64   \xdest, SCENE_COLD
    add     \xdest, \xrec, \xdest
.endm

    .text

scenes_init:
    str     x30, [sp, #-16]!
    MOV64   x0, (SCENE_COLD + SCENE_LIMIT * SCENE_SIZE)
    bl      reserve
    STX     x9, scenes
    MOV64   x0, (SCENE_LIMIT * 16)
    bl      reserve
    STX     x9, scene_pre
    MOV64   x0, FRAME_REGION
    bl      reserve
    STX     x9, frame_region
    STX     x9, frame_region_end
    ADRG    x9, shapes
    STX     x9, shape_last
    // vhstape finds a character's scenes at fixed distances from its first
    // (they are created back to back), so it keeps one run of indices
    LDX     x9, request
    ldr     x9, [x9, #RQ_EFFECT]
    cmp     x9, #EFFECT_VHSTAPE
    cset    w9, eq
    STB     w9, scene_unbanked
    ldr     x30, [sp], #16
    ret

// scene_find(w0=slot, w1=name) -> w9 = scene index or NONE. Clobbers x3,
// x12, x13, x16, x17.
scene_find:
    LDX     x9, ch_scenes
    ldr     w9, [x9, w0, uxtw #2]
    LDX     x12, scenes
    MOV64   x13, SCENE_COLD
    add     x12, x12, x13               // record 0's cold half
scene_find.next:
    cmn     w9, #1                      // NONE
    b.eq    scene_find.done
    add     x3, x12, x9, lsl #SCENE_SHIFT
    ldr     w13, [x3, #SCC_NAME]
    cmp     w13, w1
    b.eq    scene_find.done
    ldr     w9, [x3, #SCC_NEXT]
    b       scene_find.next
scene_find.done:
    ret

// scene_new(w0=slot, w1=name or AUTO, w2=SCF_LOOPING/SCF_SYNC_* flags,
//           w3=easing id or NONE) -> w9 = scene index.
// Animation.new_scene: an auto id is the scene count, probing upward past
// taken ids; an existing name is overwritten in place (it keeps its position
// in the map), faithfully. Preexisting input colors apply under
// --existing-color-handling always for characters that use them.
scene_new:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    mov     w19, w0                     // slot
    mov     w21, w1                     // name
    mov     w22, w2                     // flags
    mov     w23, w3                     // easing
    cmn     w21, #1                     // AUTO
    b.ne    scene_new.named
    // auto id: len(scenes), then upward while taken
    LDX     x9, ch_scenes
    ldr     w9, [x9, x19, lsl #2]
    mov     w21, #0
scene_new.count:
    cmn     w9, #1
    b.eq    scene_new.probe
    add     w21, w21, #1
    SCENE_PTR x3, x9
    SCN_COLD x12, x3
    ldr     w9, [x12, #SCC_NEXT]
    b       scene_new.count
scene_new.probe:
    mov     w0, w19
    mov     w1, w21
    bl      scene_find
    cmn     w9, #1
    b.eq    scene_new.fresh
    add     w21, w21, #1
    b       scene_new.probe
scene_new.named:
    mov     w0, w19
    mov     w1, w21
    bl      scene_find
    cmn     w9, #1
    b.ne    scene_new.reuse
scene_new.fresh:
    // a new record, appended to the character's map
    bl      scene_alloc
    mov     w24, w9
    SCENE_PTR x4, x24
    SCN_COLD x12, x4
    mov     w13, #NONE
    str     w13, [x12, #SCC_NEXT]
    LDX     x9, ch_scenes
    add     x9, x9, x19, lsl #2
scene_new.tail:
    ldr     w3, [x9]
    cmn     w3, #1
    b.eq    scene_new.link
    SCENE_PTR x9, x3
    MOV64   x13, SC_NEXT
    add     x9, x9, x13
    b       scene_new.tail
scene_new.link:
    str     w24, [x9]
    b       scene_new.init
scene_new.reuse:
    mov     w24, w9
    mov     w0, w19
    bl      doze_wake
    SCENE_PTR x4, x24
scene_new.init:
    // everything but the map link starts over
    SCN_COLD x12, x4
    ldr     w3, [x12, #SCC_NEXT]
    stp     xzr, xzr, [x4]
    stp     xzr, xzr, [x4, #16]
    stp     xzr, xzr, [x12]
    stp     xzr, xzr, [x12, #16]
    str     w3, [x12, #SCC_NEXT]
    str     w21, [x12, #SCC_NAME]
    str     w19, [x12, #SCC_OWNER]
    and     w9, w22, #(SCF_LOOPING | SCF_SYNC)
    cmn     w23, #1                     // NONE
    b.eq    scene_new.flags
    orr     w9, w9, #SCF_EASED
    str     w23, [x12, #SCC_EASE]
scene_new.flags:
    // existing_color_handling == always and the character uses its colors
    LDX     x3, cfg_existing_colors
    cbnz    x3, scene_new.store_flags
    LDX     x3, ch_flags
    ldrh    w3, [x3, x19, lsl #1]
    tst     w3, #CF_PREEXISTING
    b.eq    scene_new.store_flags
    orr     w9, w9, #SCF_PREEXISTING
    tst     w3, #CF_BOLD
    b.eq    scene_new.colors
    orr     w9, w9, #SCF_PRE_BOLD
scene_new.colors:
    LDX     x2, scene_pre
    add     x2, x2, x24, lsl #4
    LDX     x3, ch_fg
    ldr     x3, [x3, x19, lsl #3]
    str     x3, [x2]
    LDX     x3, ch_bg
    ldr     x3, [x3, x19, lsl #3]
    str     x3, [x2, #8]
scene_new.store_flags:
    str     w9, [x4, #SC_FLAGS]
    mov     w9, w24
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// scene_alloc(w21=name) -> w9 = a fresh scene index. Effects give every
// character the same scenes (by name), and the characters tick in slot
// order with, most of the time, the same scene active. So each name draws
// its indices from its own chunks of SCENE_CHUNK consecutive records: the
// active scenes of neighboring characters then share cache lines, instead
// of one line per character holding its active record next to an idle one.
// Names are spread over SCENE_BANKS cursors (a collision just shares a
// chunk). Clobbers x9, x3, x2 (and scene_take_free's).
scene_alloc:
    str     x30, [sp, #-16]!
    bl      scene_take_free             // particles.s: a recycled index, or NONE
    ldr     x30, [sp], #16
    cmn     w9, #1
    b.eq    scene_alloc.banked
    ret
scene_alloc.banked:
    MOV64   x13, SCENE_LIMIT
    LDB     w3, scene_unbanked
    cbnz    w3, scene_alloc.next
    MOV64   x9, 0x9E3779B1
    mul     w9, w21, w9
    lsr     w9, w9, #(32 - 6)           // log2(SCENE_BANKS)
    ADRG    x2, scene_banks
    add     x2, x2, x9, lsl #3
    ldp     w9, w3, [x2]                // next index, chunk end
    cmp     w9, w3
    b.hs    scene_alloc.chunk
    add     w3, w9, #1
    str     w3, [x2]
    ret
scene_alloc.chunk:
    LDW     w9, scene_count             // records handed out in chunks
    add     w3, w9, #SCENE_CHUNK
    cmp     w3, w13
    b.hi    scene_alloc.full
    STW     w3, scene_count
    str     w3, [x2, #4]
    add     w3, w9, #1
    str     w3, [x2]
    ret
scene_alloc.next:
    // one run of indices in creation order
    LDW     w9, scene_count
    cmp     w9, w13
    b.hs    scene_alloc.full
    add     w3, w9, #1
    STW     w3, scene_count
    ret
scene_alloc.full:
    ADRG    x0, msg_scenes_full
    mov     w1, #msg_scenes_full_len
    b       fatal

// scene_add_frame(w0=scene, x1=packed symbol, w2=duration, x3=fg or
//                 NONE, x4=bg or NONE, w5=ATTR_* bits).
// Scene.add_frame: preexisting colors replace the given ones and preexisting
// bold forces bold; a duration below 1 is an error.
scene_add_frame:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     w19, w0
    mov     w21, w2
    cmp     w2, #1
    b.lt    scene_add_frame.bad_duration
    SCENE_PTR x22, x19
    ldr     w9, [x22, #SC_FLAGS]
    tst     w9, #SCF_PREEXISTING
    b.eq    scene_add_frame.bold
    LDX     x3, scene_pre
    add     x3, x3, x19, lsl #4
    ldr     x4, [x3, #8]
    ldr     x3, [x3]
scene_add_frame.bold:
    tst     w9, #SCF_PRE_BOLD
    b.eq    scene_add_frame.visual
    orr     w5, w5, #ATTR_BOLD
scene_add_frame.visual:
    mov     x2, x1
    mov     x0, x3
    mov     x1, x4
    mov     w3, w5
    bl      visual_make
    mov     w2, w21
    mov     x4, x22
    bl      scene_append_frame
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret
scene_add_frame.bad_duration:
    sxtw    x1, w2
    ADRG    x0, msg_frame_duration
    mov     w2, #msg_frame_duration_len
    b       fail_with_number

// scene_add_frame_visual(w0=scene, w1=handle, w2=duration): add_frame
// for an effect that built (and cached) the visual itself. Under a scene's
// preexisting colors the visual is rebuilt from its header with them, so the
// result is exactly scene_add_frame's.
scene_add_frame_visual:
    cmp     w2, #1
    b.lt    scene_add_frame_visual.bad_duration
    SCENE_PTR x4, x0
    ldr     w9, [x4, #SC_FLAGS]
    tst     w9, #(SCF_PREEXISTING | SCF_PRE_BOLD)
    b.ne    scene_add_frame_visual.rebuild
    mov     w9, w1
    b       scene_append_frame
scene_add_frame_visual.rebuild:
    stp     x0, x2, [sp, #-32]!
    str     x30, [sp, #16]
    mov     w9, w1
    bl      visual_meta
    ldr     x30, [sp, #16]
    ldp     x0, x2, [sp], #32
    ldur    x1, [x9, #VH_SYMBOL]
    ldur    x3, [x9, #VH_FG]
    ldur    x4, [x9, #VH_BG]
    ldur    w5, [x9, #VH_ATTRS]
    b       scene_add_frame
scene_add_frame_visual.bad_duration:
    sxtw    x1, w2
    ADRG    x0, msg_frame_duration
    mov     w2, #msg_frame_duration_len
    b       fail_with_number

// scene_append_frame(x4=scene record, w9=handle, w2=duration): push a frame
// and keep the head cache current. Frames live in one region: a scene appends
// in place while its frames end the region - the usual case, since effects
// build one scene at a time - and otherwise first moves them to the end. So a
// character's scenes lie in creation order, which is roughly tick order.
// Preserves x9, x2, x4; clobbers x3, x1, x0, x5.
scene_append_frame:
    ldr     w3, [x4, #SC_COUNT]
    ldr     x0, [x4, #SC_FRAMES]
    add     x0, x0, x3, lsl #FRAME_SHIFT    // where the next frame goes
    LDX     x12, frame_region_end
    cmp     x0, x12
    b.ne    scene_append_frame.relocate
scene_append_frame.append:
    mov     w5, w9
    bfi     x5, x2, #32, #32
    str     x5, [x0]                    // FR_HANDLE, FR_DURATION
    add     x0, x0, #FRAME_SIZE
    STX     x0, frame_region_end
    SCN_COLD x12, x4
    ldr     w13, [x12, #SCC_EASE_TOTAL]
    add     w13, w13, w2
    str     w13, [x12, #SCC_EASE_TOTAL]
    add     w1, w3, #1
    str     w1, [x4, #SC_COUNT]
    ldr     w13, [x4, #SC_FLAGS]
    MOV64   x14, (SCF_SHAPE | SCF_SHARED)
    bic     w13, w13, w14
    str     w13, [x4, #SC_FLAGS]
    ldr     w13, [x4, #SC_HEAD]
    cmp     w3, w13
    b.ne    scene_append_frame.done
    str     w9, [x4, #SC_HEAD_HANDLE]
    str     w2, [x4, #SC_HEAD_DURATION]
    str     wzr, [x4, #SC_TICKS]        // (so no synced frame is cached)
scene_append_frame.done:
    ret
scene_append_frame.relocate:
    str     x30, [sp, #-16]!
    bl      scene_append_recycled       // particles.s: C set when it reused a block
    ldr     x30, [sp], #16
    b.cs    scene_append_frame.done
    // move this scene's frames to the end of the region
    ldr     x1, [x4, #SC_FRAMES]
    LDX     x0, frame_region_end
    str     x0, [x4, #SC_FRAMES]
    lsl     w3, w3, #FRAME_SHIFT
    REP_MOVSB
    ldr     w3, [x4, #SC_COUNT]
    b       scene_append_frame.append

// scene_append_frames(w0=scene, x1=frames, x2=count): append a list of
// frames (FRAME_SIZE records, e.g. another scene's SC_FRAMES) in one go -
// scene_add_frame_visual for each, without re-checking durations or
// rebuilding visuals for preexisting colors (callers copy frames made for
// an equivalent scene). Clobbers x9, x3, x2, x1, x0, x4, x5, x10, x11.
scene_append_frames:
    cbz     x2, scene_append_frames.none
    SCENE_PTR x4, x0
    mov     x5, x1                      // source
    mov     x10, x2                     // count
    ldr     w3, [x4, #SC_COUNT]
    ldr     x0, [x4, #SC_FRAMES]
    add     x0, x0, x3, lsl #FRAME_SHIFT
    LDX     x12, frame_region_end
    cmp     x0, x12
    b.eq    scene_append_frames.append
    // relocate this scene's frames to the end of the region
    ldr     x1, [x4, #SC_FRAMES]
    LDX     x0, frame_region_end
    str     x0, [x4, #SC_FRAMES]
    lsl     w3, w3, #FRAME_SHIFT
    REP_MOVSB
scene_append_frames.append:
    // x0 = where the first new frame goes; copy and sum the durations
    ldr     w3, [x4, #SC_COUNT]
    ldr     w13, [x4, #SC_HEAD]
    cmp     w3, w13
    b.ne    scene_append_frames.copy
    ldr     w9, [x5, #FR_HANDLE]        // the new head
    str     w9, [x4, #SC_HEAD_HANDLE]
    ldr     w9, [x5, #FR_DURATION]
    str     w9, [x4, #SC_HEAD_DURATION]
    str     wzr, [x4, #SC_TICKS]
scene_append_frames.copy:
    add     w3, w3, w10
    str     w3, [x4, #SC_COUNT]
    mov     w11, #0                     // the durations' sum
    mov     x3, #0
scene_append_frames.frame:
    ldr     x9, [x5, x3, lsl #FRAME_SHIFT]
    str     x9, [x0, x3, lsl #FRAME_SHIFT]
    lsr     x9, x9, #32
    add     w11, w11, w9
    add     x3, x3, #1
    cmp     x3, x10
    b.lo    scene_append_frames.frame
    add     x0, x0, x3, lsl #FRAME_SHIFT
    STX     x0, frame_region_end
    SCN_COLD x12, x4
    ldr     w13, [x12, #SCC_EASE_TOTAL]
    add     w13, w13, w11
    str     w13, [x12, #SCC_EASE_TOTAL]
    ldr     w13, [x4, #SC_FLAGS]
    MOV64   x14, (SCF_SHAPE | SCF_SHARED)
    bic     w13, w13, w14
    str     w13, [x4, #SC_FLAGS]
scene_append_frames.none:
    ret

// scene_load_head(x4=scene record): refresh the head cache after the head
// moved, and prefetch the frame after it. Frames retire long after they
// were written, and between two retirements of one scene every other
// character's are walked, so the next frame is out of cache by then; one
// retirement ahead is soon enough and late enough to stay cached.
// Clobbers x9, x3, x16, x17.
scene_load_head:
    ldr     w3, [x4, #SC_HEAD]
    ldr     w16, [x4, #SC_COUNT]
    cmp     w3, w16
    b.hs    scene_load_head.done
    add     w9, w3, #1
    ldr     x17, [x4, #SC_FRAMES]
    add     x3, x17, x3, lsl #FRAME_SHIFT
    cmp     w9, w16
    b.hs    scene_load_head.last
    prfm    pldl1keep, [x3, #FRAME_SIZE]
scene_load_head.last:
    ldr     w9, [x3, #FR_HANDLE]
    str     w9, [x4, #SC_HEAD_HANDLE]
    ldr     w9, [x3, #FR_DURATION]
    str     w9, [x4, #SC_HEAD_DURATION]
scene_load_head.done:
    ret

// scene_reset(w0=scene): Scene.reset_scene - every frame back in the queue
// in original order, tick counters and the easing step zeroed.
scene_reset:
    stp     x0, x30, [sp, #-32]!
    SCENE_PTR x4, x0
    str     x4, [sp, #16]
    SCN_COLD x12, x4
    ldr     w0, [x12, #SCC_OWNER]
    bl      doze_wake
    ldr     x4, [sp, #16]
    ldp     x0, x30, [sp], #32
    str     wzr, [x4, #SC_HEAD]
    str     wzr, [x4, #SC_TICKS]
    SCN_COLD x12, x4
    str     wzr, [x12, #SCC_EASE_STEP]
    b       scene_load_head

// scene_apply_gradient(w0=scene, x1=symbols (packed), x2=symbol count,
//   w3=duration, x4=fg spectrum or 0, x5=fg count, x6=bg spectrum or 0,
//   x7=bg count). Scene.apply_gradient_to_symbols with the exact
// cyclic_distribution semantics.
scene_apply_gradient:
    sub     sp, sp, #128
    stp     x19, x20, [sp, #64]
    stp     x21, x22, [sp, #80]
    stp     x23, x24, [sp, #96]
    str     x30, [sp, #112]
    stp     x0, x1, [sp]                // scene, symbols
    stp     x2, x3, [sp, #16]           // symbol count, duration
    stp     x4, x5, [sp, #32]           // fg, fg count
    stp     x6, x7, [sp, #48]           // bg, bg count
    // errors, in Rust's order
    cbnz    x4, scene_apply_gradient.some
    cbz     x7, scene_apply_gradient.none_error
scene_apply_gradient.some:
    mov     w19, #0                     // bit 0: fg has colors, bit 1: bg
    cbz     x4, scene_apply_gradient.bg_has
    ldr     x9, [sp, #40]
    cbz     x9, scene_apply_gradient.bg_has
    orr     w19, w19, #1
scene_apply_gradient.bg_has:
    ldr     x9, [sp, #48]
    cbz     x9, scene_apply_gradient.check
    ldr     x9, [sp, #56]
    cbz     x9, scene_apply_gradient.check
    orr     w19, w19, #2
scene_apply_gradient.check:
    cbz     w19, scene_apply_gradient.empty_error
    // color pairs: (fg, bg) per element of the longer spectrum
    cmp     w19, #3
    b.ne    scene_apply_gradient.single
    ldr     x9, [sp, #40]
    ldr     x12, [sp, #56]
    cmp     x9, x12
    b.lo    scene_apply_gradient.bg_longer
    // fg longer or equal: cyclic_distribution(fg, bg) -> (f, b)
    ldr     x0, [sp, #40]
    ldr     x1, [sp, #56]
    bl      cyclic_distribution         // x9 = smaller-index array
    ldr     x21, [sp, #40]              // pair count
    bl      scene_apply_gradient.pairs_alloc
    mov     x3, #0
scene_apply_gradient.fg_pairs:
    cmp     x3, x21
    b.hs    scene_apply_gradient.symbols
    ldr     x2, [sp, #32]
    ldr     x2, [x2, x3, lsl #3]
    str     x2, [x22, x3, lsl #3]
    ldr     w2, [x20, x3, lsl #2]
    ldr     x1, [sp, #48]
    ldr     x2, [x1, x2, lsl #3]
    str     x2, [x23, x3, lsl #3]
    add     x3, x3, #1
    b       scene_apply_gradient.fg_pairs
scene_apply_gradient.bg_longer:
    // cyclic_distribution(bg, fg) -> (f, b)
    ldr     x0, [sp, #56]
    ldr     x1, [sp, #40]
    bl      cyclic_distribution
    ldr     x21, [sp, #56]
    bl      scene_apply_gradient.pairs_alloc
    mov     x3, #0
scene_apply_gradient.bg_pairs:
    cmp     x3, x21
    b.hs    scene_apply_gradient.symbols
    ldr     x2, [sp, #48]
    ldr     x2, [x2, x3, lsl #3]
    str     x2, [x23, x3, lsl #3]
    ldr     w2, [x20, x3, lsl #2]
    ldr     x1, [sp, #32]
    ldr     x2, [x1, x2, lsl #3]
    str     x2, [x22, x3, lsl #3]
    add     x3, x3, #1
    b       scene_apply_gradient.bg_pairs
scene_apply_gradient.single:
    // only one side has colors
    mov     x9, #0
    ldr     x21, [sp, #40]
    ldr     x24, [sp, #32]              // source
    cmp     w19, #1
    b.eq    scene_apply_gradient.single_alloc
    ldr     x21, [sp, #56]
    ldr     x24, [sp, #48]
scene_apply_gradient.single_alloc:
    bl      scene_apply_gradient.pairs_alloc
    mov     x3, #0
scene_apply_gradient.single_pairs:
    cmp     x3, x21
    b.hs    scene_apply_gradient.symbols
    ldr     x2, [x24, x3, lsl #3]
    mov     x1, #NONE
    cmp     w19, #1
    b.ne    scene_apply_gradient.as_bg
    str     x2, [x22, x3, lsl #3]
    str     x1, [x23, x3, lsl #3]
    b       scene_apply_gradient.single_next
scene_apply_gradient.as_bg:
    str     x1, [x22, x3, lsl #3]
    str     x2, [x23, x3, lsl #3]
scene_apply_gradient.single_next:
    add     x3, x3, #1
    b       scene_apply_gradient.single_pairs
scene_apply_gradient.symbols:
    // x22/x23 = fg/bg per pair, x21 = pair count
    ldr     x9, [sp, #16]
    cmp     x9, x21
    b.lo    scene_apply_gradient.pairs_major
    // symbols major: for (symbol, colors) in cyclic(symbols, pairs)
    mov     x0, x9
    mov     x1, x21
    bl      cyclic_distribution
    mov     x20, x9
    mov     x24, #0
scene_apply_gradient.sym_frames:
    ldr     x12, [sp, #16]
    cmp     x24, x12
    b.hs    scene_apply_gradient.done
    ldr     w9, [x20, x24, lsl #2]      // pair index
    ldr     x1, [sp, #8]
    ldr     x1, [x1, x24, lsl #3]
    bl      scene_apply_gradient.add
    add     x24, x24, #1
    b       scene_apply_gradient.sym_frames
scene_apply_gradient.pairs_major:
    // for (colors, symbol) in cyclic(pairs, symbols)
    mov     x0, x21
    mov     x1, x9
    bl      cyclic_distribution
    mov     x20, x9
    mov     x24, #0
scene_apply_gradient.pair_frames:
    cmp     x24, x21
    b.hs    scene_apply_gradient.done
    ldr     w3, [x20, x24, lsl #2]      // symbol index
    ldr     x1, [sp, #8]
    ldr     x1, [x1, x3, lsl #3]
    mov     w9, w24
    bl      scene_apply_gradient.add
    add     x24, x24, #1
    b       scene_apply_gradient.pair_frames
scene_apply_gradient.done:
    ldp     x19, x20, [sp, #64]
    ldp     x21, x22, [sp, #80]
    ldp     x23, x24, [sp, #96]
    ldr     x30, [sp, #112]
    add     sp, sp, #128
    ret
scene_apply_gradient.add:
    // add_frame(symbol x1, duration, colors of pair w9); returns to the bl
    ldr     x3, [x22, x9, lsl #3]
    ldr     x4, [x23, x9, lsl #3]
    ldr     x0, [sp]
    ldr     x2, [sp, #24]
    mov     w5, #0
    b       scene_add_frame
scene_apply_gradient.pairs_alloc:
    // x20 = the distribution (if any), x22/x23 = fg/bg arrays of x21 entries
    str     x30, [sp, #-16]!
    mov     x20, x9
    lsl     x0, x21, #3
    add     x0, x0, #8
    bl      alloc
    mov     x22, x9
    lsl     x0, x21, #3
    add     x0, x0, #8
    bl      alloc
    mov     x23, x9
    ldr     x30, [sp], #16
    ret
scene_apply_gradient.none_error:
    FAIL    msg_gradient_none
scene_apply_gradient.empty_error:
    FAIL    msg_gradient_empty

// cyclic_distribution(x0=larger count, x1=smaller count) -> x9 = u32 array
// of `larger` indices into the smaller sequence, in iteration order.
cyclic_distribution:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     x21, x0
    mov     x22, x1
    lsl     x0, x21, #2
    add     x0, x0, #8
    bl      alloc
    mov     x19, x9
    udiv    x4, x21, x22                // repeat factor
    msub    x5, x4, x22, x21            // overflow
    mov     w10, #0                     // overflow used
    mov     x11, #0                     // smaller index
    mov     x1, #0                      // current repeat factor
    mov     x3, #0
cyclic_distribution.next:
    cmp     x3, x21
    b.hs    cyclic_distribution.done
    cmp     x1, x4
    b.lo    cyclic_distribution.emit
    cbz     x5, cyclic_distribution.advance
    cbz     w10, cyclic_distribution.use_overflow
    add     x11, x11, #1
    mov     x1, #0
    mov     w10, #0
    b       cyclic_distribution.emit
cyclic_distribution.use_overflow:
    mov     w10, #1
    sub     x5, x5, #1
    b       cyclic_distribution.emit
cyclic_distribution.advance:
    add     x11, x11, #1
    mov     x1, #0
cyclic_distribution.emit:
    add     x1, x1, #1
    str     w11, [x19, x3, lsl #2]
    add     x3, x3, #1
    b       cyclic_distribution.next
cyclic_distribution.done:
    mov     x9, x19
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// scene_copy(w0=slot, w1=source scene, w2=name) -> w9 = new scene
// index: a clone of the source (frames, flags, playback state) inserted
// into the character's scene map under `name`, overwriting like new_scene.
scene_copy:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     w19, w0
    mov     w21, w1
    mov     w22, w2
    SCENE_PTR x4, x21
    SCN_COLD x12, x4
    ldr     w0, [x12, #SCC_OWNER]
    bl      doze_wake
    mov     w0, w19
    mov     w1, w22
    mov     w2, #0
    mov     w3, #NONE
    bl      scene_new
    mov     w10, w9                     // the new scene
    SCENE_PTR x4, x9
    SCENE_PTR x1, x21
    SCN_COLD x12, x4
    SCN_COLD x13, x1
    // copy everything but the name, the map link and the owner
    ldr     w5, [x12, #SCC_NEXT]
    ldp     x14, x15, [x1]
    stp     x14, x15, [x4]
    ldp     x14, x15, [x1, #16]
    stp     x14, x15, [x4, #16]
    ldp     x14, x15, [x13]
    stp     x14, x15, [x12]
    ldp     x14, x15, [x13, #16]
    stp     x14, x15, [x12, #16]
    // and its preexisting colors
    LDX     x2, scene_pre
    add     x3, x2, x21, lsl #4
    ldp     x14, x15, [x3]
    add     x3, x2, x10, lsl #4
    stp     x14, x15, [x3]
    str     w5, [x12, #SCC_NEXT]
    str     w22, [x12, #SCC_NAME]
    str     w19, [x12, #SCC_OWNER]
    ldr     w14, [x4, #SC_FLAGS]
    mov     w15, #SCF_SHARED
    bic     w14, w14, w15
    str     w14, [x4, #SC_FLAGS]
    // the frames are the clone's own, at the end of the frame region
    ldr     x1, [x4, #SC_FRAMES]
    LDX     x0, frame_region_end
    str     x0, [x4, #SC_FRAMES]
    ldr     w3, [x4, #SC_COUNT]
    lsl     w3, w3, #FRAME_SHIFT
    REP_MOVSB
    STX     x0, frame_region_end
    mov     w9, w10
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// ------------------------------------------------------------ activation

// scene_activate(w0=slot, w1=scene): Animation.activate_scene - resume
// semantics: the visual is the head of the remaining queue and playback is
// not reset. Fires SCENE_ACTIVATED.
scene_activate:
    stp     x0, x1, [sp, #-32]!
    str     x30, [sp, #16]
    bl      doze_wake
    ldp     x0, x1, [sp]
    SCENE_PTR x4, x1
    ldr     w3, [x4, #SC_HEAD]
    ldr     w9, [x4, #SC_COUNT]
    cmp     w3, w9
    b.hs    scene_activate.empty
    ldr     w9, [x4, #SC_FLAGS]
    tst     w9, #SCF_SHARED
    b.ne    scene_activate.shared
    tst     w9, #SCF_SYNC
    b.eq    scene_activate.shared
    bl      scene_share
scene_activate.shared:
    LDX     x2, ch_scene
    str     w1, [x2, w0, uxtw #2]
    ldr     w9, [x4, #SC_HEAD_HANDLE]
    SET_HANDLE
    LDX     x9, ch_subs
    ldrb    w9, [x9, w0, uxtw]
    tbz     w9, #EV_SCENE_ACTIVATED, scene_activate.done
    SCN_COLD x12, x4
    ldr     w3, [x12, #SCC_NAME]
    mov     w1, #EV_SCENE_ACTIVATED
    mov     w2, #CALLER_SCENE
    ldr     x30, [sp, #16]
    add     sp, sp, #32
    b       handle_event
scene_activate.done:
    ldr     x30, [sp, #16]
    add     sp, sp, #32
    ret
scene_activate.empty:
    FAIL    msg_scene_empty

// ------------------------------------------------------------ shared frames
//
// Effects often give many characters identical frame lists (a gradient
// toward the same color with the same symbol). Synced stepping reads a
// list every tick, so private copies spread the working set over the whole
// frame region. When a synced scene is activated its frames are looked up
// by content in share_table, and the scene then points at the first list
// seen with that content. (Plain scenes read their frames once per frame,
// and there the lookups cost more than they save.) Lists are only ever
// appended to, and scene_append_frame appends in place only at the end of
// the frame region (anywhere else it moves the scene's frames first), so no
// scene can change another's frames. Appending clears SCF_SHARED, and the
// next activation looks the list up again.

// scene_share(x4=scene record): point the scene at the shared copy of its
// frames. Preserves x0, x1, x4; clobbers x9, x3, x2, x5, x10, x11.
scene_share:
    ldr     w9, [x4, #SC_FLAGS]
    orr     w9, w9, #SCF_SHARED
    str     w9, [x4, #SC_FLAGS]
    LDB     w9, share_off
    cbnz    w9, scene_share.done
    ldr     w3, [x4, #SC_COUNT]
    cmp     w3, #SHARE_MAX_FRAMES
    b.hi    scene_share.done
    LDX     x5, share_table
    cbnz    x5, scene_share.hash
    stp     x0, x1, [sp, #-32]!
    stp     x4, x30, [sp, #16]
    MOV64   x0, (SHARE_SIZE * 16)
    bl      reserve
    ldp     x4, x30, [sp, #16]
    ldp     x0, x1, [sp], #32
    STX     x9, share_table
    mov     x5, x9
    ldr     w3, [x4, #SC_COUNT]
scene_share.hash:
    // the hash: a running sum of the frames (handle and duration) and the
    // xor of its prefixes, which is order-sensitive and one cycle a frame
    ldr     x10, [x4, #SC_FRAMES]
    mov     x9, x3                      // the sum, seeded with the count
    mov     x2, #0                      // the xor of the prefix sums
scene_share.fold:
    ldr     x12, [x10]
    add     x9, x9, x12
    eor     x2, x2, x9
    add     x10, x10, #FRAME_SIZE
    subs    w3, w3, #1
    b.ne    scene_share.fold
    MOV64   x11, 0x9E3779B97F4A7C15
    mul     x9, x9, x11
    ror     x2, x2, #(64 - 29)
    eor     x9, x9, x2
    mul     x9, x9, x11
    mov     x10, x9                     // the hash
    lsr     x9, x9, #(64 - SHARE_BITS)
scene_share.probe:
    add     x2, x5, x9, lsl #4          // the entry
    ldr     x3, [x2]
    cbz     x3, scene_share.insert
    ldr     w12, [x2, #12]
    cmp     w12, w10
    b.ne    scene_share.next
    ldr     w11, [x4, #SC_COUNT]
    ldr     w12, [x2, #8]
    cmp     w12, w11
    b.ne    scene_share.next
    // compare the lists
    ldr     x12, [x4, #SC_FRAMES]
    mov     x13, x3
scene_share.compare:
    ldr     x3, [x12]
    ldr     x14, [x13]
    cmp     x3, x14
    b.ne    scene_share.next
    add     x12, x12, #FRAME_SIZE
    add     x13, x13, #FRAME_SIZE
    subs    w11, w11, #1
    b.ne    scene_share.compare
    ldr     x3, [x2]
    str     x3, [x4, #SC_FRAMES]
    LDW     w12, share_hits
    add     w12, w12, #1
    STW     w12, share_hits
    ret
scene_share.next:
    add     w9, w9, #1
    and     w9, w9, #(SHARE_SIZE - 1)
    b       scene_share.probe
scene_share.insert:
    // a new list; when few lists repeat, looking them up costs more than
    // sharing saves, so the lookups stop for the rest of the run
    LDW     w3, share_count
    mov     w12, #(SHARE_SIZE * 3 / 4)
    cmp     w3, w12
    b.hs    scene_share.off
    cmp     w3, #SHARE_PROBATION
    b.lo    scene_share.keep
    LDW     w11, share_hits
    lsl     w11, w11, #2
    cmp     w11, w3
    b.lo    scene_share.off             // under one hit in four new lists
scene_share.keep:
    add     w12, w3, #1
    STW     w12, share_count
    ldr     x3, [x4, #SC_FRAMES]
    str     x3, [x2]
    ldr     w3, [x4, #SC_COUNT]
    str     w3, [x2, #8]
    str     w10, [x2, #12]
scene_share.done:
    ret
scene_share.off:
    mov     w12, #1
    STB     w12, share_off
    ret

// scene_activate_name(w0=slot, w1=name)
scene_activate_name:
    stp     x0, x30, [sp, #-16]!
    bl      scene_find
    ldp     x0, x30, [sp], #16
    cmn     w9, #1
    b.eq    scene_activate_name.missing
    mov     w1, w9
    b       scene_activate
scene_activate_name.missing:
    FAIL    msg_scene_missing

// scene_deactivate(w0=slot, w1=name or NONE): Animation.deactivate_scene -
// any active scene, or only the named one.
scene_deactivate:
    stp     x0, x1, [sp, #-32]!
    str     x30, [sp, #16]
    bl      doze_wake
    ldr     x30, [sp, #16]
    ldp     x0, x1, [sp], #32
    LDX     x9, ch_scene
    ldr     w3, [x9, w0, uxtw #2]
    cmn     w3, #1
    b.eq    scene_deactivate.done
    cmn     w1, #1
    b.eq    scene_deactivate.clear
    SCENE_PTR x2, x3
    SCN_COLD x12, x2
    ldr     w13, [x12, #SCC_NAME]
    cmp     w13, w1
    b.ne    scene_deactivate.done
scene_deactivate.clear:
    mov     w12, #NONE
    str     w12, [x9, w0, uxtw #2]
    MARK_CANDIDATE
scene_deactivate.done:
    ret

// scene_is_complete(w0=slot) -> w9 = 1 when Animation.active_scene_is_complete
// (no scene, no remaining frames, or a looping scene). Clobbers x3, x12,
// x16, x17.
scene_is_complete:
    LDX     x9, ch_scene
    ldr     w9, [x9, w0, uxtw #2]
    cmn     w9, #1
    b.eq    scene_is_complete.yes
    SCENE_PTR x3, x9
    ldr     w12, [x3, #SC_FLAGS]
    tst     w12, #SCF_LOOPING
    b.ne    scene_is_complete.yes
    ldr     w9, [x3, #SC_HEAD]
    ldr     w12, [x3, #SC_COUNT]
    cmp     w9, w12
    b.eq    scene_is_complete.yes
    mov     w9, #0
    ret
scene_is_complete.yes:
    mov     w9, #1
    ret

// ------------------------------------------------------------ stepping

// step_animation(w0=slot): Animation.step_animation plus
// _complete_scene_if_finished, SCENE_COMPLETE dispatch included.
// step_animation_awake is the same for a character known not to be dozing.
step_animation:
    LDX     x9, doze_bits
    lsr     w3, w0, #6
    ldr     x9, [x9, x3, lsl #3]
    lsr     x9, x9, x0
    tbz     x9, #0, step_animation_awake
    stp     x0, x30, [sp, #-16]!
    bl      doze_wake
    ldp     x0, x30, [sp], #16
step_animation_awake:
    sub     sp, sp, #32                 // x30; x0, x4 around doze_try
    str     x30, [sp]
    LDX     x9, ch_scene
    ldr     w1, [x9, w0, uxtw #2]
    cmn     w1, #1
    b.eq    step_animation_awake.done
    SCENE_PTR x4, x1
    ldr     w3, [x4, #SC_HEAD]
    ldr     w12, [x4, #SC_COUNT]
    cmp     w3, w12
    b.hs    step_animation_awake.done   // no remaining frames: nothing to step
    ldr     w9, [x4, #SC_FLAGS]
    tst     w9, #SCF_SYNC
    b.ne    step_animation_awake.synced
    tst     w9, #SCF_EASED
    b.ne    step_animation_awake.eased
    // get_next_visual: the head frame's visual (usually shown already)
    ldr     w9, [x4, #SC_HEAD_HANDLE]
    LDX     x2, ch_handle
    ldr     w12, [x2, w0, uxtw #2]
    cmp     w12, w9
    b.eq    step_animation_awake.shown
    SET_HANDLE
step_animation_awake.shown:
    ldr     w9, [x4, #SC_TICKS]
    add     w9, w9, #1
    ldr     w12, [x4, #SC_HEAD_DURATION]
    cmp     w9, w12
    b.ne    step_animation_awake.ticked
    // the head frame retires
    mov     w9, #0
    ldr     w3, [x4, #SC_HEAD]
    add     w3, w3, #1
    ldr     w12, [x4, #SC_COUNT]
    cmp     w3, w12
    b.ne    step_animation_awake.advance
    ldr     w12, [x4, #SC_FLAGS]
    tst     w12, #SCF_LOOPING
    b.eq    step_animation_awake.exhausted
    mov     w3, #0
step_animation_awake.advance:
    str     w3, [x4, #SC_HEAD]
    str     w9, [x4, #SC_TICKS]
    bl      scene_load_head
    b       step_animation_awake.check
step_animation_awake.exhausted:
    str     w3, [x4, #SC_HEAD]
    str     w9, [x4, #SC_TICKS]
    b       step_animation_awake.check
step_animation_awake.ticked:
    str     w9, [x4, #SC_TICKS]
    LDW     w12, doze_slot
    cmp     w0, w12
    b.ne    step_animation_awake.check
    // update's tick: the next duration - ticks ticks are pure but for the
    // last one when this is the last frame (its retirement completes)
    ldr     w12, [x4, #SC_FLAGS]
    tst     w12, #SCF_LOOPING
    b.ne    step_animation_awake.check
    ldr     w1, [x4, #SC_HEAD_DURATION]
    sub     w1, w1, w9
    ldr     w3, [x4, #SC_HEAD]
    add     w3, w3, #1
    ldr     w12, [x4, #SC_COUNT]
    cmp     w3, w12
    b.lo    1f                          // not the last frame
    sub     w1, w1, #1
1:  cbz     w1, step_animation_awake.check
    stp     x0, x4, [sp, #16]
    bl      doze_try
    ldp     x0, x4, [sp, #16]
    ldr     w12, [x4, #SC_TICKS]
    add     w12, w12, w1
    str     w12, [x4, #SC_TICKS]
    b       step_animation_awake.check
step_animation_awake.synced:
    bl      step_synced_scene
    b       step_animation_awake.check
step_animation_awake.eased:
    bl      step_eased_scene
step_animation_awake.check:
    // _complete_scene_if_finished
    ldr     w12, [x4, #SC_FLAGS]
    tst     w12, #SCF_LOOPING
    b.ne    step_animation_awake.complete
    ldr     w3, [x4, #SC_HEAD]
    ldr     w12, [x4, #SC_COUNT]
    cmp     w3, w12
    b.ne    step_animation_awake.done
    // reset_scene, then no active scene
    str     wzr, [x4, #SC_HEAD]
    str     wzr, [x4, #SC_TICKS]
    SCN_COLD x12, x4
    str     wzr, [x12, #SCC_EASE_STEP]
    bl      scene_load_head
    LDX     x9, ch_scene
    mov     w12, #NONE
    str     w12, [x9, w0, uxtw #2]
step_animation_awake.complete:
    // SCENE_COMPLETE fires every tick for looping scenes, faithfully
    MARK_CANDIDATE
    LDX     x9, ch_subs
    ldrb    w9, [x9, w0, uxtw]
    tbz     w9, #EV_SCENE_COMPLETE, step_animation_awake.done
    SCN_COLD x12, x4
    ldr     w3, [x12, #SCC_NAME]
    mov     w1, #EV_SCENE_COMPLETE
    mov     w2, #CALLER_SCENE
    ldr     x30, [sp]
    add     sp, sp, #32
    b       handle_event
step_animation_awake.done:
    ldr     x30, [sp]
    add     sp, sp, #32
    ret

// step_synced_scene(w0=slot, x4=scene record): Animation._step_synced_scene.
// Preserves x0 and x4.
step_synced_scene:
    str     x30, [sp, #-16]!
    LDX     x9, ch_path
    ldr     w9, [x9, w0, uxtw #2]
    cmn     w9, #1
    b.ne    step_synced_scene.path
    // no active path: jump to the final frame and force completion
    ldr     w3, [x4, #SC_COUNT]
    sub     w3, w3, #1
    ldr     x12, [x4, #SC_FRAMES]
    add     x3, x12, x3, lsl #FRAME_SHIFT
    ldr     w9, [x3, #FR_HANDLE]
    SET_HANDLE
    ldr     w3, [x4, #SC_COUNT]
    str     w3, [x4, #SC_HEAD]
    ldr     x30, [sp], #16
    ret
step_synced_scene.path:
    // (the x86 TIER 3+ path_sync_index shortcut is left out)
    bl      path_view                   // motion.s: x2 = the path's step fields
    ldr     w3, [x4, #SC_COUNT]
    ldr     w12, [x4, #SC_HEAD]
    sub     w3, w3, w12
    sub     w3, w3, #1
    sxtw    x5, w3                      // final_frame_index
    ldr     w12, [x4, #SC_FLAGS]
    tst     w12, #SCF_SYNC_STEP
    b.eq    step_synced_scene.distance
    // max(current_step, 1) / max(max_steps, 1)
    mov     x3, #1
    ldr     x9, [x2, #PA_STEP]
    cmp     x9, x3
    csel    x9, x3, x9, lt
    scvtf   d0, x9
    ldr     x9, [x2, #PA_MAX]
    cmp     x9, x3
    csel    x9, x3, x9, lt
    scvtf   d1, x9
    fdiv    d0, d0, d1
    fmov    d2, d0
    b       step_synced_scene.index
step_synced_scene.distance:
    // total = max(total_distance, 1); remaining = max(total_distance - last, 1)
    // reached = max(total - remaining, 1); ratio = reached / total
    // (f64::max: fmaxnm, as the oracle has it; a NaN yields the 1.0)
    fmov    d3, #1.0
    ldr     d0, [x2, #PA_TOTAL]
    fmaxnm  d0, d0, d3
    ldr     d1, [x2, #PA_TOTAL]
    ldr     d4, [x2, #PA_LAST]
    fsub    d1, d1, d4
    fmaxnm  d1, d1, d3
    fsub    d2, d0, d1
    fmaxnm  d2, d2, d3
    fdiv    d2, d2, d0
step_synced_scene.index:
    // round(final * ratio).min(final).max(0)
    scvtf   d0, x5
    fmul    d0, d0, d2
    ROUND_HALF_EVEN
    cmp     x9, x5
    csel    x9, x5, x9, gt
    cmp     x9, #0
    csel    x9, xzr, x9, lt
step_synced_scene.indexed:
    ldr     w12, [x4, #SC_HEAD]
    add     w9, w9, w12
    // the frame shown last time is cached (SC_SYNC_POS/SC_SYNC_HANDLE): a
    // character usually takes several steps per frame, and the frame lists
    // are out of cache by the next tick. A position's frame never changes
    // (lists are only appended to, and shared lists are equal).
    add     w3, w9, #1
    ldr     w12, [x4, #SC_SYNC_POS]
    cmp     w3, w12
    b.ne    step_synced_scene.load
    ldr     w9, [x4, #SC_SYNC_HANDLE]
    b       step_synced_scene.loaded
step_synced_scene.load:
    str     w3, [x4, #SC_SYNC_POS]
    ldr     x12, [x4, #SC_FRAMES]
    add     x9, x12, x9, lsl #FRAME_SHIFT
    ldr     w9, [x9, #FR_HANDLE]
    str     w9, [x4, #SC_SYNC_HANDLE]
step_synced_scene.loaded:
    LDX     x3, ch_handle
    ldr     w12, [x3, w0, uxtw #2]
    cmp     w12, w9
    b.eq    step_synced_scene.same
    SET_HANDLE
step_synced_scene.same:
    ldr     x30, [sp], #16
    ret

// ------------------------------------------------------------ eased shapes
//
// An eased scene's tick index is a pure function of (easing, step, total),
// and effects usually give many characters the same shape, often with the
// same frames too. A shape record memoizes, per step, index + 1 and - for
// scenes whose frames equal its reference frames (a copy of the first
// scene's) - the visual shown; 0 is unknown in both. A scene's SC_FLAGS
// remember which shape its frames were compared with (the tag, shape index
// + 1) and whether they are the same; appending a frame clears both.


// shape_find(x4=eased scene record) -> x5 = its shape record, or 0 when
// shapes are not memoized for it (too long, or the table is full).
// Clobbers x9, x3, x2 (preserves x0, x1).
shape_find:
    SCN_COLD x12, x4
    ldr     w9, [x12, #SCC_EASE]
    ldr     w3, [x12, #SCC_EASE_TOTAL]
    orr     x9, x3, x9, lsl #32         // easing << 32 | total steps
    LDX     x5, shape_last
    ldr     x13, [x5, #SH_KEY]
    cmp     x9, x13
    b.eq    shape_find.found
    ADRG    x5, shapes
    LDW     w2, shape_count
shape_find.search:
    cbz     w2, shape_find.claim
    ldr     x13, [x5, #SH_KEY]
    cmp     x9, x13
    b.eq    shape_find.hit
    add     x5, x5, #(1 << SHAPE_SHIFT)
    sub     w2, w2, #1
    b       shape_find.search
shape_find.claim:
    LDW     w13, shape_count
    cmp     w13, #SHAPE_LIMIT
    b.hs    shape_find.none
    mov     w14, #EASED_MEMO_LIMIT
    cmp     w3, w14
    b.hi    shape_find.none
    add     w13, w13, #1
    STW     w13, shape_count
    str     x9, [x5, #SH_KEY]
    stp     x0, x1, [sp, #-48]!
    stp     x4, x5, [sp, #16]
    str     x30, [sp, #32]
    lsl     x0, x3, #2
    add     x0, x0, #8
    bl      alloc
    ldp     x4, x5, [sp, #16]
    str     x9, [x5, #SH_INDEX]
    SCN_COLD x12, x4
    ldr     w3, [x12, #SCC_EASE_TOTAL]
    lsl     x0, x3, #2
    add     x0, x0, #8
    bl      alloc
    ldp     x4, x5, [sp, #16]
    str     x9, [x5, #SH_HANDLE]
    ldr     w3, [x4, #SC_COUNT]
    str     w3, [x5, #SH_COUNT]
    lsl     x0, x3, #3
    add     x0, x0, #8
    bl      alloc
    ldp     x4, x5, [sp, #16]
    str     x9, [x5, #SH_REF]
    mov     x0, x9
    ldr     x1, [x4, #SC_FRAMES]
    ldr     w3, [x4, #SC_COUNT]
    REP_MOVSQ
    ldr     x30, [sp, #32]
    ldp     x0, x1, [sp], #48
shape_find.hit:
    STX     x5, shape_last
shape_find.found:
    ret
shape_find.none:
    mov     x5, #0
    ret

// SCN_SHAPE_TAG xdest, xshape: xdest = the shape's tag in SC_FLAGS'
// position.
.macro SCN_SHAPE_TAG xdest, xshape
    ADRG    \xdest, shapes
    sub     \xdest, \xshape, \xdest
    lsr     \xdest, \xdest, #SHAPE_SHIFT
    add     \xdest, \xdest, #1
    lsl     \xdest, \xdest, #SCF_SHAPE_TAG_SHIFT
.endm

// shape_check(x4=scene record, x5=its shape): compare the scene's frames
// with the shape's reference frames, recording the tag and the verdict in
// SC_FLAGS. Clobbers x9, x3, x2 (preserves x0, x1).
shape_check:
    SCN_SHAPE_TAG x2, x5
    ldr     w9, [x4, #SC_FLAGS]
    mov     w12, #SCF_SHAPE
    bic     w9, w9, w12
    orr     w2, w2, w9
    ldr     w3, [x4, #SC_COUNT]
    ldr     w12, [x5, #SH_COUNT]
    cmp     w3, w12
    b.ne    shape_check.store
    ldr     x12, [x4, #SC_FRAMES]
    ldr     x13, [x5, #SH_REF]
shape_check.compare:
    ldr     x9, [x12]
    ldr     x14, [x13]
    cmp     x9, x14
    b.ne    shape_check.store
    add     x12, x12, #FRAME_SIZE
    add     x13, x13, #FRAME_SIZE
    subs    w3, w3, #1
    b.ne    shape_check.compare
shape_check.same:
    orr     w2, w2, #SCF_SHAPE_SAME
shape_check.store:
    str     w2, [x4, #SC_FLAGS]
    ret
// dropped: shape_check.compare8 shape_check.compare_tail (TIER 4 only)

// step_eased_scene(w0=slot, x4=scene record): Animation._step_eased_scene.
// Preserves x0 and x4 (the easing call clobbers everything else).
step_eased_scene:
    str     x30, [sp, #-48]!            // x30; x0, x4, x5 around calls
    bl      shape_find
    cbz     x5, step_eased_scene.compute
    SCN_SHAPE_TAG x9, x5
    ldr     w3, [x4, #SC_FLAGS]
    and     w3, w3, #SCF_SHAPE_TAG
    cmp     w3, w9
    b.eq    step_eased_scene.checked
    bl      shape_check
step_eased_scene.checked:
    SCN_COLD x12, x4
    ldr     w2, [x12, #SCC_EASE_STEP]
    ldr     w13, [x4, #SC_FLAGS]
    tst     w13, #SCF_SHAPE_SAME
    b.eq    step_eased_scene.memo_index
    ldr     x3, [x5, #SH_HANDLE]
    ldr     w9, [x3, x2, lsl #2]
    cbnz    w9, step_eased_scene.show
step_eased_scene.memo_index:
    ldr     x3, [x5, #SH_INDEX]
    ldr     w9, [x3, x2, lsl #2]
    cbz     w9, step_eased_scene.compute
    sub     w9, w9, #1
    b       step_eased_scene.index_map
step_eased_scene.compute:
    stp     x0, x4, [sp, #16]
    str     x5, [sp, #32]
    SCN_COLD x12, x4
    ldr     w9, [x12, #SCC_EASE_STEP]
    scvtf   d0, x9
    ldr     w9, [x12, #SCC_EASE_TOTAL]
    scvtf   d1, x9
    fdiv    d0, d0, d1
    ldr     w0, [x12, #SCC_EASE]
    bl      ease
    ldr     x4, [sp, #24]
    // final = max(total - 1, 0); index = round(factor * final).min(final).max(0)
    SCN_COLD x12, x4
    ldr     w9, [x12, #SCC_EASE_TOTAL]
    sub     x9, x9, #1
    cmp     x9, #0
    csel    x9, xzr, x9, lt
    mov     x5, x9
    scvtf   d1, x9
    fmul    d0, d0, d1
    bl      round_half_even             // pycompat.s: clobbers x9, x16, x17
    cmp     x9, x5
    csel    x9, x5, x9, gt
    cmp     x9, #0
    csel    x9, xzr, x9, lt
    ldr     x5, [sp, #32]
    ldp     x0, x4, [sp, #16]
    cbz     x5, step_eased_scene.index_map
    ldr     x3, [x5, #SH_INDEX]
    SCN_COLD x12, x4
    ldr     w2, [x12, #SCC_EASE_STEP]
    add     w10, w9, #1
    str     w10, [x3, x2, lsl #2]
step_eased_scene.index_map:
    // frame_index_map[index]: the frame whose tick range holds index. A
    // cursor (frame, its first tick) walks from the previous lookup, so the
    // usual small moves cost a step or two.
    ldr     x1, [x4, #SC_FRAMES]
    SCN_COLD x12, x4
    ldr     w3, [x12, #SCC_CURSOR]
    ldr     w2, [x12, #SCC_CURSOR_START]
step_eased_scene.back:
    cmp     w9, w2
    b.hs    step_eased_scene.forward
    sub     w3, w3, #1
    add     x13, x1, x3, lsl #FRAME_SHIFT
    ldr     w13, [x13, #FR_DURATION]
    sub     w2, w2, w13
    b       step_eased_scene.back
step_eased_scene.forward:
    add     x13, x1, x3, lsl #FRAME_SHIFT
    ldr     w13, [x13, #FR_DURATION]
    add     w10, w2, w13
    cmp     w9, w10
    b.lo    step_eased_scene.found
    mov     w2, w10
    add     w3, w3, #1
    b       step_eased_scene.forward
step_eased_scene.found:
    str     w3, [x12, #SCC_CURSOR]
    str     w2, [x12, #SCC_CURSOR_START]
    add     x13, x1, x3, lsl #FRAME_SHIFT
    ldr     w9, [x13, #FR_HANDLE]
    // the shape remembers it for every scene with the same frames
    cbz     x5, step_eased_scene.show
    ldr     w13, [x4, #SC_FLAGS]
    tst     w13, #SCF_SHAPE_SAME
    b.eq    step_eased_scene.show
    ldr     x3, [x5, #SH_HANDLE]
    ldr     w2, [x12, #SCC_EASE_STEP]
    str     w9, [x3, x2, lsl #2]
step_eased_scene.show:
    SET_HANDLE
    // advance; the end either loops or empties the queue
    SCN_COLD x12, x4
    ldr     w9, [x12, #SCC_EASE_STEP]
    add     w9, w9, #1
    str     w9, [x12, #SCC_EASE_STEP]
    ldr     w13, [x12, #SCC_EASE_TOTAL]
    cmp     w9, w13
    b.ne    step_eased_scene.stepped
    ldr     w13, [x4, #SC_FLAGS]
    tst     w13, #SCF_LOOPING
    b.eq    step_eased_scene.played
    str     wzr, [x12, #SCC_EASE_STEP]
    b       step_eased_scene.done
step_eased_scene.played:
    ldr     w9, [x4, #SC_COUNT]
    str     w9, [x4, #SC_HEAD]
step_eased_scene.done:
    ldr     x30, [sp], #48
    ret
step_eased_scene.stepped:
    // update's tick: the next steps that show the same visual and do not
    // end the scene are pure - known from the shape's visuals when the
    // scene has its reference frames, else from its indexes and the frame
    // just shown (the cursor's)
    LDW     w13, doze_slot
    cmp     w0, w13
    b.ne    step_eased_scene.done
    cbz     x5, step_eased_scene.done
    ldr     w13, [x4, #SC_FLAGS]
    tst     w13, #SCF_LOOPING
    b.ne    step_eased_scene.done
    ldr     w3, [x12, #SCC_EASE_TOTAL]
    sub     w3, w3, w9
    subs    w3, w3, #1                  // steps before the last
    b.le    step_eased_scene.done
    mov     w2, #254
    cmp     w3, w2
    csel    w3, w2, w3, hi
    mov     w2, w9
    LDX     x9, ch_handle
    ldr     w10, [x9, w0, uxtw #2]      // the visual shown
    ldr     w13, [x4, #SC_FLAGS]
    tst     w13, #SCF_SHAPE_SAME
    b.eq    step_eased_scene.by_index
    ldr     x1, [x5, #SH_HANDLE]
    add     x1, x1, x2, lsl #2
    mov     w9, #0
step_eased_scene.same:
    cmp     w9, w3
    b.hs    step_eased_scene.scanned
    ldr     w13, [x1, x9, lsl #2]
    cmp     w13, w10                    // 0 (not known yet) never matches
    b.ne    step_eased_scene.scanned
    add     w9, w9, #1
    b       step_eased_scene.same
step_eased_scene.by_index:
    ldr     x1, [x4, #SC_FRAMES]
    ldr     w9, [x12, #SCC_CURSOR]
    add     x13, x1, x9, lsl #FRAME_SHIFT
    ldr     w10, [x13, #FR_DURATION]
    ldr     x1, [x5, #SH_INDEX]
    add     x1, x1, x2, lsl #2
    ldr     w5, [x12, #SCC_CURSOR_START]
    mov     w9, #0
step_eased_scene.scan:
    cmp     w9, w3
    b.hs    step_eased_scene.scanned
    ldr     w2, [x1, x9, lsl #2]
    cbz     w2, step_eased_scene.scanned    // not known yet
    sub     w2, w2, #1
    sub     w2, w2, w5
    cmp     w2, w10
    b.hs    step_eased_scene.scanned        // another frame
    add     w9, w9, #1
    b       step_eased_scene.scan
step_eased_scene.scanned:
    cbz     w9, step_eased_scene.done
    mov     w1, w9
    stp     x0, x4, [sp, #16]
    bl      doze_try
    ldp     x0, x4, [sp, #16]
    SCN_COLD x12, x4
    ldr     w13, [x12, #SCC_EASE_STEP]
    add     w13, w13, w1
    str     w13, [x12, #SCC_EASE_STEP]
    ldr     x30, [sp], #48
    ret

// ------------------------------------------------------------ visual memo

// visual_memo(x0=fg, x1=bg, x2=packed symbol, w3=ATTR_* bits) -> w9:
// visual_make behind a small direct-mapped cache of recent visuals.
// visual_make's pool lookup compares the header stored with the visual,
// a cache miss in a pool of thousands; appearances mostly repeat a few
// recent visuals, which this finds in one line. (Scene frames repeat less
// closely: there it measured neutral, so they call visual_make.) Handles never
// change within a run, so a cached one stays right. Same clobbers as
// visual_make (x9, x3, x2, x1, x0, x4, x5, x10, x11 and more when it formats).
visual_memo:
    MOV64   x4, 0x9E3779B97F4A7C15
    mul     x9, x0, x4
    eor     x9, x9, x2
    ror     x5, x1, #(64 - 17)
    eor     x9, x9, x5
    eor     x9, x9, x3
    mul     x9, x9, x4
    lsr     x9, x9, #(64 - VMEMO_BITS)
    lsl     w9, w9, #5
    ADRG    x4, vmemo
    add     x4, x4, x9                  // the entry: symbol, fg, bg, handle | attrs << 32
    ldr     x12, [x4]
    cmp     x12, x2
    b.ne    visual_memo.miss
    ldr     x12, [x4, #8]
    cmp     x12, x0
    b.ne    visual_memo.miss
    ldr     x12, [x4, #16]
    cmp     x12, x1
    b.ne    visual_memo.miss
    ldr     w12, [x4, #28]
    cmp     w12, w3
    b.ne    visual_memo.miss
    ldr     w9, [x4, #24]
    ret
visual_memo.miss:
    stp     x4, x2, [sp, #-48]!
    stp     x0, x1, [sp, #16]
    stp     x3, x30, [sp, #32]
    bl      visual_make
    ldp     x3, x30, [sp, #32]
    ldp     x0, x1, [sp, #16]
    ldp     x4, x2, [sp], #48
    str     x2, [x4]
    str     x0, [x4, #8]
    str     x1, [x4, #16]
    str     w9, [x4, #24]
    str     w3, [x4, #28]
    ret

// ------------------------------------------------------------ appearance

// set_appearance(w0=slot, x1=packed symbol or 0 for the input symbol,
//                x2=fg or NONE, x3=bg or NONE): Animation.set_appearance.
// Under --existing-color-handling always, a character that uses its input
// colors shows those (and its bold) instead.
set_appearance:
    stp     x19, x30, [sp, #-48]!
    stp     x0, x1, [sp, #16]
    stp     x2, x3, [sp, #32]
    bl      doze_wake
    ldp     x0, x1, [sp, #16]
    ldp     x2, x3, [sp, #32]
    mov     w19, w0
    cbnz    x1, set_appearance.symbol
    LDX     x9, ch_sym
    ldr     x1, [x9, x19, lsl #3]
set_appearance.symbol:
    mov     w5, #0                      // attributes
    LDX     x9, cfg_existing_colors
    cbnz    x9, set_appearance.make
    LDX     x9, ch_flags
    ldrh    w9, [x9, x19, lsl #1]
    tst     w9, #CF_PREEXISTING
    b.eq    set_appearance.make
    LDX     x2, ch_fg
    ldr     x2, [x2, x19, lsl #3]
    LDX     x3, ch_bg
    ldr     x3, [x3, x19, lsl #3]
    tst     w9, #CF_BOLD
    b.eq    set_appearance.make
    mov     w5, #ATTR_BOLD
set_appearance.make:
    mov     x0, x2
    mov     x2, x1
    mov     x1, x3
    mov     w3, w5
    bl      visual_memo
    mov     w0, w19
    SET_HANDLE
    ldp     x19, x30, [sp], #48
    ret

// reset_appearance(w0=slot): the RESET_APPEARANCE action - the input symbol
// with no colors (subject to set_appearance's existing-color rule).
reset_appearance:
    mov     x1, #0
    mov     x2, #NONE
    mov     x3, #NONE
    b       set_appearance

    .section .rodata
// dropped: scene_one (fmov d3, #1.0 in step_synced_scene)
STRING msg_scenes_full, "ttfx: asm engine: scene limit reached\n"
STRING msg_scene_empty, "activate_scene: empty scene"
STRING msg_scene_missing, "activate_scene: scene not found"
STRING msg_frame_duration, "Frame duration must be at least 1. Received: "
STRING msg_gradient_none, "Foreground and background gradient are None. At least one gradient must be provided."
STRING msg_gradient_empty, "Foreground and background gradient are empty. At least one gradient must have at least one color."

    TSTATE
    .balign 8
scenes:         .skip 8
scene_pre:      .skip 8
frame_region:   .skip 8
frame_region_end: .skip 8
scene_count:    .skip 4             // records handed out (in chunks)
    .balign 8
scene_banks:    .skip 8 * SCENE_BANKS   // scene_alloc's cursors
scene_unbanked: .skip 1             // indices in creation order (vhstape)
    .balign 8
shape_last:     .skip 8             // the last shape found (initially shapes)
shape_count:    .skip 4
share_count:    .skip 4             // lists in share_table
share_hits:     .skip 4             // lookups that found one
share_off:      .skip 1             // lookups stopped
    .balign 8
share_table:    .skip 8
    .balign 64
shapes:         .skip SHAPE_LIMIT << SHAPE_SHIFT
vmemo:          .skip 32 << VMEMO_BITS  // visual_memo's cache
