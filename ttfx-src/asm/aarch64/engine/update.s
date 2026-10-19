// engine/update.s - the active set, EffectCharacter.tick and
// BaseEffectIterator.update (src/engine/ctx.rs, active_characters.rs).
//
// The active set is a bitmap over slots, so iterating it visits characters in
// ascending slot order - the canonical order the parity rules demand. update
// ticks a snapshot and then prunes. Only candidates can have left the set:
// characters inserted since the last prune and characters whose scene or
// path ended or was deactivated (every such engine path does MARK_CANDIDATE).
// Pruning those equals Rust's retain over the whole set. The passes only
// visit the window of bitmap words [active_lo, active_hi), which
// active_insert widens and each prune narrows: most effects keep a band of
// active characters, not the whole store.
//
// Dozing. Most ticks only count down: no path, the visual unchanged, no
// event - a plain scene's head frame ticking (and retiring, but for the
// last frame), or an eased scene's steps that keep showing the same frame.
// When step_animation runs as update's tick of a character ([doze_slot]),
// it counts the pure ticks ahead (k) and advances the scene's counter
// (SC_TICKS or SC_EASE_STEP) past them at once; doze_try then sets the
// character's bit in doze_bits, which keeps it out of the snapshots, and
// ch_wake[slot], the update number (mod 256) whose snapshot takes it back.
// So a dozing character costs nothing per update. A plain doze may end on
// its head frame's last tick; that retirement happens when it wakes
// (doze_retire).
//
// Anything that could make one of those ticks impure, or that reads or
// rewrites the active scene's playback state, first calls doze_wake: scene
// activation and deactivation, step_animation, set_appearance, scene_reset,
// scene_copy, scene_new over a used record, particle resets, active_remove,
// active_clear, path_activate (motion.s) and thunderstorm's easing
// change. doze_wake gives back the ticks not yet due, and a character woken
// during update ahead of the ticking slot rejoins this update's snapshot,
// so it ticks exactly where Rust ticks it.
//
// The x86 engine's batched motion (motion_batch, TIER 3+) is not ported:
// every tick goes through motion_move, as at its TIER 1.
//
// UPD_STAGGER (bytes between the bitmaps' page offsets) is in defs.inc.

    .text

.equ UPD_BITMAP, CHAR_LIMIT / 8 + UPD_STAGGER
.equ UPD_REGION, 4 * (CHAR_LIMIT / 8) + CHAR_LIMIT + 5 * UPD_STAGGER

// update_init: once per run. chars_init already runs it, since input parsing
// (terminal_init) can set appearances, which wake dozers.
update_init:
    LDX     x9, active_bits
    cbnz    x9, update_init.done
    // the arrays are staggered within a page: the same word of two of them
    // would share its address's low 12 bits, and a load then waits on a
    // store to the other (4K aliasing)
    // only short prefixes of these widely separated arrays are normally
    // touched: keep their pages small, like the character fields'
    PUSH1   x30
    MOV64   x0, UPD_REGION
    bl      reserve_small
    POP1    x30
    STX     x9, active_bits
    MOV64   x2, UPD_BITMAP
    add     x9, x9, x2
    STX     x9, snapshot_bits
    add     x9, x9, x2
    STX     x9, candidate_bits
    add     x9, x9, x2
    STX     x9, doze_bits
    add     x9, x9, x2
    STX     x9, ch_wake
    mov     w2, #-1
    STW     w2, upd_cursor
    STW     w2, doze_slot
update_init.done:
    ret

// BIT_POP dest, word: dest = index of word's lowest set bit (word != 0),
// which is then cleared. Clobbers x16.
.macro BIT_POP dest, word
    rbit    \dest, \word
    clz     \dest, \dest
    sub     x16, \word, #1
    and     \word, \word, x16
.endm

// doze_wake(w0=slot): end slot's doze, if any, settling its scene's
// ticks_elapsed. Clobbers x9 only (and x16, x17).
doze_wake:
    LDX     x9, doze_bits
    ubfx    x16, x0, #6, #26
    ldr     x9, [x9, x16, lsl #3]
    lsr     x9, x9, x0
    tbnz    x9, #0, doze_wake.wake
    ret
doze_wake.wake:
    stp     x0, x30, [sp, #-48]!
    stp     x1, x2, [sp, #16]
    stp     x3, x4, [sp, #32]
    ubfx    x3, x0, #6, #26             // the word
    mov     x1, #1
    lsl     x1, x1, x0                  // the bit
    LDX     x2, doze_bits
    ldr     x9, [x2, x3, lsl #3]
    bic     x9, x9, x1
    str     x9, [x2, x3, lsl #3]
    // ticks still owed: wake - update when this update's tick is still
    // ahead (it rejoins the snapshot), one less when it is behind us or no
    // update is running
    LDX     x9, ch_wake
    ldrb    w2, [x9, w0, uxtw]
    LDW     w9, upd_count
    sub     w2, w2, w9
    and     w2, w2, #0xff
    LDW     w9, upd_cursor
    cmp     w0, w9
    b.hs    doze_wake.ahead
    sub     w2, w2, #1
    b       doze_wake.settle
doze_wake.ahead:
    LDX     x9, snapshot_bits
    ldr     x4, [x9, x3, lsl #3]
    orr     x4, x4, x1
    str     x4, [x9, x3, lsl #3]
doze_wake.settle:
    LDX     x9, ch_scene
    ldr     w9, [x9, w0, uxtw #2]
    SCENE_PTR x4, x9
    ldr     w9, [x4, #SC_FLAGS]
    tst     w9, #SCF_EASED
    b.ne    doze_wake.eased
    ldr     w9, [x4, #SC_TICKS]
    sub     w9, w9, w2
    str     w9, [x4, #SC_TICKS]
    bl      doze_retire
    b       doze_wake.restore
doze_wake.eased:
    MOV64   x3, SC_EASE_STEP
    ldr     w9, [x4, x3]
    sub     w9, w9, w2
    str     w9, [x4, x3]
doze_wake.restore:
    ldp     x3, x4, [sp, #32]
    ldp     x1, x2, [sp, #16]
    ldp     x0, x30, [sp], #48
    ret

// doze_try(w0=slot, w1=k > 0 pure ticks ahead) -> w1 = the ticks it
// dozes through (k, at most 254 so the wake byte stays unambiguous), or 0
// when it can't (a path, or no longer in the set). step_animation calls it
// on update's tick ([doze_slot]) and advances its counters by w1. Clobbers
// x9, x3, x2 (and x16).
doze_try:
    mov     w9, #254
    cmp     w1, w9
    csel    w1, w9, w1, hi
    LDX     x9, ch_path
    ldr     w9, [x9, w0, uxtw #2]
    cmn     w9, #1
    b.ne    doze_try.refuse
    LDX     x9, active_bits
    ubfx    x3, x0, #6, #26
    ldr     x2, [x9, x3, lsl #3]
    lsr     x2, x2, x0
    tbz     x2, #0, doze_try.refuse
    LDX     x9, doze_bits
    ldr     x2, [x9, x3, lsl #3]
    mov     x16, #1
    lsl     x16, x16, x0
    orr     x2, x2, x16
    str     x2, [x9, x3, lsl #3]
    LDW     w9, upd_count
    add     w9, w9, w1
    add     w9, w9, #1
    LDX     x2, ch_wake
    strb    w9, [x2, w0, uxtw]
    ret
doze_try.refuse:
    mov     w1, #0
    ret

// doze_retire(x4=scene record): a doze may end just after its head frame's
// last tick; that tick's retirement (never the scene's last frame) happens
// here. Clobbers x9, x3.
doze_retire:
    ldr     w9, [x4, #SC_TICKS]
    ldr     w3, [x4, #SC_HEAD_DURATION]
    cmp     w9, w3
    b.ne    doze_retire.done
    str     wzr, [x4, #SC_TICKS]
    ldr     w9, [x4, #SC_HEAD]
    add     w9, w9, #1
    str     w9, [x4, #SC_HEAD]
    b       scene_load_head
doze_retire.done:
    ret

// ACTIVE_WORDS xreg: xreg = bitmap words in use (covers every allocated
// slot).
.macro ACTIVE_WORDS xreg, wreg
    LDW     \wreg, char_count
    add     \xreg, \xreg, #63
    lsr     \xreg, \xreg, #6
.endm

// active_insert(w0=slot). Clobbers x9, x3, x2 (and x16).
active_insert:
    LDX     x9, active_bits
    ubfx    x3, x0, #6, #26
    mov     x16, #1
    lsl     x16, x16, x0
    ldr     x2, [x9, x3, lsl #3]
    orr     x2, x2, x16
    str     x2, [x9, x3, lsl #3]
    LDX     x9, candidate_bits
    ldr     x2, [x9, x3, lsl #3]
    orr     x2, x2, x16
    str     x2, [x9, x3, lsl #3]
    // widen the window of words that can hold active characters
    LDW     w9, active_hi
    LDW     w2, active_lo
    cmp     w9, w2
    b.ls    active_insert.first
    cmp     w3, w2
    b.hs    active_insert.above
    STW     w3, active_lo
active_insert.above:
    add     w3, w3, #1
    cmp     w3, w9
    b.ls    active_insert.done
    STW     w3, active_hi
active_insert.done:
    ret
active_insert.first:
    STW     w3, active_lo
    add     w3, w3, #1
    STW     w3, active_hi
    ret

// active_remove(w0=slot). Clobbers x9, x3, x2 (and x16).
active_remove:
    PUSH1   x30
    bl      doze_wake
    POP1    x30
    LDX     x9, active_bits
    ubfx    x3, x0, #6, #26
    mov     x2, #1
    lsl     x2, x2, x0
    ldr     x16, [x9, x3, lsl #3]
    bic     x16, x16, x2
    str     x16, [x9, x3, lsl #3]
    ret

// active_contains(w0=slot) -> w9. Clobbers x3 (and x16).
active_contains:
    LDX     x9, active_bits
    ubfx    x3, x0, #6, #26
    ldr     x9, [x9, x3, lsl #3]
    lsr     x9, x9, x0
    and     w9, w9, #1
    ret

// active_clear: empty the set, waking every dozing character first.
active_clear:
    stp     x19, x21, [sp, #-32]!
    stp     x22, x30, [sp, #16]
    ACTIVE_WORDS x22, w22
    mov     x19, #0
active_clear.word:
    cmp     x19, x22
    b.hs    active_clear.clear
    LDX     x9, doze_bits
    ldr     x21, [x9, x19, lsl #3]
active_clear.bit:
    cbz     x21, active_clear.next
    BIT_POP x0, x21
    add     x0, x0, x19, lsl #6
    bl      doze_wake
    b       active_clear.bit
active_clear.next:
    add     x19, x19, #1
    b       active_clear.word
active_clear.clear:
    LDX     x0, active_bits
    mov     x3, x22
    mov     x9, #0
    REP_STOSQ
    STW     wzr, active_lo
    STW     wzr, active_hi
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp], #32
    ret

// active_empty -> w9 = 1 when no character is active. Clobbers x3, x2.
active_empty:
    LDX     x9, active_bits
    ACTIVE_WORDS x3, w3
    mov     x2, #0
active_empty.loop:
    cmp     x2, x3
    b.hs    active_empty.empty
    ldr     x16, [x9, x2, lsl #3]
    cbnz    x16, active_empty.not_empty
    add     x2, x2, #1
    b       active_empty.loop
active_empty.empty:
    mov     w9, #1
    ret
active_empty.not_empty:
    mov     w9, #0
    ret

// active_count -> x9 = number of active characters. Clobbers x1, x3, x2,
// x4, x5 (and x16).
active_count:
    LDX     x1, active_bits
    ACTIVE_WORDS x3, w3
    mov     x9, #0
    mov     x2, #0
    mov     x5, #0x0101010101010101
active_count.loop:
    cmp     x2, x3
    b.hs    active_count.done
    // popcount, in general registers
    ldr     x4, [x1, x2, lsl #3]
    lsr     x16, x4, #1
    and     x16, x16, #0x5555555555555555
    sub     x4, x4, x16
    and     x16, x4, #0x3333333333333333
    lsr     x4, x4, #2
    and     x4, x4, #0x3333333333333333
    add     x4, x4, x16
    add     x4, x4, x4, lsr #4
    and     x4, x4, #0x0f0f0f0f0f0f0f0f
    mul     x4, x4, x5
    add     x9, x9, x4, lsr #56
    add     x2, x2, #1
    b       active_count.loop
active_count.done:
    ret

// is_active(w0=slot) -> w9: EffectCharacter.is_active - an active path, or
// an active scene that is not complete (looping scenes read as complete).
is_active:
    LDX     x9, ch_path
    ldr     w9, [x9, w0, uxtw #2]
    cmn     w9, #1
    b.ne    is_active.yes
    PUSH1   x30
    bl      scene_is_complete
    POP1    x30
    eor     w9, w9, #1
    ret
is_active.yes:
    mov     w9, #1
    ret

// tick(w0=slot): EffectCharacter.tick - motion first, then animation.
// A character without an active path has nothing to move.
tick:
    LDX     x9, ch_path
    ldr     w9, [x9, w0, uxtw #2]
    cmn     w9, #1
    b.ne    tick.moving
    b       step_animation
tick.moving:
    PUSH2   x19, x30
    mov     w19, w0
    bl      motion_move
    mov     w0, w19
    bl      step_animation
    POP2    x19, x30
    ret

// tick_awake(w0=slot): tick for update's pass, where the character is not
// dozing (nothing but update's own tick without a path starts a doze).
tick_awake:
    LDX     x9, ch_path
    ldr     w9, [x9, w0, uxtw #2]
    cmn     w9, #1
    b.ne    tick_awake.moving
    b       step_animation_awake
tick_awake.moving:
    PUSH2   x19, x30
    mov     w19, w0
    bl      motion_move
    mov     w0, w19
    bl      step_animation_awake
    POP2    x19, x30
    ret

// WAKE_MASK dest: dest = bit i set where byte i of the 64 at x1 equals the
// byte broadcast in v7, with v6 = upd_bit_weights. Clobbers v0-v3.
.macro WAKE_MASK dest
    ld1     {v0.16b, v1.16b, v2.16b, v3.16b}, [x1]
    cmeq    v0.16b, v0.16b, v7.16b
    cmeq    v1.16b, v1.16b, v7.16b
    cmeq    v2.16b, v2.16b, v7.16b
    cmeq    v3.16b, v3.16b, v7.16b
    and     v0.16b, v0.16b, v6.16b
    and     v1.16b, v1.16b, v6.16b
    and     v2.16b, v2.16b, v6.16b
    and     v3.16b, v3.16b, v6.16b
    addp    v0.16b, v0.16b, v1.16b      // pairs: bytes 0-7 from v0, 8-15 from v1
    addp    v2.16b, v2.16b, v3.16b
    addp    v0.16b, v0.16b, v2.16b      // quads: v0, v1, v2, v3
    addp    v0.16b, v0.16b, v0.16b      // byte k = bits 8k..8k+7
    fmov    \dest, d0
.endm

// update: tick a snapshot of the active set in ascending order, then prune.
update:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]              // [sp, #56]: the prune slot
    LDX     x21, active_bits
    LDX     x22, snapshot_bits
    // only the window of words that can hold active characters
    LDW     w23, active_hi
    LDW     w20, active_lo
    // the snapshot (callbacks may change the live set while we tick): this
    // update's dozers wake, the others stay out
    LDW     w9, upd_count
    add     w9, w9, #1
    STW     w9, upd_count
    dup     v7.16b, w9
    ADRG    x9, upd_bit_weights
    ldr     q6, [x9]
    LDX     x4, doze_bits
    LDX     x1, ch_wake
    add     x1, x1, x20, lsl #6
    mov     w19, w20
update.snap_word:
    cmp     x19, x23
    b.hs    update.snapped
    ldr     x9, [x21, x19, lsl #3]
    ldr     x5, [x4, x19, lsl #3]
    cbz     x5, update.snap_store
    // those waking now join the snapshot; their doze ends at their tick
    WAKE_MASK x10
    and     x10, x10, x5
    eor     x5, x5, x10
    bic     x9, x9, x5
update.snap_store:
    str     x9, [x22, x19, lsl #3]
    add     x1, x1, #64
    add     x19, x19, #1
    b       update.snap_word
update.snapped:
    mov     w19, w20
update.tick_word:
    cmp     x19, x23
    b.hs    update.prune
    ldr     x24, [x22, x19, lsl #3]
    cbz     x24, update.tick_next
    lsl     x20, x19, #6
update.tick_bit:
    BIT_POP x0, x24
    str     x24, [x22, x19, lsl #3]
    add     x0, x0, x20
    STW     w0, upd_cursor
    LDX     x2, doze_bits
    ldr     x9, [x2, x19, lsl #3]
    lsr     x16, x9, x0
    tbnz    x16, #0, update.waking
update.tick:
    // without a path, the tick is step_animation, which may start a doze
    LDX     x3, ch_path
    ldr     w3, [x3, w0, uxtw #2]
    cmn     w3, #1
    b.ne    update.moving
    STW     w0, doze_slot
    bl      step_animation_awake
    mov     w9, #-1
    STW     w9, doze_slot
    b       update.ticked
update.moving:
    // tick_awake, inline
    bl      motion_move
    LDW     w0, upd_cursor
    bl      step_animation_awake
update.ticked:
    // re-read: a character woken during the pass may have joined this word
    // (the x86 engine's motion_epoch check here only voids TIER 3+
    // motion_batch results)
    ldr     x24, [x22, x19, lsl #3]
    cbnz    x24, update.tick_bit
    add     x19, x19, #1
    b       update.tick_word
update.waking:
    // its doze ran out: settle a pending retirement, then tick as usual
    mov     x16, #1
    lsl     x16, x16, x0
    bic     x9, x9, x16
    str     x9, [x2, x19, lsl #3]
    LDX     x9, ch_scene
    ldr     w9, [x9, w0, uxtw #2]
    SCENE_PTR x4, x9
    bl      doze_retire
    LDW     w0, upd_cursor
    b       update.tick
update.tick_next:
    add     x19, x19, #1
    b       update.tick_word
update.prune:
    mov     w9, #-1
    STW     w9, upd_cursor
    // the set may have grown during the pass (new characters); candidates
    // outside the window are not active
    LDW     w23, active_hi
    LDX     x22, candidate_bits
    LDW     w19, active_lo
update.prune_word:
    cmp     x19, x23
    b.hs    update.shrink
    ldr     x24, [x22, x19, lsl #3]
    cbz     x24, update.prune_next
    str     xzr, [x22, x19, lsl #3]
    lsl     x20, x19, #6
update.prune_bit:
    cbz     x24, update.prune_next
    BIT_POP x0, x24
    add     x0, x0, x20
    str     w0, [sp, #56]
    bl      is_active
    ldr     w0, [sp, #56]
    cbnz    w9, update.prune_bit
    ubfx    x3, x0, #6, #26
    mov     x16, #1
    lsl     x16, x16, x0
    ldr     x2, [x21, x3, lsl #3]
    bic     x2, x2, x16
    str     x2, [x21, x3, lsl #3]
    b       update.prune_bit
update.prune_next:
    add     x19, x19, #1
    b       update.prune_word
update.shrink:
    // narrow the window to the words still in use
    LDW     w9, active_lo
update.shrink_lo:
    cmp     w9, w23
    b.hs    update.empty
    ldr     x2, [x21, w9, uxtw #3]
    cbnz    x2, update.shrink_hi
    add     w9, w9, #1
    b       update.shrink_lo
update.shrink_hi:
    sub     w3, w23, #1
    ldr     x2, [x21, w3, uxtw #3]
    cbnz    x2, update.narrowed
    sub     w23, w23, #1
    b       update.shrink_hi
update.empty:
    mov     w9, #0
    mov     w23, #0
update.narrowed:
    STW     w9, active_lo
    STW     w23, active_hi
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

    .section .rodata
    .balign 16
upd_bit_weights:                        // WAKE_MASK's pmovmskb
    .byte   1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128

    TSTATE
    .balign 8
active_bits:    .skip 8
snapshot_bits:  .skip 8
candidate_bits: .skip 8
doze_bits:      .skip 8             // characters dozing through pure ticks
ch_wake:        .skip 8             // u8 per slot: the update that wakes it
active_lo:      .skip 4             // words [lo, hi) hold every active
active_hi:      .skip 4             // character (empty when hi <= lo)
upd_count:      .skip 4             // updates started
upd_cursor:     .skip 4             // the slot ticking now, or -1
doze_slot:      .skip 4             // the slot whose update tick is running, or -1
