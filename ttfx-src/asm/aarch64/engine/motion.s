// engine/motion.s - Waypoint, Segment, Path and Motion
// (src/engine/motion.rs; stepping from src/engine/ctx.rs).
//
// Paths belong to a character's path map (ch_paths, linked through PA_NEXT in
// insertion order) and are addressed by index into [paths]. Records never
// move; their segment and waypoint arrays can (they grow by copying), so
// stepping re-reads the array after every event emission, the same way Rust
// re-resolves the path after each reentrant action.
//
// The walk index (PA_INT_COUNT, PA_DONE, PA_CURSOR, PA_REACH, SG_PREFIX), the
// eased factor tables (PA_ETAB, ETAB_*) and the shared segment lists
// (PAF_SHARED, SEGSHARE_*) are defined in defs.inc.
//
// The x86 engine's batched steps and slot mirrors (motion_batch and its
// helpers, MV_* / MB_*) exist only at its TIER 3 and up; this port is the
// TIER 1 engine, where update calls motion_move for every mover, MV_RETIRE
// is empty and path_view reads the record.

// MO_PATH_REACH xrec: store PA_REACH after PA_INT_COUNT or PA_DONE changed.
// Clobbers x9, x3.
.macro MO_PATH_REACH rec
    ldrh    w9, [\rec, #PA_DONE]
    add     w9, w9, #1
    ldrh    w3, [\rec, #PA_INT_COUNT]
    cmp     w3, w9
    csel    w3, w9, w3, hi
    strh    w3, [\rec, #PA_REACH]
.endm

    .text

paths_init:
    str     x30, [sp, #-16]!
    mov     x0, #(PATH_LIMIT * PATH_SIZE)
    bl      reserve
    STX     x9, paths
    ldr     x30, [sp], #16
    ret

// path_find(w0=slot, w1=name) -> w9 = path index or NONE. Preserves x0, x1;
// clobbers x3, x16.
path_find:
    LDX     x9, ch_paths
    ldr     w9, [x9, w0, uxtw #2]
path_find.next:
    cmn     w9, #1                      // NONE
    b.eq    path_find.done
    PATH_PTR x3, x9
    ldr     w16, [x3, #PA_NAME]
    cmp     w16, w1
    b.eq    path_find.done
    ldr     w9, [x3, #PA_NEXT]
    b       path_find.next
path_find.done:
    ret

// path_new(w0=slot, d0=speed, w1=easing id or NONE, x2=layer or
//          NONE_I64, x3=hold time, w4=loop, w5=name or AUTO) -> w9.
// Motion.new_path: auto ids are the path count probing upward; a duplicate
// explicit id is an error (an effect bug, so fatal here).
path_new:
    stp     x19, x20, [sp, #-80]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    str     d0, [sp, #56]               // speed; [sp, #64] the index
    mov     w19, w0
    mov     w20, w1
    mov     x21, x2
    mov     x22, x3
    mov     w23, w4
    mov     w24, w5
    // Rust rejects speed <= 0.0 only: a NaN speed (uniform(inf, inf)) is
    // accepted, so an unordered compare must not take the error branch
    fcmp    d0, #0.0
    b.ls    path_new.bad_speed          // ordered <=
path_new.speed_ok:
    cmn     w24, #1                     // AUTO
    b.ne    path_new.named
    LDX     x9, ch_paths
    ldr     w9, [x9, x19, lsl #2]
    mov     w24, #0
path_new.count:
    cmn     w9, #1
    b.eq    path_new.probe
    add     w24, w24, #1
    PATH_PTR x3, x9
    ldr     w9, [x3, #PA_NEXT]
    b       path_new.count
path_new.probe:
    mov     w0, w19
    mov     w1, w24
    bl      path_find
    cmn     w9, #1
    b.eq    path_new.fresh
    add     w24, w24, #1
    b       path_new.probe
path_new.named:
    mov     w0, w19
    mov     w1, w24
    bl      path_find
    cmn     w9, #1
    b.ne    path_new.duplicate
path_new.fresh:
    bl      path_take_free              // particles: a recycled index, or NONE
    cmn     w9, #1
    b.ne    path_new.recycled
    LDW     w9, path_count
    mov     w6, #PATH_LIMIT
    cmp     w9, w6
    b.hs    path_new.full
    add     w6, w9, #1
    STW     w6, path_count
path_new.recycled:
    mov     w9, w9
    str     x9, [sp, #64]
    PATH_PTR x4, x9
    stp     xzr, xzr, [x4, #0]
    stp     xzr, xzr, [x4, #16]
    stp     xzr, xzr, [x4, #32]
    stp     xzr, xzr, [x4, #48]
    stp     xzr, xzr, [x4, #64]
    stp     xzr, xzr, [x4, #80]
    stp     xzr, xzr, [x4, #96]
    stp     xzr, xzr, [x4, #112]
    bl      path_take_restore           // particles: its empty arrays (x4 = record)
    ldr     x9, [sp, #64]
    PATH_PTR x4, x9
    str     w24, [x4, #PA_NAME]
    mov     w6, #-1
    str     w6, [x4, #PA_NEXT]
    ldr     d0, [sp, #56]
    str     d0, [x4, #PA_SPEED]
    str     w20, [x4, #PA_EASE]
    mov     w9, #0
    MOV64   x3, NONE_I64
    cmp     x21, x3
    b.eq    path_new.no_layer
    str     w21, [x4, #PA_LAYER]
    orr     w9, w9, #PAF_LAYER
path_new.no_layer:
    cbz     w23, path_new.flags
    orr     w9, w9, #PAF_LOOP
path_new.flags:
    str     w9, [x4, #PA_FLAGS]
    str     x22, [x4, #PA_HOLD]
    str     x22, [x4, #PA_HOLD_LEFT]
    // append to the character's map
    LDX     x3, ch_paths
    add     x3, x3, x19, lsl #2
path_new.tail:
    ldr     w2, [x3]
    cmn     w2, #1
    b.eq    path_new.link
    PATH_PTR x3, x2
    add     x3, x3, #PA_NEXT
    b       path_new.tail
path_new.link:
    ldr     x9, [sp, #64]
    str     w9, [x3]
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #80
    ret
path_new.bad_speed:
    ADRG    x0, msg_path_speed
    mov     w1, #msg_path_speed_len
    b       fatal
path_new.duplicate:
    ADRG    x0, msg_duplicate_path
    mov     w1, #msg_duplicate_path_len
    b       fatal
path_new.full:
    ADRG    x0, msg_paths_full
    mov     w1, #msg_paths_full_len
    b       fatal

// path_new_waypoint(w0=path, x1=coord, x2=bezier controls or 0, w3=control
//                   count, w4=name or AUTO) -> x9 = pointer to the waypoint.
// Path.new_waypoint + _add_waypoint_to_path: from the second waypoint on, a
// segment from the previous one, the running total and max_steps.
path_new_waypoint:
    stp     x19, x20, [sp, #-80]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56] distance, [sp, #64] result
    mov     w19, w0
    mov     x21, x1
    mov     x22, x2
    mov     w23, w3
    mov     w24, w4
    PATH_PTR x20, x19
    bl      path_unshare
    cmn     w24, #1                     // AUTO
    b.ne    path_new_waypoint.named
    ldr     w24, [x20, #PA_WP_COUNT]
path_new_waypoint.probe:
    mov     w1, w24
    bl      waypoint_named
    cbz     x9, path_new_waypoint.have_name
    add     w24, w24, #1
    b       path_new_waypoint.probe
path_new_waypoint.named:
    mov     w1, w24
    bl      waypoint_named
    cbnz    x9, path_new_waypoint.duplicate
path_new_waypoint.have_name:
    // a private copy of the controls (the caller's array may not live on)
    cbz     w23, path_new_waypoint.no_controls
    cbz     x22, path_new_waypoint.no_controls
    lsl     x0, x23, #3
    bl      bez_alloc                   // particles: alloc, or a freed block
    mov     x0, x9
    mov     x1, x22
    mov     w3, w23
    REP_MOVSQ
    mov     x22, x9
    b       path_new_waypoint.grow
path_new_waypoint.no_controls:
    mov     x22, #0
    mov     w23, #0
path_new_waypoint.grow:
    ldr     w3, [x20, #PA_WP_COUNT]
    ldr     w6, [x20, #PA_WP_CAP]
    cmp     w3, w6
    b.lo    path_new_waypoint.room
    add     x0, x20, #PA_WPS
    mov     w1, #WAYPOINT_SIZE
    add     x2, x20, #PA_WP_COUNT
    bl      grow_array
path_new_waypoint.room:
    ldr     w3, [x20, #PA_WP_COUNT]
    ldr     x6, [x20, #PA_WPS]
    add     x3, x6, x3, lsl #5
    str     x21, [x3, #WP_COORD]
    str     w24, [x3, #WP_NAME]
    str     w23, [x3, #WP_BEZ_COUNT]
    str     x22, [x3, #WP_BEZ]
    ldr     w6, [x20, #PA_WP_COUNT]
    add     w6, w6, #1
    str     w6, [x20, #PA_WP_COUNT]
    str     x3, [sp, #64]               // the new waypoint
    cmp     w6, #2
    b.lo    path_new_waypoint.done
    // distance from the previous waypoint
    ldr     x0, [x3, #(WP_COORD - WAYPOINT_SIZE)]
    cbz     w23, path_new_waypoint.line
    mov     x1, x22
    mov     w2, w23
    mov     x3, x21
    bl      find_length_of_bezier_curve
    b       path_new_waypoint.distance
path_new_waypoint.line:
    mov     x1, x21
    mov     w2, #1
    bl      find_length_of_line
path_new_waypoint.distance:
    str     d0, [sp, #56]
    PATH_PTR x20, x19
    ldr     d1, [x20, #PA_TOTAL]
    fadd    d1, d1, d0
    str     d1, [x20, #PA_TOTAL]
    // the segment: previous and new waypoint by value
    ldr     w3, [x20, #PA_SEG_COUNT]
    ldr     w6, [x20, #PA_SEG_CAP]
    cmp     w3, w6
    b.lo    path_new_waypoint.seg_room
    add     x0, x20, #PA_SEGS
    mov     w1, #SEGMENT_SIZE
    add     x2, x20, #PA_SEG_COUNT
    bl      grow_array
path_new_waypoint.seg_room:
    ldr     w3, [x20, #PA_SEG_COUNT]
    mov     w6, #SEGMENT_SIZE
    ldr     x7, [x20, #PA_SEGS]
    umaddl  x3, w3, w6, x7
    ldr     w9, [x20, #PA_WP_COUNT]
    sub     w9, w9, #2
    ldr     x7, [x20, #PA_WPS]
    add     x9, x7, x9, lsl #5
    ldp     x6, x7, [x9]
    stp     x6, x7, [x3, #SG_START]
    ldp     x6, x7, [x9, #16]
    stp     x6, x7, [x3, #(SG_START + 16)]
    // the new waypoint from registers
    str     x21, [x3, #(SG_END + WP_COORD)]
    str     w24, [x3, #(SG_END + WP_NAME)]
    str     w23, [x3, #(SG_END + WP_BEZ_COUNT)]
    str     x22, [x3, #(SG_END + WP_BEZ)]
    str     xzr, [x3, #(SG_END + 24)]
    ldr     d0, [sp, #56]
    str     d0, [x3, #SG_DISTANCE]
    strh    wzr, [x3, #SG_ENTERED]
    ldr     w6, [x20, #PA_SEG_COUNT]
    add     w6, w6, #1
    str     w6, [x20, #PA_SEG_COUNT]
    bl      path_extend_index
    // max_steps = round(total_distance / speed)
    ldr     d0, [x20, #PA_TOTAL]
    ldr     d1, [x20, #PA_SPEED]
    fdiv    d0, d0, d1
    ROUND_HALF_EVEN
    str     x9, [x20, #PA_MAX]
    str     wzr, [x20, #PA_ETAB]
    ldr     w9, [x20, #PA_WP_COUNT]
    sub     w9, w9, #1
    ldr     x6, [x20, #PA_WPS]
    add     x9, x6, x9, lsl #5
    str     x9, [sp, #64]
path_new_waypoint.done:
    ldr     x9, [sp, #64]
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #80
    ret
path_new_waypoint.duplicate:
    ADRG    x0, msg_duplicate_waypoint
    mov     w1, #msg_duplicate_waypoint_len
    b       fatal

// waypoint_named(x20=path record, w1=name) -> x9 = the waypoint or 0.
// Clobbers x3, x16.
waypoint_named:
    ldr     w3, [x20, #PA_WP_COUNT]
    ldr     x9, [x20, #PA_WPS]
waypoint_named.next:
    cbz     w3, waypoint_named.none
    ldr     w16, [x9, #WP_NAME]
    cmp     w16, w1
    b.eq    waypoint_named.done
    add     x9, x9, #WAYPOINT_SIZE
    sub     w3, w3, #1
    b       waypoint_named.next
waypoint_named.none:
    mov     x9, #0
waypoint_named.done:
    ret

// grow_array(x0=pointer field, w1=element size, x2=count field (u32, the
// capacity follows it)): double the capacity (at least 4) by copying.
grow_array:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     x19, x0
    mov     w21, w1
    mov     x22, x2
    ldr     w9, [x22, #4]
    add     w9, w9, w9
    mov     w3, #4
    cmp     w9, w3
    csel    w9, w3, w9, lo
    str     w9, [x22, #4]
    mul     x0, x9, x21
    bl      alloc
    ldr     x1, [x19]
    mov     x0, x9
    ldr     w3, [x22]
    mul     w3, w3, w21
    REP_MOVSB
    str     x9, [x19]
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// path_extend_index(x20=path record): extend the run of whole-number
// segment distances (PA_INT_COUNT) and its prefix sums (SG_PREFIX) over the
// segments after it. Distances are at most 2^20 and sums below 2^30, so
// every sum is exact in an f64 and in a u32.
// Clobbers x9, x3, x2, x4, x5, x6, d0, d1.
path_extend_index:
    ldrh    w3, [x20, #PA_INT_COUNT]
    ldr     x4, [x20, #PA_SEGS]
    mov     w2, #0                      // the sum so far
    cbz     w3, path_extend_index.next
    mov     w9, #SEGMENT_SIZE
    umaddl  x9, w3, w9, x4
    ldur    w2, [x9, #(SG_PREFIX - SEGMENT_SIZE)]
path_extend_index.next:
    ldr     w6, [x20, #PA_SEG_COUNT]
    cmp     w3, w6
    b.hs    path_extend_index.done
    mov     w6, #0xffff
    cmp     w3, w6
    b.hs    path_extend_index.done
    mov     w9, #SEGMENT_SIZE
    umaddl  x9, w3, w9, x4
    ldr     d0, [x9, #SG_DISTANCE]
    // fcvtzs saturates and maps NaN to 0: the compare below rejects NaN
    // (unordered is "not equal"), as x86's out-of-range test did
    fcvtzs  x5, d0
    cmp     x5, #(1 << 20)
    b.hi    path_extend_index.done      // unsigned: also negatives
    scvtf   d1, x5
    fcmp    d1, d0
    b.ne    path_extend_index.done
    add     w2, w2, w5
    mov     w6, #(1 << 30)
    cmp     w2, w6
    b.hs    path_extend_index.done
    str     w2, [x9, #SG_PREFIX]
    add     w3, w3, #1
    strh    w3, [x20, #PA_INT_COUNT]
    b       path_extend_index.next
path_extend_index.done:
    MO_PATH_REACH x20
    ret

// ------------------------------------------------------------ shared segments
//
// Effects sometimes give several characters the same path (binarypath's
// eight bits per input character), and each walks its own copy of the
// segments. When a path is activated and its owner has no segment events,
// its segments are looked up by content (everything but the event flags)
// in segshare_table, and the path then walks the table's copy, which is
// never written: its flags read as fired. Without events nothing can reset
// the path mid-walk, so the path's own flags follow from its record: the
// segments before PA_DONE fired, and segment PA_DONE entered once any step
// was taken (every walk that moves PA_DONE ends in the new PA_DONE, and the
// first one enters where it ends). Anything that would write the segments
// or could observe the flags first gives the path a private copy with the
// flags spelled out (path_unshare): new waypoints, activation, path_reset
// and a segment event registered for the owner (event_register).

// path_unshare(x20=path record): give a shared path its own segments.
// Clobbers x9, x3, x2, x1, x0, x6 (and path_unpark_segs's).
path_unshare:
    ldr     w9, [x20, #PA_FLAGS]
    tbnz    w9, #3, path_unshare.copy   // PAF_SHARED
    ret
path_unshare.copy:
    str     x30, [sp, #-16]!
    ldr     w3, [x20, #PA_SEG_COUNT]
    mov     w9, #4
    cmp     w3, w9
    csel    w3, w9, w3, lo
    str     w3, [x20, #PA_SEG_CAP]
    mov     w9, #SEGMENT_SIZE
    mul     w0, w3, w9
    bl      path_unpark_segs            // particles: alloc, or its parked list
    ldr     x1, [x20, #PA_SEGS]
    str     x9, [x20, #PA_SEGS]
    mov     x0, x9
    ldr     w3, [x20, #PA_SEG_COUNT]
    mov     w6, #SEGMENT_SIZE
    mul     w3, w3, w6
    REP_MOVSB
    // the flags: fired before PA_DONE, entered at PA_DONE after a step
    ldr     x0, [x20, #PA_SEGS]
    ldrh    w2, [x20, #PA_DONE]
    mov     w3, #0
path_unshare.flag:
    ldr     w6, [x20, #PA_SEG_COUNT]
    cmp     w3, w6
    b.hs    path_unshare.flagged
    mov     w9, #0x0101
    cmp     w3, w2
    b.lo    path_unshare.store
    mov     w9, #0
    b.ne    path_unshare.store
    ldr     x6, [x20, #PA_STEP]
    cbz     x6, path_unshare.store
    mov     w9, #1
path_unshare.store:
    strh    w9, [x0, #SG_ENTERED]
    add     x0, x0, #SEGMENT_SIZE
    add     w3, w3, #1
    b       path_unshare.flag
path_unshare.flagged:
    ldr     w9, [x20, #PA_FLAGS]
    and     w9, w9, #(0xffffffff ^ PAF_SHARED)
    str     w9, [x20, #PA_FLAGS]
    ldr     x30, [sp], #16
    ret

// path_unshare_all(w0=slot): path_unshare for every path of the character
// (it is about to observe segment events). Preserves x0, x1; clobbers
// x9, x3, x2.
path_unshare_all:
    stp     x19, x20, [sp, #-48]!
    stp     x0, x1, [sp, #16]
    str     x30, [sp, #32]
    LDX     x9, ch_paths
    ldr     w19, [x9, w0, uxtw #2]
path_unshare_all.next:
    cmn     w19, #1
    b.eq    path_unshare_all.done
    PATH_PTR x20, x19
    bl      path_unshare
    ldr     w19, [x20, #PA_NEXT]
    b       path_unshare_all.next
path_unshare_all.done:
    ldp     x0, x1, [sp, #16]
    ldr     x30, [sp, #32]
    ldp     x19, x20, [sp], #48
    ret

// SEG_HASH_STEP acc, segment register, offset: fold one qword (the second
// accumulator is x2). Clobbers x6.
.macro MO_SEG_HASH_STEP acc, seg, off
    ldr     x6, [\seg, #\off]
    add     \acc, \acc, x6
    eor     x2, x2, \acc
    ror     x2, x2, #57                 // rol 7
.endm

// path_seg_share(x20=path record, w19=owner slot): after activation, walk
// the shared copy of the segments when there is one (or make one).
// Clobbers x0-x11 (x9 included) and what alloc, reserve and path_park_segs
// clobber.
path_seg_share:
    LDB     w9, segshare_off
    cbnz    w9, path_seg_share.ret
    LDX     x9, ch_subs
    ldrb    w9, [x9, x19]
    tst     w9, #((1 << EV_SEGMENT_ENTERED) | (1 << EV_SEGMENT_EXITED))
    b.ne    path_seg_share.ret
    ldr     w3, [x20, #PA_SEG_COUNT]
    cbz     w3, path_seg_share.ret
    mov     w6, #0xfff0
    cmp     w3, w6
    b.hs    path_seg_share.ret
    sub     sp, sp, #32                 // [sp, #8] entry, [sp, #16] hash, [sp, #24] list
    str     x30, [sp]
    LDX     x5, segshare_table
    cbnz    x5, path_seg_share.hash
    mov     x0, #(SEGSHARE_SIZE * 16)
    bl      reserve
    STX     x9, segshare_table
    mov     x5, x9
    ldr     w3, [x20, #PA_SEG_COUNT]
path_seg_share.hash:
    // a running sum of each segment's end and distance and the xor of its
    // rotated prefixes (the compare checks the rest)
    ldr     x10, [x20, #PA_SEGS]
    mov     w9, w3                      // seeded with the count
    mov     x2, #0
path_seg_share.fold:
    MO_SEG_HASH_STEP x9, x10, (SG_END + WP_COORD)
    MO_SEG_HASH_STEP x9, x10, SG_DISTANCE
    add     x10, x10, #SEGMENT_SIZE
    subs    w3, w3, #1
    b.ne    path_seg_share.fold
    MOV64   x11, 0x9E3779B97F4A7C15
    mul     x9, x9, x11
    eor     x9, x9, x2
    mul     x9, x9, x11
    mov     x10, x9                     // the hash
    lsr     x9, x9, #(64 - SEGSHARE_BITS)
path_seg_share.probe:
    add     x2, x5, x9, lsl #4          // the entry
    ldr     x1, [x2]
    cbz     x1, path_seg_share.insert
    ldr     w6, [x2, #12]
    cmp     w6, w10
    b.ne    path_seg_share.next
    ldr     w3, [x20, #PA_SEG_COUNT]
    ldr     w6, [x2, #8]
    cmp     w6, w3
    b.ne    path_seg_share.next
    // compare everything but the flags
    ldr     x0, [x20, #PA_SEGS]
path_seg_share.compare:
    .irp so, 0, 8, 16, 24, 32, 40, 48, 56, 64
    ldr     x11, [x0, #\so]
    ldr     x6, [x1, #\so]
    cmp     x11, x6
    b.ne    path_seg_share.next
    .endr
    ldr     w11, [x0, #SG_PREFIX]
    ldr     w6, [x1, #SG_PREFIX]
    cmp     w11, w6
    b.ne    path_seg_share.next
    add     x0, x0, #SEGMENT_SIZE
    add     x1, x1, #SEGMENT_SIZE
    subs    w3, w3, #1
    b.ne    path_seg_share.compare
    LDW     w6, segshare_hits
    add     w6, w6, #1
    STW     w6, segshare_hits
    ldr     x1, [x2]
    b       path_seg_share.use
path_seg_share.next:
    add     w9, w9, #1
    and     w9, w9, #(SEGSHARE_SIZE - 1)
    b       path_seg_share.probe
path_seg_share.insert:
    // a new list: the table keeps its own copy, with every flag fired
    LDW     w3, segshare_count
    cmp     w3, #(SEGSHARE_SIZE * 3 / 4)
    b.hs    path_seg_share.off
    cmp     w3, #SEGSHARE_PROBATION
    b.lo    path_seg_share.keep
    LDW     w11, segshare_hits
    lsl     w11, w11, #2
    cmp     w11, w3
    b.lo    path_seg_share.off          // under one hit in four new lists
path_seg_share.keep:
    add     w3, w3, #1
    STW     w3, segshare_count
    mov     x4, x2
    str     x4, [sp, #8]
    str     x10, [sp, #16]
    ldr     w6, [x20, #PA_SEG_COUNT]
    mov     w7, #SEGMENT_SIZE
    mul     w0, w6, w7
    bl      alloc
    ldr     x4, [sp, #8]
    ldr     x10, [sp, #16]
    str     x9, [x4]
    ldr     w3, [x20, #PA_SEG_COUNT]
    str     w3, [x4, #8]
    str     w10, [x4, #12]
    mov     x0, x9
    ldr     x1, [x20, #PA_SEGS]
    mov     w6, #SEGMENT_SIZE
    mul     w3, w3, w6
    REP_MOVSB
    ldr     x1, [x4]
    mov     x0, x1
    ldr     w3, [x20, #PA_SEG_COUNT]
    mov     w6, #0x0101
path_seg_share.fire:
    strh    w6, [x0, #SG_ENTERED]
    add     x0, x0, #SEGMENT_SIZE
    subs    w3, w3, #1
    b.ne    path_seg_share.fire
path_seg_share.use:
    // the path walks the shared list; activation left every flag clear
    str     x1, [sp, #24]
    bl      path_park_segs              // particles: its own list, for reuse
    ldr     x1, [sp, #24]
    str     x1, [x20, #PA_SEGS]
    ldr     w3, [x20, #PA_SEG_COUNT]
    str     w3, [x20, #PA_SEG_CAP]
    ldr     w6, [x20, #PA_FLAGS]
    orr     w6, w6, #PAF_SHARED
    str     w6, [x20, #PA_FLAGS]
    ldr     x30, [sp]
    add     sp, sp, #32
path_seg_share.ret:
    ret
path_seg_share.off:
    mov     w6, #1
    STB     w6, segshare_off
    ldr     x30, [sp]
    add     sp, sp, #32
    ret

// ------------------------------------------------------------ activation

// path_activate(w0=slot, w1=path): Motion.activate_path - a synthetic origin
// segment from the current coordinate to the first waypoint replaces the
// previous one (rebasing the total distance), playback restarts, the path's
// layer applies, and PATH_ACTIVATED fires.
path_activate:
    stp     x19, x20, [sp, #-96]!
    stp     x21, x22, [sp, #16]
    str     x30, [sp, #32]              // [sp, #48] first waypoint, [sp, #80] distance
    mov     w19, w0
    mov     w21, w1
    bl      doze_wake                   // update: it must tick again
    PATH_PTR x20, x21
    ldr     w9, [x20, #PA_WP_COUNT]
    cbz     w9, path_activate.empty
    bl      path_unshare
    mov     w0, w19
    bl      char_coord
    mov     x22, x9                     // current coordinate
    // distance to the first waypoint
    ldr     x9, [x20, #PA_WPS]
    ldp     x6, x7, [x9]
    stp     x6, x7, [sp, #48]           // first waypoint (segment end)
    ldp     x6, x7, [x9, #16]
    stp     x6, x7, [sp, #64]
    ldr     w2, [x9, #WP_BEZ_COUNT]
    cbz     w2, path_activate.line
    mov     x0, x22
    ldr     x1, [x9, #WP_BEZ]
    ldr     x3, [x9, #WP_COORD]
    bl      find_length_of_bezier_curve
    b       path_activate.distance
path_activate.line:
    mov     x0, x22
    ldr     x1, [x9, #WP_COORD]
    mov     w2, #1
    bl      find_length_of_line
path_activate.distance:
    str     d0, [sp, #80]
    LDX     x9, ch_path
    str     w21, [x9, x19, lsl #2]
    PATH_PTR x20, x21
    ldr     d1, [x20, #PA_TOTAL]
    fadd    d1, d1, d0
    ldr     w6, [x20, #PA_FLAGS]
    tbz     w6, #2, path_activate.insert    // PAF_ORIGIN
    ldr     d2, [x20, #PA_ORIGIN_DIST]
    fsub    d1, d1, d2
    str     d1, [x20, #PA_TOTAL]
    ldr     x0, [x20, #PA_SEGS]         // replace segments[0]
    b       path_activate.write_origin
path_activate.insert:
    str     d1, [x20, #PA_TOTAL]
    ldr     w3, [x20, #PA_SEG_COUNT]
    ldr     w6, [x20, #PA_SEG_CAP]
    cmp     w3, w6
    b.lo    path_activate.shift
    add     x0, x20, #PA_SEGS
    mov     w1, #SEGMENT_SIZE
    add     x2, x20, #PA_SEG_COUNT
    bl      grow_array
path_activate.shift:
    // move the segments up by one (from the end), then write segments[0]
    ldr     w3, [x20, #PA_SEG_COUNT]
    mov     w6, #SEGMENT_SIZE
    umull   x3, w3, w6
    ldr     x1, [x20, #PA_SEGS]
path_activate.shift_chunk:
    subs    x3, x3, #16                 // SEGMENT_SIZE is a multiple of 16
    b.lo    path_activate.shifted
    add     x6, x1, x3
    ldp     x7, x8, [x6]
    stp     x7, x8, [x6, #SEGMENT_SIZE]
    b       path_activate.shift_chunk
path_activate.shifted:
    ldr     w6, [x20, #PA_SEG_COUNT]
    add     w6, w6, #1
    str     w6, [x20, #PA_SEG_COUNT]
    ldr     x0, [x20, #PA_SEGS]
path_activate.write_origin:
    str     x22, [x0, #(SG_START + WP_COORD)]
    mov     w6, #ORIGIN_NAME
    str     w6, [x0, #(SG_START + WP_NAME)]
    str     wzr, [x0, #(SG_START + WP_BEZ_COUNT)]
    str     xzr, [x0, #(SG_START + WP_BEZ)]
    str     xzr, [x0, #(SG_START + 24)]
    ldp     x6, x7, [sp, #48]
    stp     x6, x7, [x0, #SG_END]
    ldp     x6, x7, [sp, #64]
    stp     x6, x7, [x0, #(SG_END + 16)]
    ldr     d0, [sp, #80]
    str     d0, [x0, #SG_DISTANCE]
    str     d0, [x20, #PA_ORIGIN_DIST]
    ldr     w6, [x20, #PA_FLAGS]
    orr     w6, w6, #PAF_ORIGIN
    str     w6, [x20, #PA_FLAGS]
    str     xzr, [x20, #PA_STEP]
    ldr     x9, [x20, #PA_HOLD]
    str     x9, [x20, #PA_HOLD_LEFT]
    ldr     d0, [x20, #PA_TOTAL]
    ldr     d1, [x20, #PA_SPEED]
    fdiv    d0, d0, d1
    ROUND_HALF_EVEN
    str     x9, [x20, #PA_MAX]
    str     wzr, [x20, #PA_ETAB]
    // every segment's events can fire again; the origin changed the sums
    ldr     w3, [x20, #PA_SEG_COUNT]
    ldr     x9, [x20, #PA_SEGS]
path_activate.clear:
    cbz     w3, path_activate.index
    strh    wzr, [x9, #SG_ENTERED]
    add     x9, x9, #SEGMENT_SIZE
    sub     w3, w3, #1
    b       path_activate.clear
path_activate.index:
    str     wzr, [x20, #PA_INT_COUNT]   // and PA_DONE
    strh    wzr, [x20, #PA_CURSOR]
    bl      path_extend_index
    bl      path_seg_share
path_activate.layer:
    ldr     w6, [x20, #PA_FLAGS]
    tbz     w6, #0, path_activate.event // PAF_LAYER
    mov     w0, w19
    ldrsw   x1, [x20, #PA_LAYER]
    bl      set_layer
path_activate.event:
    LDX     x9, ch_subs
    ldrb    w9, [x9, x19]
    tbz     w9, #EV_PATH_ACTIVATED, path_activate.done
    mov     w0, w19
    mov     w1, #EV_PATH_ACTIVATED
    mov     w2, #CALLER_PATH
    ldr     w3, [x20, #PA_NAME]
    bl      handle_event
path_activate.done:
    ldr     x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #96
    ret
path_activate.empty:
    ADRG    x0, msg_empty_path
    mov     w1, #msg_empty_path_len
    b       fatal

// path_activate_name(w0=slot, w1=name)
path_activate_name:
    str     x30, [sp, #-16]!
    bl      path_find                   // preserves x0, x1
    ldr     x30, [sp], #16
    cmn     w9, #1
    b.eq    path_activate_name.missing
    mov     w1, w9
    b       path_activate
path_activate_name.missing:
    ADRG    x0, msg_path_missing
    mov     w1, #msg_path_missing_len
    b       fatal

// path_deactivate(w0=slot, w1=name or NONE): Motion.deactivate_path - any
// active path, or only the named one. Clobbers x9, x3, x2, x6, x16.
path_deactivate:
    LDX     x9, ch_path
    ldr     w3, [x9, w0, uxtw #2]
    cmn     w3, #1
    b.eq    path_deactivate.done
    cmn     w1, #1
    b.eq    path_deactivate.clear
    PATH_PTR x2, x3
    ldr     w6, [x2, #PA_NAME]
    cmp     w6, w1
    b.ne    path_deactivate.done
path_deactivate.clear:
    mov     w6, #-1
    str     w6, [x9, w0, uxtw #2]
    MARK_CANDIDATE
path_deactivate.done:
    ret

// chain_paths(w0=slot, x1=path names (u32), x2=count, w3=loop):
// Motion.chain_paths - each path's completion activates the next.
chain_paths:
    cmp     x2, #2
    b.lo    chain_paths.done
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    mov     w19, w0
    mov     x21, x1
    mov     x22, x2
    mov     w23, w3
    mov     x24, #1
chain_paths.link:
    cmp     x24, x22
    b.hs    chain_paths.loop
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    sub     x9, x24, #1
    ldr     w3, [x21, x9, lsl #2]
    mov     w4, #ACT_ACTIVATE_PATH
    ldr     w5, [x21, x24, lsl #2]
    mov     x6, #0                      // arg1
    bl      event_register
    add     x24, x24, #1
    b       chain_paths.link
chain_paths.loop:
    cbz     w23, chain_paths.out
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    sub     x9, x22, #1
    ldr     w3, [x21, x9, lsl #2]
    mov     w4, #ACT_ACTIVATE_PATH
    ldr     w5, [x21]
    mov     x6, #0                      // arg1
    bl      event_register
chain_paths.out:
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
chain_paths.done:
    ret

// ------------------------------------------------------------ stepping

// path_ease_table(x20=path record) -> w9 = PA_ETAB: the path's eased
// factor table, ease(step / max_steps) at [etab_base + w9 * 8 + step * 8]
// for step 1..=max_steps. Tables are shared by every path with the same
// easing and max_steps (found through a direct-mapped map; a collision just
// makes a fresh table) and filled when made; an entry of 0 means not
// stored (ease gave +0.0). ETAB_NONE: too many steps, or the region is
// full - use ease(). A new table is filled at once. Clobbers C.
path_ease_table:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    ldr     x9, [x20, #PA_MAX]
    cmp     x9, #ETAB_MAX_STEPS
    b.hi    path_ease_table.none
    LDX     x0, etab_map
    cbnz    x0, path_ease_table.lookup
    mov     x0, #ETAB_REGION
    bl      reserve
    STX     x9, etab_base
    mov     x9, #1                      // offset 0 means "no table yet"
    STX     x9, etab_used
    mov     x0, #(ETAB_MAP_SIZE * 16)
    bl      alloc
    STX     x9, etab_map
    mov     x0, x9
    ldr     x9, [x20, #PA_MAX]
path_ease_table.lookup:
    ldr     w1, [x20, #PA_EASE]
    add     w1, w1, #1
    orr     x1, x1, x9, lsl #32         // key: max_steps, easing + 1 (never 0)
    MOV64   x3, 0x9E3779B97F4A7C15
    mul     x9, x1, x3
    lsr     x9, x9, #(64 - ETAB_MAP_BITS)
    add     x0, x0, x9, lsl #4
    ldr     x3, [x0]
    cmp     x3, x1
    b.ne    path_ease_table.create
    ldr     w9, [x0, #8]
    str     w9, [x20, #PA_ETAB]
    b       path_ease_table.ret
path_ease_table.create:
    ldr     x3, [x20, #PA_MAX]
    add     x3, x3, #1                  // entries 0..=max_steps
    LDX     x9, etab_used
    add     x2, x9, x3
    mov     x6, #(ETAB_REGION / 8)
    cmp     x2, x6
    b.hi    path_ease_table.none
    STX     x2, etab_used
    str     x1, [x0]
    str     w9, [x0, #8]
    str     w9, [x20, #PA_ETAB]
    // fill it now: the paths that make a table nearly always walk it to
    // the end
    mov     w19, w9
    ldr     x21, [x20, #PA_MAX]
    mov     x22, #1
path_ease_table.fill:
    cmp     x22, x21
    b.hi    path_ease_table.filled
    scvtf   d0, x22
    scvtf   d1, x21
    fdiv    d0, d0, d1                  // path_step's ratio
    ldr     w0, [x20, #PA_EASE]
    bl      ease
    LDX     x9, etab_base
    add     x9, x9, x19, lsl #3
    str     d0, [x9, x22, lsl #3]
    add     x22, x22, #1
    b       path_ease_table.fill
path_ease_table.filled:
    mov     w9, w19
    b       path_ease_table.ret
path_ease_table.none:
    mov     w9, #ETAB_NONE
    str     w9, [x20, #PA_ETAB]
path_ease_table.ret:
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// path_step(w0=slot, w1=path) -> x9 = the next coordinate. Path.step: the
// index-based segment walk with its reentrant segment events.
//
// The walk subtracts each passed segment's distance from the distance to
// travel, in order. While those distances are whole numbers (and the
// distance is below 2^52) every subtraction is exact, so the running value
// is the distance minus a prefix sum, bit for bit, and each `d <= distance`
// test is `d <= prefix sum`. Over the leading run of such segments whose
// events have all fired (PA_DONE), the walk is a cursor search over the
// prefix sums (SG_PREFIX), which steps along with the character. The search
// also covers the segment just after that run (PA_REACH): it may hold the
// destination too.
//
// When the segment the walk would test next holds the destination and was
// entered already - nearly every step - no event can fire, and the step
// finishes without saving any register; only the rest of the walk (.slow)
// keeps its state in callee-saved ones.
path_step:
    PATH_PTR x4, x1                     // records never move
    ldr     x9, [x4, #PA_MAX]
    cbz     x9, path_step.at_end
    ldr     x3, [x4, #PA_STEP]
    cmp     x3, x9
    b.ge    path_step.at_end
    ldr     d1, [x4, #PA_TOTAL]
    fcmp    d1, #0.0
    b.eq    path_step.at_end            // total_distance == 0.0 (ordered)
path_step.step:
    add     x9, x3, #1
    str     x9, [x4, #PA_STEP]
    ldr     w3, [x4, #PA_EASE]
    cmn     w3, #1
    b.eq    path_step.ratio_only
    // eased: the factor for (easing, max_steps, step) from its table
    ldr     w3, [x4, #PA_ETAB]
    cbz     w3, path_step.find_table
path_step.have_table:
    cmn     w3, #1                      // ETAB_NONE
    b.eq    path_step.ratio_only
    LDX     x2, etab_base
    add     x2, x2, x3, lsl #3
    ldr     d0, [x2, x9, lsl #3]
    fmov    x3, d0
    cbnz    x3, path_step.factor        // 0 = not filled (or +0.0: recomputed)
    scvtf   d0, x9
    ldr     x3, [x4, #PA_MAX]
    scvtf   d1, x3
    fdiv    d0, d0, d1                  // ratio
    stp     x0, x4, [sp, #-32]!
    str     x30, [sp, #16]
    ldr     w0, [x4, #PA_EASE]
    bl      ease
    ldr     x30, [sp, #16]
    ldp     x0, x4, [sp], #32
    ldr     w3, [x4, #PA_ETAB]
    ldr     x9, [x4, #PA_STEP]
    LDX     x2, etab_base
    add     x2, x2, x3, lsl #3
    str     d0, [x2, x9, lsl #3]
    b       path_step.factor
path_step.find_table:
    stp     x0, x4, [sp, #-32]!
    stp     x20, x30, [sp, #16]
    mov     x20, x4
    bl      path_ease_table
    ldp     x20, x30, [sp, #16]
    ldp     x0, x4, [sp], #32
    mov     w3, w9
    ldr     x9, [x4, #PA_STEP]
    b       path_step.have_table
path_step.ratio_only:
    scvtf   d0, x9
    ldr     x3, [x4, #PA_MAX]
    scvtf   d1, x3
    fdiv    d0, d0, d1                  // ratio
    ldr     w9, [x4, #PA_EASE]
    cmn     w9, #1
    b.eq    path_step.factor
    stp     x0, x4, [sp, #-32]!
    str     x30, [sp, #16]
    mov     w0, w9
    bl      ease
    ldr     x30, [sp, #16]
    ldp     x0, x4, [sp], #32
path_step.factor:
    ldr     d1, [x4, #PA_TOTAL]
    fmul    d0, d0, d1
    str     d0, [x4, #PA_LAST]          // distance_to_travel
    // the exact prefix: the walk skips the first L = min(PA_INT_COUNT,
    // PA_DONE) segments, and the one after them may hold the destination
    // too (L2 = min(PA_INT_COUNT, PA_DONE + 1))
    mov     w3, #0
    ldrh    w2, [x4, #PA_REACH]         // L2
    cbz     w2, path_step.try
    LDD     d1, path_two_p52
    fcmp    d1, d0
    b.le    path_step.try               // too large, or NaN
    ldr     x5, [x4, #PA_SEGS]
    ldrh    w3, [x4, #PA_CURSOR]
    cmp     w3, w2
    csel    w3, w2, w3, hi
path_step.back:
    // the first segment c with prefix(c + 1) >= d, else L2
    cbz     w3, path_step.forward
    mov     w9, #SEGMENT_SIZE
    umaddl  x6, w3, w9, x5
    ldur    w6, [x6, #(SG_PREFIX - SEGMENT_SIZE)]
    scvtf   d1, w6
    fcmp    d1, d0
    b.lt    path_step.forward
    sub     w3, w3, #1
    b       path_step.back
path_step.forward:
    cmp     w3, w2
    b.hs    path_step.past
    mov     w9, #SEGMENT_SIZE
    umaddl  x6, w3, w9, x5
    ldr     w6, [x6, #SG_PREFIX]
    scvtf   d1, w6
    fcmp    d1, d0
    b.ge    path_step.found
    add     w3, w3, #1
    b       path_step.forward
path_step.found:
    // segments[c] holds the destination
    strh    w3, [x4, #PA_CURSOR]
    mov     w9, #SEGMENT_SIZE
    umull   x9, w3, w9
    cbz     w3, path_step.entered
    add     x6, x5, x9
    ldur    w6, [x6, #(SG_PREFIX - SEGMENT_SIZE)]
    scvtf   d1, w6
    fsub    d0, d0, d1
    b       path_step.entered
path_step.past:
    // the walk goes on from segment L
    ldrh    w3, [x4, #PA_DONE]
    cmp     w3, w2
    csel    w3, w2, w3, hi
    strh    w3, [x4, #PA_CURSOR]
    cbz     w3, path_step.try
    mov     w9, #SEGMENT_SIZE
    umaddl  x6, w3, w9, x5
    ldur    w6, [x6, #(SG_PREFIX - SEGMENT_SIZE)]
    scvtf   d1, w6
    fsub    d0, d0, d1
path_step.try:
    // the walk's test of segment c with d0 left to travel: when it holds
    // the destination and was entered already, no event fires, and no
    // callee-saved register is needed
    ldr     w6, [x4, #PA_SEG_COUNT]
    cmp     w3, w6
    b.hs    path_step.slow
    mov     w9, #SEGMENT_SIZE
    umull   x9, w3, w9
    ldr     x5, [x4, #PA_SEGS]
    add     x6, x5, x9
    ldr     d1, [x6, #SG_DISTANCE]
    fcmp    d0, d1
    b.gt    path_step.slow
path_step.entered:
    add     x6, x5, x9
    ldrb    w6, [x6, #SG_ENTERED]
    cbz     w6, path_step.slow          // its enter event is due
path_step.finish:
    // no event, so no callee-saved register is needed
    add     x5, x5, x9
    ldr     d1, [x5, #SG_DISTANCE]
    fcmp    d1, #0.0
    b.ne    path_step.fast_ratio        // not equal, or unordered
    fmov    d0, xzr                     // zero-length segment: t = 0
    b       path_step.fast_position
path_step.fast_ratio:
    fdiv    d0, d0, d1
    ldr     w6, [x4, #PA_EASE]
    cmn     w6, #1
    b.ne    path_step.fast_position     // eased: unclamped, overshoot allowed
    fmov    d1, #1.0
    fminnm  d0, d0, d1                  // f64::min(x, 1.0)
path_step.fast_position:
    ldr     x0, [x5, #(SG_START + WP_COORD)]
    ldr     x1, [x5, #(SG_END + WP_COORD)]
    ldr     w2, [x5, #(SG_END + WP_BEZ_COUNT)]
    cbnz    w2, path_step.fast_curve
    b       find_coord_on_line
path_step.fast_curve:
    mov     x3, x1
    ldr     x1, [x5, #(SG_END + WP_BEZ)]
    b       find_coord_on_bezier_curve
path_step.at_end:
    ldr     w9, [x4, #PA_SEG_COUNT]
    sub     w9, w9, #1
    mov     w6, #SEGMENT_SIZE
    ldr     x5, [x4, #PA_SEGS]
    umaddl  x9, w9, w6, x5
    ldr     x9, [x9, #(SG_END + WP_COORD)]
    ret
path_step.shared_walk:
    // the walk over a shared list: its owner has no segment events, so a
    // passed segment only extends PA_DONE (see path_unshare)
    ldr     w6, [x4, #PA_SEG_COUNT]
    cmp     w3, w6
    b.hs    path_step.shared_over
    mov     w9, #SEGMENT_SIZE
    umull   x9, w3, w9
    ldr     x5, [x4, #PA_SEGS]
    add     x6, x5, x9
    ldr     d1, [x6, #SG_DISTANCE]
    fcmp    d0, d1
    b.le    path_step.finish            // <= or unordered
    fsub    d0, d0, d1
    ldrh    w2, [x4, #PA_DONE]
    cmp     w3, w2
    b.ne    path_step.shared_next
    add     w2, w2, #1
    strh    w2, [x4, #PA_DONE]
    add     w2, w2, #1
    ldrh    w10, [x4, #PA_INT_COUNT]
    cmp     w10, w2
    csel    w10, w2, w10, hi
    strh    w10, [x4, #PA_REACH]
path_step.shared_next:
    add     w3, w3, #1
    b       path_step.shared_walk
path_step.shared_over:
    // for-else: overshoot past the last waypoint re-adds its distance
    sub     w9, w3, #1
    mov     w6, #SEGMENT_SIZE
    umull   x9, w9, w6
    ldr     x5, [x4, #PA_SEGS]
    add     x6, x5, x9
    ldr     d1, [x6, #SG_DISTANCE]
    fadd    d0, d0, d1
    b       path_step.finish
path_step.slow:
    // the walk from segment w3 with d0 still to travel, which may fire
    // segment events
    ldr     w6, [x4, #PA_FLAGS]
    tbnz    w6, #3, path_step.shared_walk   // PAF_SHARED
    stp     x19, x20, [sp, #-96]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    // [sp, #48] distance left, [sp, #56] exit already triggered,
    // [sp, #64] the end waypoint's key (32 bytes)
    mov     w19, w0
    mov     x20, x4
    mov     w22, #-1                    // active segment: NONE
    mov     w23, w3                     // i
    str     d0, [sp, #48]
path_step.walk:
    ldr     w6, [x20, #PA_SEG_COUNT]
    cmp     w23, w6
    b.hs    path_step.walked
    mov     w9, #SEGMENT_SIZE
    ldr     x6, [x20, #PA_SEGS]
    umaddl  x24, w23, w9, x6            // segments[i]
    ldr     d0, [sp, #48]
    ldr     d1, [x24, #SG_DISTANCE]
    fcmp    d0, d1
    b.gt    path_step.beyond
path_step.holds:
    // this segment holds the destination
    mov     w22, w23
    ldrb    w6, [x24, #SG_ENTERED]
    cbnz    w6, path_step.walked
    mov     w6, #1
    strb    w6, [x24, #SG_ENTERED]
    LDX     x9, ch_subs
    ldrb    w9, [x9, x19]
    tbz     w9, #EV_SEGMENT_ENTERED, path_step.walked
    ldp     x6, x7, [x24, #SG_END]
    stp     x6, x7, [sp, #64]           // the end waypoint's key
    ldp     x6, x7, [x24, #(SG_END + 16)]
    stp     x6, x7, [sp, #80]
    mov     w0, w19
    mov     w1, #EV_SEGMENT_ENTERED
    mov     w2, #CALLER_WAYPOINT
    add     x3, sp, #64
    bl      handle_event
    b       path_step.walked
path_step.beyond:
    fsub    d0, d0, d1
    str     d0, [sp, #48]
    ldrh    w6, [x24, #SG_ENTERED]
    cmp     w6, #0x0101
    b.eq    path_step.advance           // both events already fired
    LDX     x9, ch_subs
    ldrb    w9, [x9, x19]
    tst     w9, #((1 << EV_SEGMENT_ENTERED) | (1 << EV_SEGMENT_EXITED))
    b.ne    path_step.observed
    mov     w6, #0x0101
    strh    w6, [x24, #SG_ENTERED]
    b       path_step.advance
path_step.observed:
    ldp     x6, x7, [x24, #SG_END]
    stp     x6, x7, [sp, #64]
    ldp     x6, x7, [x24, #(SG_END + 16)]
    stp     x6, x7, [sp, #80]
    ldrb    w6, [x24, #SG_EXITED]
    strb    w6, [sp, #56]               // exit already triggered?
    ldrb    w6, [x24, #SG_ENTERED]
    cbnz    w6, path_step.exit
    mov     w6, #1
    strb    w6, [x24, #SG_ENTERED]
    mov     w0, w19
    mov     w1, #EV_SEGMENT_ENTERED
    mov     w2, #CALLER_WAYPOINT
    add     x3, sp, #64
    bl      handle_event
path_step.exit:
    ldrb    w6, [sp, #56]
    cbnz    w6, path_step.reload
    mov     w9, #SEGMENT_SIZE
    ldr     x6, [x20, #PA_SEGS]
    umaddl  x24, w23, w9, x6
    mov     w6, #1
    strb    w6, [x24, #SG_EXITED]
    mov     w0, w19
    mov     w1, #EV_SEGMENT_EXITED
    mov     w2, #CALLER_WAYPOINT
    add     x3, sp, #64
    bl      handle_event
path_step.reload:
    // an action may have grown (moved) or reset the segments
    ldr     w6, [x20, #PA_SEG_COUNT]
    cmp     w23, w6
    b.hs    path_step.next_segment
    mov     w9, #SEGMENT_SIZE
    ldr     x6, [x20, #PA_SEGS]
    umaddl  x24, w23, w9, x6
path_step.advance:
    // extend the all-fired prefix
    ldrh    w9, [x20, #PA_DONE]
    cmp     w9, w23
    b.ne    path_step.next_segment
    ldrh    w6, [x24, #SG_ENTERED]
    cmp     w6, #0x0101
    b.ne    path_step.next_segment
    mov     w6, #0xfffe
    cmp     w9, w6
    b.hi    path_step.next_segment
    add     w9, w9, #1
    strh    w9, [x20, #PA_DONE]
    MO_PATH_REACH x20
path_step.next_segment:
    add     w23, w23, #1
    b       path_step.walk
path_step.walked:
    cmn     w22, #1
    b.ne    path_step.have_segment
    // for-else: overshoot past the last waypoint re-adds its distance
    ldr     w22, [x20, #PA_SEG_COUNT]
    sub     w22, w22, #1
    mov     w9, #SEGMENT_SIZE
    ldr     x6, [x20, #PA_SEGS]
    umaddl  x24, w22, w9, x6
    ldr     d0, [sp, #48]
    ldr     d1, [x24, #SG_DISTANCE]
    fadd    d0, d0, d1
    str     d0, [sp, #48]
path_step.have_segment:
    mov     w9, #SEGMENT_SIZE
    ldr     x6, [x20, #PA_SEGS]
    umaddl  x24, w22, w9, x6
    ldr     d1, [x24, #SG_DISTANCE]
    fmov    d0, xzr
    fcmp    d1, #0.0
    b.ne    path_step.ratio             // not equal, or unordered
    b       path_step.position          // zero-length segment: t = 0
path_step.ratio:
    ldr     d0, [sp, #48]
    fdiv    d0, d0, d1
    ldr     w6, [x20, #PA_EASE]
    cmn     w6, #1
    b.ne    path_step.position          // eased: unclamped, overshoot allowed
    fmov    d1, #1.0
    fminnm  d0, d0, d1                  // f64::min(x, 1.0)
path_step.position:
    ldr     x0, [x24, #(SG_START + WP_COORD)]
    ldr     x1, [x24, #(SG_END + WP_COORD)]
    ldr     w2, [x24, #(SG_END + WP_BEZ_COUNT)]
    cbnz    w2, path_step.curve
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x20, [sp], #96
    b       find_coord_on_line
path_step.curve:
    mov     x3, x1
    ldr     x1, [x24, #(SG_END + WP_BEZ)]
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x20, [sp], #96
    b       find_coord_on_bezier_curve

// motion_move(w0=slot): Motion.move - step the active path, then holds,
// loops, completion and their events.
motion_move:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     w19, w0
    // (Motion.previous_coord is not kept: nothing reads it)
    LDX     x9, ch_path
    ldr     w1, [x9, x19, lsl #2]
    cmn     w1, #1
    b.eq    motion_move.done
    PATH_PTR x9, x1
    ldr     w6, [x9, #PA_SEG_COUNT]
    cbz     w6, motion_move.done
    mov     w0, w19
    bl      path_step
motion_move.stepped:
    // an unchanged coordinate needs no set_coordinate: the character's
    // render cell already matches it
    LDX     x3, ch_col
    ldr     w6, [x3, x19, lsl #2]
    cmp     w6, w9
    b.ne    motion_move.moved
    asr     x2, x9, #32
    LDX     x3, ch_row
    ldr     w6, [x3, x19, lsl #2]
    cmp     w6, w2
    b.eq    motion_move.placed
motion_move.moved:
    mov     w0, w19
    mov     x1, x9
    bl      set_coordinate
motion_move.placed:
    // Python re-reads active_path after the step (a callback may swap it)
    LDX     x9, ch_path
    ldr     w21, [x9, x19, lsl #2]
    cmn     w21, #1
    b.eq    motion_move.cleared
    PATH_PTR x22, x21
    ldr     x9, [x22, #PA_STEP]
    ldr     x6, [x22, #PA_MAX]
    cmp     x9, x6
    b.ne    motion_move.done
    ldr     x9, [x22, #PA_HOLD]
    cbz     x9, motion_move.no_hold
    ldr     x6, [x22, #PA_HOLD_LEFT]
    cmp     x9, x6
    b.ne    motion_move.no_hold
    LDX     x9, ch_subs
    ldrb    w9, [x9, x19]
    tbz     w9, #EV_PATH_HOLDING, motion_move.hold
    mov     w0, w19
    mov     w1, #EV_PATH_HOLDING
    mov     w2, #CALLER_PATH
    ldr     w3, [x22, #PA_NAME]
    bl      handle_event
motion_move.hold:
    PATH_PTR x22, x21
    ldr     x9, [x22, #PA_HOLD_LEFT]
    sub     x9, x9, #1
    str     x9, [x22, #PA_HOLD_LEFT]
    b       motion_move.done
motion_move.no_hold:
    ldr     x9, [x22, #PA_HOLD_LEFT]
    cbz     x9, motion_move.held
    sub     x9, x9, #1
    str     x9, [x22, #PA_HOLD_LEFT]
    b       motion_move.done
motion_move.held:
    ldr     w6, [x22, #PA_FLAGS]
    tbz     w6, #1, motion_move.complete    // PAF_LOOP
    ldr     w6, [x22, #PA_SEG_COUNT]
    cmp     w6, #1
    b.ls    motion_move.complete
    // loop: deactivate and activate again
    mov     w0, w19
    ldr     w1, [x22, #PA_NAME]
    bl      path_deactivate
    mov     w0, w19
    mov     w1, w21
    bl      path_activate
    b       motion_move.done
motion_move.complete:
    LDX     x9, ch_done_path
    str     w21, [x9, x19, lsl #2]
    mov     w0, w19
    ldr     w1, [x22, #PA_NAME]
    bl      path_deactivate
    LDX     x9, ch_subs
    ldrb    w9, [x9, x19]
    tbz     w9, #EV_PATH_COMPLETE, motion_move.done
    mov     w0, w19
    mov     w1, #EV_PATH_COMPLETE
    mov     w2, #CALLER_PATH
    ldr     w3, [x22, #PA_NAME]
    bl      handle_event
motion_move.done:
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret
motion_move.cleared:
    ADRG    x0, msg_path_cleared
    mov     w1, #msg_path_cleared_len
    b       fatal

// ------------------------------------------------------------ batched steps
//
// The x86 engine's TIER 3+ motion batch (update works out a bitmap word's
// pure path steps ahead, in vectors, from per-slot mirrors of the path
// state) is left out: at TIER 1, which this port follows, update calls
// motion_move for every mover, run_action does not call motion_void, the
// synced scene step reads the path through path_view, and nothing retires
// a mirror.
// dropped: path_sync_index mv_sync mv_retire motion_void motion_batch motion_apply (TIER 3+ only in the x86 engine)
// dropped: mb_split mv_lane_masks mv_flag_curve mv_flag_over mv_nan mv_two_p51 mv_flag_first mv_tag_bit mv_key_final (the TIER 3+ batch's constants)
// dropped: mv_base path_owners mb_write_bits mb_tail_bits mb_moved_bits mb_act_bits mb_mirror_bits mb_idle_bits mb_bare_bits mb_first mv_synced mv_view mb (the TIER 3+ batch's state)

// path_view(w0=slot, w9=its active path) -> x2 = the path's current_step,
// max_steps, total_distance and last_distance_reached at their PA_*
// offsets, for the synced scene step: the record (TIER 1 keeps no mirrors).
// Preserves everything else but x16.
path_view:
    ubfiz   x2, x9, #7, #32             // PATH_SIZE
    adrp    x16, paths
    ldr     x16, [x16, :lo12:paths]
    add     x2, x2, x16
    ret

// paths_clear(w0=slot): drop the character's path map (particle resets).
// Clobbers x9, x16.
paths_clear:
    LDX     x9, ch_paths
    mov     w16, #-1
    str     w16, [x9, w0, uxtw #2]
    ret

// path_reset(w0=path): `motion.paths.remove(id)` followed by `new_path` with
// the same id and parameters (rings' "disperse"): the record is emptied in
// place - no waypoints, segments or distances, playback at the start - and
// keeps its name, speed, easing, layer, hold time and loop flag. Its arrays'
// capacity is reused. Clobbers x9, x16.
path_reset:
    PATH_PTR x9, x0
    ldr     w16, [x9, #PA_FLAGS]
    tbz     w16, #3, path_reset.own     // PAF_SHARED
    str     xzr, [x9, #PA_SEGS]         // the next segment starts a list
    str     wzr, [x9, #PA_SEG_CAP]
path_reset.own:
    and     w16, w16, #(0xffffffff ^ (PAF_ORIGIN | PAF_SHARED))
    str     w16, [x9, #PA_FLAGS]
    str     wzr, [x9, #PA_WP_COUNT]
    str     wzr, [x9, #PA_SEG_COUNT]
    str     xzr, [x9, #PA_TOTAL]
    str     xzr, [x9, #PA_STEP]
    str     xzr, [x9, #PA_MAX]
    str     wzr, [x9, #PA_ETAB]
    str     xzr, [x9, #PA_LAST]
    str     xzr, [x9, #PA_ORIGIN_DIST]
    str     xzr, [x9, #PA_INT_COUNT]    // and PA_DONE, PA_CURSOR, PA_REACH
    ldr     x16, [x9, #PA_HOLD]
    str     x16, [x9, #PA_HOLD_LEFT]
    ret

    .section .rodata
    .balign 8
path_one:   .double 1.0
path_two_p52: .8byte 0x4330000000000000     // 2^52
STRING msg_path_speed, "ttfx: asm engine: path speed must be greater than 0\n"
STRING msg_duplicate_path, "ttfx: asm engine: duplicate path id\n"
STRING msg_duplicate_waypoint, "ttfx: asm engine: duplicate waypoint id\n"
STRING msg_paths_full, "ttfx: asm engine: path limit reached\n"
STRING msg_empty_path, "ttfx: asm engine: activated an empty path\n"
STRING msg_path_missing, "ttfx: asm engine: path not found\n"
STRING msg_path_cleared, "ttfx: asm engine: active path cleared mid-move\n"

    TSTATE
    .balign 8
paths:          .skip 8
etab_base:      .skip 8
etab_map:       .skip 8
etab_used:      .skip 8             // in f64 entries
path_count:     .skip 4
segshare_count: .skip 4             // lists in segshare_table
segshare_hits:  .skip 4             // lookups that found one
    .balign 8
segshare_table: .skip 8
segshare_off:   .skip 1             // lookups stopped
    .balign 4
motion_epoch:   .skip 4             // bumped by actions that may touch any path

    .text
