// engine/particles.s - ParticlePool / ParticleReset
// (src/engine/particles.rs).
//
// A pool lives in the effect's own memory (a POOL struc, defs.inc). Rust
// passes the reset flags and the initializer closure per call; here they are
// pool fields the effect sets (and may change between calls). Callbacks take
// (w0 = particle slot, x1 = the matching user word).
//
// The available queue pops and pushes on the right (Python deque.pop /
// append). Membership for reclaim's "no duplicate entries" rule is a flag bit
// on the character (CF_POOLED), since a character belongs to at most one
// pool.
//
// CF_POOLED, the RESET_* bits (ParticleReset::default is RESET_DEFAULT:
// CLEAR_PATHS | DEACTIVATE_PATH | DEACTIVATE_SCENE), POOL.* and
// POOL_CAPACITY are in defs.inc.

    .text

.equ PT_QUEUE_BYTES, POOL_CAPACITY * 4

// pool_init(x0=pool, x1=symbols, x2=symbol count, x3=max size or -1,
//           x4=coord). ParticlePool::new: at least one symbol.
pool_init:
    PUSH2   x19, x30
    mov     x19, x0
    cbz     x2, pool_init.no_symbols
    stp     x1, x2, [x19, #POOL.symbols]
    stp     x3, x4, [x19, #POOL.max_size]
    mov     x9, #RESET_DEFAULT
    stp     x9, xzr, [x19, #POOL.reset]     // reset, initializer
    MOV64   x0, PT_QUEUE_BYTES
    bl      reserve_small
    str     x9, [x19, #POOL.available]
    MOV64   x0, PT_QUEUE_BYTES
    bl      reserve_small
    str     x9, [x19, #POOL.particles]
    str     xzr, [x19, #POOL.available_count]
    str     xzr, [x19, #POOL.particle_count]
    POP2    x19, x30
    ret
pool_init.no_symbols:
    FAIL    msg_pool_symbols

// pool_preallocate(x0=pool, x1=count): the initial_count loop of __init__,
// using the pool's initializer.
pool_preallocate:
    stp     x19, x21, [sp, #-32]!
    str     x30, [sp, #16]
    mov     x19, x0
    mov     x21, x1
    ldr     x9, [x19, #POOL.max_size]
    cmn     x9, #1
    b.eq    pool_preallocate.loop
    cmp     x9, x21
    b.lo    pool_preallocate.too_small
pool_preallocate.loop:
    cbz     x21, pool_preallocate.done
    mov     x0, x19
    mov     x1, #0
    bl      pool_create_particle
    mov     x0, x19
    bl      pool_push_available
    sub     x21, x21, #1
    b       pool_preallocate.loop
pool_preallocate.done:
    ldr     x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret
pool_preallocate.too_small:
    FAIL    msg_pool_max

// pool_create_particle(x0=pool, x1=symbol or 0 for a random one) -> w9.
// _create_particle: the symbol (drawn from the pool's symbols when none is
// given), a character at the pool's coordinate, the initializer, ownership.
pool_create_particle:
    stp     x19, x21, [sp, #-32]!
    str     x30, [sp, #16]
    mov     x19, x0
    mov     x2, x1
    cbnz    x2, pool_create_particle.symbol
    ldr     x0, [x19, #POOL.symbol_count]
    bl      rng_below
    ldr     x3, [x19, #POOL.symbols]
    ldr     x2, [x3, x9, lsl #3]
pool_create_particle.symbol:
    mov     x0, x2
    ldr     x1, [x19, #POOL.coord]
    bl      add_character
    mov     w21, w9
    ldr     x9, [x19, #POOL.initializer]
    cbz     x9, pool_create_particle.own
    mov     w0, w21
    ldr     x1, [x19, #POOL.init_user]
    blr     x9
pool_create_particle.own:
    ldr     x9, [x19, #POOL.particle_count]
    ldr     x3, [x19, #POOL.particles]
    str     w21, [x3, x9, lsl #2]
    add     x9, x9, #1
    str     x9, [x19, #POOL.particle_count]
    mov     w9, w21
    ldr     x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// pool_push_available(x0=pool, w9=slot) - internal: push the slot.
// Clobbers x3, x2.
pool_push_available:
    ldr     x3, [x0, #POOL.available_count]
    ldr     x2, [x0, #POOL.available]
    str     w9, [x2, x3, lsl #2]
    add     x3, x3, #1
    str     x3, [x0, #POOL.available_count]
    LDX     x2, ch_flags
    ldrh    w3, [x2, w9, uxtw #1]
    orr     w3, w3, #CF_POOLED
    strh    w3, [x2, w9, uxtw #1]
    ret

// particle_reset(w0=slot, w1=RESET_* bits): _reset_particle.
particle_reset:
    stp     x19, x21, [sp, #-32]!
    str     x30, [sp, #16]
    mov     w19, w0
    mov     w21, w1
    bl      doze_wake
    tst     w21, #RESET_DEACTIVATE_PATH
    b.eq    particle_reset.scene
    LDX     x9, ch_path
    mov     w3, #NONE
    str     w3, [x9, x19, lsl #2]
    mov     w0, w19
    MARK_CANDIDATE
particle_reset.scene:
    tst     w21, #RESET_DEACTIVATE_SCENE
    b.eq    particle_reset.paths
    LDX     x9, ch_scene
    mov     w3, #NONE
    str     w3, [x9, x19, lsl #2]
    mov     w0, w19
    MARK_CANDIDATE
particle_reset.paths:
    tst     w21, #RESET_CLEAR_PATHS
    b.eq    particle_reset.scenes
    mov     w0, w19
    bl      paths_release
particle_reset.scenes:
    tst     w21, #RESET_CLEAR_SCENES
    b.eq    particle_reset.events
    mov     w0, w19
    bl      scenes_release
particle_reset.events:
    tst     w21, #RESET_CLEAR_EVENTS
    b.eq    particle_reset.appearance
    mov     w0, w19
    bl      events_release
particle_reset.appearance:
    tst     w21, #RESET_APPEARANCE
    b.eq    particle_reset.done
    mov     w0, w19
    bl      reset_appearance
particle_reset.done:
    ldr     x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// pool_acquire(x0=pool, x1=symbol or 0) -> w9 = slot or NONE (the pool is
// at max_size). ParticlePool.acquire with the pool's reset and initializer.
pool_acquire:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     x19, x0
    mov     x21, x1
    ldr     x9, [x19, #POOL.available_count]
    cbz     x9, pool_acquire.create
    sub     x9, x9, #1
    str     x9, [x19, #POOL.available_count]
    ldr     x3, [x19, #POOL.available]
    ldr     w22, [x3, x9, lsl #2]
    LDX     x3, ch_flags
    ldrh    w9, [x3, x22, lsl #1]
    and     w9, w9, #0xfffffeff               // ~CF_POOLED
    strh    w9, [x3, x22, lsl #1]
    mov     w0, w22
    ldr     x1, [x19, #POOL.reset]
    bl      particle_reset
    cbz     x21, pool_acquire.done
    // a given symbol becomes the particle's input symbol and appearance
    LDX     x9, ch_sym
    str     x21, [x9, x22, lsl #3]
    mov     w0, w22
    mov     x1, x21
    mov     x2, #NONE
    mov     x3, #NONE
    bl      set_appearance
    b       pool_acquire.done
pool_acquire.create:
    ldr     x9, [x19, #POOL.max_size]
    cmn     x9, #1
    b.eq    pool_acquire.new
    ldr     x3, [x19, #POOL.particle_count]
    cmp     x3, x9
    b.hs    pool_acquire.exhausted
pool_acquire.new:
    mov     x0, x19
    mov     x1, x21
    bl      pool_create_particle
    mov     w22, w9
    mov     w0, w9
    ldr     x1, [x19, #POOL.reset]
    bl      particle_reset
pool_acquire.done:
    mov     w9, w22
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret
pool_acquire.exhausted:
    mov     w9, #NONE
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// pool_emit(x0=pool, x1=origin coord, x2=symbol or 0, w3=visible,
//           x4=on_emit fn or 0, x5=on_emit user) -> w9 = slot or NONE.
// ParticlePool.emit: acquire, position, on_emit, visibility, activate.
pool_emit:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    mov     x21, x1
    mov     w22, w3
    mov     x23, x4
    mov     x24, x5
    mov     x1, x2
    bl      pool_acquire
    cmn     w9, #1
    b.eq    pool_emit.done
    mov     w19, w9
    mov     w0, w19
    mov     x1, x21
    bl      set_coordinate
    cbz     x23, pool_emit.visible
    mov     w0, w19
    mov     x1, x24
    blr     x23
pool_emit.visible:
    mov     w0, w19
    mov     w1, w22
    bl      set_visibility
    mov     w0, w19
    bl      active_insert
    mov     w9, w19
pool_emit.done:
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// pool_reclaim(x0=pool, w1=slot, w2=hide, w3=deactivate):
// ParticlePool.reclaim (idempotent: no duplicate queue entries).
pool_reclaim:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     x21, x0
    mov     w19, w1
    mov     w22, w3
    cbz     w2, pool_reclaim.deactivate
    mov     w0, w19
    mov     w1, #0
    bl      set_visibility
pool_reclaim.deactivate:
    cbz     w22, pool_reclaim.remove
    mov     w0, w19
    bl      doze_wake
    mov     w3, #NONE
    LDX     x9, ch_path
    str     w3, [x9, x19, lsl #2]
    LDX     x9, ch_scene
    str     w3, [x9, x19, lsl #2]
pool_reclaim.remove:
    mov     w0, w19
    bl      active_remove
    LDX     x9, ch_flags
    ldrh    w3, [x9, x19, lsl #1]
    tst     w3, #CF_POOLED
    b.ne    pool_reclaim.done
    mov     x0, x21
    mov     w9, w19
    bl      pool_push_available
pool_reclaim.done:
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// pool_extend(x0=pool, x1=u32 slots, x2=count): adopt existing
// characters, no reset.
pool_extend:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    mov     x19, x0
    mov     x21, x1
    mov     x22, x2
pool_extend.next:
    cbz     x22, pool_extend.done
    ldr     w9, [x21]
    ldr     x3, [x19, #POOL.particle_count]
    ldr     x2, [x19, #POOL.particles]
    str     w9, [x2, x3, lsl #2]
    add     x3, x3, #1
    str     x3, [x19, #POOL.particle_count]
    mov     x0, x19
    bl      pool_push_available
    add     x21, x21, #4
    sub     x22, x22, #1
    b       pool_extend.next
pool_extend.done:
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// ------------------------------------------------------------ recycling
//
// Rust drops the paths, scenes and events a reset clears (and thunderstorm's
// strike characters' scenes and events on reuse). Here records live in
// index-addressed regions that never shrink, so a particle effect running
// for hours would grow without bound. Cleared records therefore go to free
// lists, one per kind, which path_new, scene_alloc and event_register take
// from first.
//
// A release (at the reset) only puts the records in limbo, untouched:
// a reset can run inside an event dispatch or update's tick, which keep
// reading the old records (handle_event reads an action's successor after
// running it). recycle_flush, at the start of the next frame, moves them to
// the free lists. The storage a record owns goes with it to its next life:
// a path's waypoint and segment arrays (zeroed, as alloc's are) and its
// bezier control blocks, and a scene's frames. Records and arrays are only
// ever addressed, never compared or ordered, so which memory they reuse is
// unobservable.
//
// Kept out of reuse, because something else may still point at them:
// - an active path or scene a reset leaves active (it stays off the map);
// - a synced scene's frames (share_table may hand them to other scenes);
// - the controls of a path whose segments were ever shared (segshare_table
//   copies the waypoints, pointers and all), and every path's controls once
//   an event is keyed by a waypoint (the entry copies the pointer).
//
// PAF_EVER_SHARED (the path walked a shared list once), RC_CAP_SHIFT
// (parked arrays: pointer | capacity << 48), RC_CAP_MAX and BEZ_BLOCK_COUNT
// (controls that fit a 64-byte block) are in defs.inc.

.equ PT_LOW48, (1 << RC_CAP_SHIFT) - 1
.equ PT_BANK_HASH, 0x9E3779B1               // scene_alloc's bank hash
.equ PT_SHAPE_SHARED, SCF_SHAPE | SCF_SHARED

// paths_release(w0=slot): Motion.paths.clear(). Clobbers x9, x3, x2.
paths_release:
    LDX     x9, ch_paths
    ldr     w3, [x9, w0, uxtw #2]
    mov     w2, #NONE
    str     w2, [x9, w0, uxtw #2]
    LDX     x9, ch_path
    ldr     w2, [x9, w0, uxtw #2]       // left active: not released
paths_release.next:
    cmn     w3, #1
    b.eq    paths_release.done
    PATH_PTR x12, x3
    ldr     w13, [x12, #PA_NEXT]
    cmp     w3, w2
    b.eq    paths_release.skip
    LDW     w9, rc_path_limbo
    sub     w9, w9, #1                  // head index or NONE
    str     w9, [x12, #PA_NEXT]
    add     w9, w3, #1
    STW     w9, rc_path_limbo
paths_release.skip:
    mov     w3, w13
    b       paths_release.next
paths_release.done:
    ret

// scenes_release(w0=slot): Animation.scenes.clear(). Clobbers x9, x3, x2.
scenes_release:
    LDX     x9, ch_scenes
    ldr     w3, [x9, w0, uxtw #2]
    mov     w2, #NONE
    str     w2, [x9, w0, uxtw #2]
    LDX     x9, ch_scene
    ldr     w2, [x9, w0, uxtw #2]       // left active: not released
    MOV64   x14, SC_NEXT
scenes_release.next:
    cmn     w3, #1
    b.eq    scenes_release.done
    SCENE_PTR x12, x3
    ldr     w13, [x12, x14]
    cmp     w3, w2
    b.eq    scenes_release.skip
    LDW     w9, rc_scene_limbo
    sub     w9, w9, #1
    str     w9, [x12, x14]
    add     w9, w3, #1
    STW     w9, rc_scene_limbo
scenes_release.skip:
    mov     w3, w13
    b       scenes_release.next
scenes_release.done:
    ret

// events_release(w0=slot): EventHandler.clear, the entries (and with them
// their actions) to limbo. Clobbers x9, x3, x2 (and what event_clear does).
events_release:
    LDX     x9, ch_events
    ldr     w3, [x9, w0, uxtw #2]
events_release.next:
    cmn     w3, #1
    b.ne    events_release.entry
    b       event_clear
events_release.entry:
    LDX     x2, event_entries
    add     x2, x2, x3, lsl #6          // ENTRY_SIZE
    LDW     w9, rc_entry_limbo
    sub     w9, w9, #1
    ldr     w12, [x2, #EN_NEXT]
    str     w9, [x2, #EN_NEXT]
    add     w3, w3, #1
    STW     w3, rc_entry_limbo
    mov     w3, w12
    b       events_release.next

// recycle_flush: limbo to the free lists. Called by next_frame (lib.s)
// before the effect's next_frame, when no dispatch or tick is running.
// Preserves x19-x24.
recycle_flush:
    LDW     w9, rc_path_limbo
    LDW     w3, rc_scene_limbo
    orr     w9, w9, w3
    LDW     w3, rc_entry_limbo
    orr     w9, w9, w3
    cbnz    w9, recycle_flush.work
    ret
recycle_flush.work:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    str     x30, [sp, #32]
recycle_flush.path:
    LDW     w9, rc_path_limbo
    cbz     w9, recycle_flush.scene
    sub     w19, w9, #1
    PATH_PTR x20, x19
    ldr     w9, [x20, #PA_NEXT]
    add     w9, w9, #1
    STW     w9, rc_path_limbo
    bl      path_recycle
    b       recycle_flush.path
recycle_flush.scene:
    LDW     w9, rc_scene_limbo
    cbz     w9, recycle_flush.entry
    sub     w19, w9, #1
    SCENE_PTR x20, x19
    MOV64   x12, SC_NEXT
    ldr     w9, [x20, x12]
    add     w9, w9, #1
    STW     w9, rc_scene_limbo
    bl      scene_recycle
    b       recycle_flush.scene
recycle_flush.entry:
    LDW     w9, rc_entry_limbo
    cbz     w9, recycle_flush.done
    sub     w9, w9, #1
    LDX     x2, event_entries
    add     x2, x2, x9, lsl #6
    ldr     w3, [x2, #EN_NEXT]
    add     w3, w3, #1
    STW     w3, rc_entry_limbo
    // the actions, first to last, onto the action free list
    ldr     w3, [x2, #EN_FIRST]
    cmn     w3, #1
    b.eq    recycle_flush.entry_free
    ldr     w4, [x2, #EN_LAST]
    LDX     x12, event_actions
    add     x4, x12, x4, lsl #5         // ACTION_SIZE
    LDW     w5, rc_action_free
    sub     w5, w5, #1
    str     w5, [x4, #AC_NEXT]
    add     w3, w3, #1
    STW     w3, rc_action_free
recycle_flush.entry_free:
    LDW     w3, rc_entry_free
    sub     w3, w3, #1
    str     w3, [x2, #EN_NEXT]
    add     w9, w9, #1
    STW     w9, rc_entry_free
    b       recycle_flush.entry
recycle_flush.done:
    ldr     x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// path_recycle(w19=path, x20=record): recycle_flush for one path - its
// arrays zeroed and kept in the record, its bezier controls freed, the
// record onto the free list. (The x86 engine's TIER 3+ motion mirror is
// not ported, so there is no mirror to drop.) Clobbers x9, x3, x2, x1, x0,
// x4, x5, x10, x11, x21, x22.
path_recycle:
    // the segments: its own list, or the one parked while it walked a
    // shared list (never the shared list itself)
    ldr     x21, [x20, #PA_SEGS]
    ldr     w22, [x20, #PA_SEG_CAP]
    LDX     x9, rc_path_priv
    cbz     x9, path_recycle.unparked
    ldr     x3, [x9, x19, lsl #3]
    cbz     x3, path_recycle.unparked
    str     xzr, [x9, x19, lsl #3]
    ldr     w2, [x20, #PA_FLAGS]
    tst     w2, #PAF_SHARED
    b.eq    path_recycle.segs           // it has its own (the parked one is dropped)
    lsr     x22, x3, #RC_CAP_SHIFT
    and     x21, x3, #PT_LOW48
    b       path_recycle.segs
path_recycle.unparked:
    ldr     w2, [x20, #PA_FLAGS]
    tst     w2, #PAF_SHARED
    b.eq    path_recycle.segs
    mov     x21, #0
    mov     w22, #0
path_recycle.segs:
    cbnz    x21, path_recycle.segs_zero
    mov     w22, #0
path_recycle.segs_zero:
    mov     x0, x21
    mov     w2, #SEGMENT_SIZE
    mul     w3, w22, w2
    mov     w9, #0
    REP_STOSB
    str     x21, [x20, #PA_SEGS]
    str     w22, [x20, #PA_SEG_CAP]
    // the controls
    LDB     w9, rc_wp_events
    cbnz    w9, path_recycle.wps
    ldr     w9, [x20, #PA_FLAGS]
    tst     w9, #PAF_EVER_SHARED
    b.ne    path_recycle.wps
    ldr     x1, [x20, #PA_WPS]
    ldr     w3, [x20, #PA_WP_COUNT]
path_recycle.bez:
    cbz     w3, path_recycle.wps
    ldr     x2, [x1, #WP_BEZ]
    cbz     x2, path_recycle.bez_next
    ldr     w9, [x1, #WP_BEZ_COUNT]
    cmp     w9, #BEZ_BLOCK_COUNT
    b.hi    path_recycle.bez_next
    LDX     x9, rc_bez_free
    str     x9, [x2]
    STX     x2, rc_bez_free
path_recycle.bez_next:
    add     x1, x1, #WAYPOINT_SIZE
    sub     w3, w3, #1
    b       path_recycle.bez
path_recycle.wps:
    ldr     x0, [x20, #PA_WPS]
    ldr     w3, [x20, #PA_WP_CAP]
    cbnz    x0, path_recycle.wps_zero
    mov     w3, #0
    str     w3, [x20, #PA_WP_CAP]
path_recycle.wps_zero:
    lsl     w3, w3, #5                  // WAYPOINT_SIZE
    mov     w9, #0
    REP_STOSB
    // onto the free list
    LDW     w9, rc_path_free
    sub     w9, w9, #1
    str     w9, [x20, #PA_NEXT]
    add     w9, w19, #1
    STW     w9, rc_path_free
    ret

// scene_recycle(w19=scene, x20=record): recycle_flush for one scene - its
// frame block parked for its next life, the record onto its bank's free
// list. Clobbers x9, x3, x2, x1, x0, x4, x5, x10, x11.
scene_recycle:
    ldr     x1, [x20, #SC_FRAMES]
    cbz     x1, scene_recycle.free
    LDX     x4, rc_scene_blk
    cbnz    x4, scene_recycle.have_table
    PUSH1   x30
    mov     x0, #(SCENE_LIMIT * 8)
    bl      reserve_small
    POP1    x30
    STX     x9, rc_scene_blk
    mov     x4, x9
    ldr     x1, [x20, #SC_FRAMES]
scene_recycle.have_table:
    add     x4, x4, x19, lsl #3
    mov     x9, #0
    ldr     w3, [x20, #SC_FLAGS]
    tst     w3, #SCF_SYNC
    b.ne    scene_recycle.park          // share_table may hand them out
    ldr     w3, [x20, #SC_COUNT]
    ldr     x2, [x4]
    and     x9, x2, #PT_LOW48
    cmp     x9, x1
    b.ne    scene_recycle.own           // it moved: the old block is lost
    lsr     x2, x2, #RC_CAP_SHIFT
    cmp     w2, w3
    csel    w3, w2, w3, hi
scene_recycle.own:
    mov     x9, #0
    mov     w2, #RC_CAP_MAX
    cmp     w3, w2
    b.hi    scene_recycle.park
    lsl     x9, x3, #RC_CAP_SHIFT
    orr     x9, x9, x1
scene_recycle.park:
    str     x9, [x4]
scene_recycle.free:
    LDB     w9, scene_unbanked
    cbnz    w9, scene_recycle.done      // (vhstape never resets scenes)
    MOV64   x12, SC_NAME
    ldr     w9, [x20, x12]
    MOV64   x13, PT_BANK_HASH
    mul     w9, w9, w13
    lsr     w9, w9, #(32 - 6)           // scene_alloc's bank
    ADRG    x2, rc_scene_free
    add     x2, x2, x9, lsl #2
    ldr     w9, [x2]
    sub     w9, w9, #1
    MOV64   x12, SC_NEXT
    str     w9, [x20, x12]
    add     w9, w19, #1
    str     w9, [x2]
scene_recycle.done:
    ret

// ---------------------------------------- the takers (hooks in the owners)

// path_take_free -> w9 = a recycled path index, or NONE (path_new). Its
// arrays wait in rc_take_* for path_take_restore. Clobbers x3, x2.
path_take_free:
    LDW     w9, rc_path_free
    cbz     w9, path_take_free.none
    sub     w9, w9, #1
    PATH_PTR x3, x9
    ldr     w2, [x3, #PA_NEXT]
    add     w2, w2, #1
    STW     w2, rc_path_free
    ldr     x2, [x3, #PA_WPS]
    STX     x2, rc_take_wps
    ldr     x2, [x3, #PA_SEGS]
    STX     x2, rc_take_segs
    ldr     w2, [x3, #PA_WP_CAP]
    STW     w2, rc_take_wp_cap
    ldr     w2, [x3, #PA_SEG_CAP]
    STW     w2, rc_take_seg_cap
    ret
path_take_free.none:
    mov     w9, #NONE
    ret

// path_take_restore(x4=new path record): path_new, after zeroing the
// record - a recycled record's empty arrays. Clobbers x9.
path_take_restore:
    LDX     x9, rc_take_wps
    cbz     x9, path_take_restore.segs
    str     x9, [x4, #PA_WPS]
    LDW     w9, rc_take_wp_cap
    str     w9, [x4, #PA_WP_CAP]
    STX     xzr, rc_take_wps
path_take_restore.segs:
    LDX     x9, rc_take_segs
    cbz     x9, path_take_restore.done
    str     x9, [x4, #PA_SEGS]
    LDW     w9, rc_take_seg_cap
    str     w9, [x4, #PA_SEG_CAP]
    STX     xzr, rc_take_segs
path_take_restore.done:
    ret

// bez_alloc(x0=bytes) -> x9: alloc for a waypoint's bezier controls
// (path_new_waypoint), from the freed 64-byte blocks when they fit.
// Clobbers x0 (and what alloc does when it runs).
bez_alloc:
    cmp     x0, #(BEZ_BLOCK_COUNT * 8)
    b.hi    bez_alloc.alloc
    LDX     x9, rc_bez_free
    cbz     x9, bez_alloc.alloc
    ldr     x0, [x9]
    STX     x0, rc_bez_free
    stp     xzr, xzr, [x9]
    stp     xzr, xzr, [x9, #16]
    stp     xzr, xzr, [x9, #32]
    stp     xzr, xzr, [x9, #48]
    ret
bez_alloc.alloc:
    b       alloc

// path_park_segs(x20=path record): path_seg_share, as the path starts
// walking a shared list - its own list is parked for path_unshare (or its
// next life) instead of dropped. Preserves x1; clobbers x9, x3, x2, x0,
// x4, x5, x10, x11.
path_park_segs:
    ldr     w9, [x20, #PA_FLAGS]
    orr     w9, w9, #PAF_EVER_SHARED
    str     w9, [x20, #PA_FLAGS]
    LDX     x9, rc_path_priv
    cbnz    x9, path_park_segs.have_table
    PUSH2   x1, x30
    mov     x0, #(PATH_LIMIT * 8)
    bl      reserve_small
    POP2    x1, x30
    STX     x9, rc_path_priv
path_park_segs.have_table:
    LDX     x3, paths
    sub     x3, x20, x3
    lsr     x3, x3, #7                  // PATH_SIZE
    ldr     x2, [x9, x3, lsl #3]
    cbnz    x2, path_park_segs.done     // one parked already: this one is lost
    ldr     w2, [x20, #PA_SEG_CAP]
    mov     w0, #RC_CAP_MAX
    cmp     w2, w0
    b.hi    path_park_segs.done
    lsl     x2, x2, #RC_CAP_SHIFT
    ldr     x0, [x20, #PA_SEGS]
    orr     x2, x2, x0
    str     x2, [x9, x3, lsl #3]
path_park_segs.done:
    ret

// path_unpark_segs(x20=path record, w0=bytes) -> x9: path_unshare's
// alloc for its own list - the parked one when it is big enough (then
// PA_SEG_CAP is its capacity), zeroed. Clobbers x3, x2, x0, x1.
path_unpark_segs:
    LDX     x9, rc_path_priv
    cbz     x9, path_unpark_segs.alloc
    LDX     x3, paths
    sub     x3, x20, x3
    lsr     x3, x3, #7
    add     x1, x9, x3, lsl #3
    ldr     x2, [x1]
    cbz     x2, path_unpark_segs.alloc
    lsr     x9, x2, #RC_CAP_SHIFT
    mov     w3, #SEGMENT_SIZE
    mul     w3, w9, w3
    cmp     w3, w0
    b.lo    path_unpark_segs.alloc
    str     w9, [x20, #PA_SEG_CAP]
    str     xzr, [x1]
    and     x2, x2, #PT_LOW48
    mov     x0, x2
    mov     w9, #0
    REP_STOSB
    mov     x9, x2
    ret
path_unpark_segs.alloc:
    b       alloc

// scene_take_free(w21=name) -> w9 = a recycled scene index from the
// name's bank, or NONE (scene_alloc). Clobbers x3, x2.
scene_take_free:
    LDB     w9, scene_unbanked
    cbnz    w9, scene_take_free.none
    MOV64   x3, PT_BANK_HASH
    mul     w9, w21, w3
    lsr     w9, w9, #(32 - 6)
    ADRG    x2, rc_scene_free
    add     x2, x2, x9, lsl #2
    ldr     w9, [x2]
    cbz     w9, scene_take_free.none
    sub     w9, w9, #1
    SCENE_PTR x3, x9
    MOV64   x12, SC_NEXT
    ldr     w3, [x3, x12]
    add     w3, w3, #1
    str     w3, [x2]
    ret
scene_take_free.none:
    mov     w9, #NONE
    ret

// scene_append_recycled(x4=scene record, w9=handle, w2=duration,
// w3=SC_COUNT) -> C set (and Z set) when the frame went into the scene's
// reused frame block: scene_append_frame's append, in place, for a record
// whose last life left a block with room. C clear (and Z clear): nothing
// done, x3 kept. The x86 CF result, literally: test it with b.cs (or
// b.eq), not b.lo. Preserves x9, x2, x4; clobbers x1, x0, x5.
scene_append_recycled:
    LDX     x5, rc_scene_blk
    cbz     x5, scene_append_recycled.no
    LDX     x1, scenes
    sub     x1, x4, x1
    lsr     x1, x1, #SCENE_SHIFT
    ldr     x5, [x5, x1, lsl #3]
    cbz     x5, scene_append_recycled.no
    lsr     x1, x5, #RC_CAP_SHIFT
    cmp     w3, w1
    b.hs    scene_append_recycled.no    // full: it moves to the region's end
    and     x5, x5, #PT_LOW48
    ldr     x1, [x4, #SC_FRAMES]
    cbnz    x1, scene_append_recycled.placed
    cbnz    w3, scene_append_recycled.no
    str     x5, [x4, #SC_FRAMES]
    mov     x1, x5
scene_append_recycled.placed:
    cmp     x1, x5
    b.ne    scene_append_recycled.no
    add     x0, x1, w3, uxtw #FRAME_SHIFT
    mov     w5, w9
    orr     x5, x5, x2, lsl #32
    str     x5, [x0]                    // FR_HANDLE, FR_DURATION
    MOV64   x12, SC_EASE_TOTAL
    ldr     w13, [x4, x12]
    add     w13, w13, w2
    str     w13, [x4, x12]
    add     w1, w3, #1
    str     w1, [x4, #SC_COUNT]
    ldr     w13, [x4, #SC_FLAGS]
    MOV64   x12, PT_SHAPE_SHARED
    bic     w13, w13, w12
    str     w13, [x4, #SC_FLAGS]
    ldr     w13, [x4, #SC_HEAD]
    cmp     w3, w13
    b.ne    scene_append_recycled.done
    str     w9, [x4, #SC_HEAD_HANDLE]
    str     w2, [x4, #SC_HEAD_DURATION]
    str     wzr, [x4, #SC_TICKS]
scene_append_recycled.done:
    cmp     xzr, xzr                    // C and Z set
    ret
scene_append_recycled.no:
    msr     nzcv, xzr                   // C and Z clear
    ret

// entry_take_free(w0=caller kind) -> w9 = a recycled, zeroed event entry,
// or NONE (event_register). Clobbers x3, x2.
entry_take_free:
    cmp     w0, #CALLER_WAYPOINT
    b.ne    entry_take_free.take
    mov     w9, #1
    STB     w9, rc_wp_events            // entries copy control pointers
entry_take_free.take:
    LDW     w9, rc_entry_free
    cbz     w9, entry_take_free.none
    sub     w9, w9, #1
    LDX     x3, event_entries
    add     x3, x3, x9, lsl #6
    ldr     w2, [x3, #EN_NEXT]
    add     w2, w2, #1
    STW     w2, rc_entry_free
    stp     xzr, xzr, [x3]
    stp     xzr, xzr, [x3, #16]
    stp     xzr, xzr, [x3, #32]
    stp     xzr, xzr, [x3, #48]
    ret
entry_take_free.none:
    mov     w9, #NONE
    ret

// action_take_free -> w9 = a recycled, zeroed event action, or NONE
// (event_register). Clobbers x3.
action_take_free:
    LDW     w9, rc_action_free
    cbz     w9, action_take_free.none
    sub     w9, w9, #1
    LDX     x3, event_actions
    add     x3, x3, x9, lsl #5
    ldr     w12, [x3, #AC_NEXT]
    add     w12, w12, #1
    STW     w12, rc_action_free
    stp     xzr, xzr, [x3]
    stp     xzr, xzr, [x3, #16]
    ret
action_take_free.none:
    mov     w9, #NONE
    ret

    TSTATE
    .balign 8
rc_path_priv:   .skip 8                 // u64 per path: its own segments, parked
rc_scene_blk:   .skip 8                 // u64 per scene: its last life's frames
rc_bez_free:    .skip 8                 // 64-byte control blocks, linked
rc_take_wps:    .skip 8                 // path_take_free -> path_take_restore
rc_take_segs:   .skip 8
rc_take_wp_cap: .skip 4
rc_take_seg_cap: .skip 4
// list heads: index + 1, 0 = empty; linked through the record's next field
rc_path_limbo:  .skip 4
rc_path_free:   .skip 4
rc_scene_limbo: .skip 4
rc_entry_limbo: .skip 4
rc_entry_free:  .skip 4
rc_action_free: .skip 4
rc_scene_free:  .skip 4 * SCENE_BANKS
rc_wp_events:   .skip 1                 // an event is keyed by a waypoint

    .section .rodata
STRING msg_pool_symbols, "ParticlePool requires at least one symbol."
STRING msg_pool_max, "max_size must be greater than or equal to initial_count."
