// engine/events.s - EventHandler (src/engine/events.rs) and dispatch
// (EngineCtx::handle_event / register_event, src/engine/ctx.rs).
//
// Each character owns a list of entries in registration order; an entry is
// one (event, caller) pair with its actions, also in registration order.
// Dispatch runs the actions inline and reentrantly at the emission point, and
// re-reads each action's successor after running it, so actions appended
// while dispatching still run - Python list iteration, faithfully.
//
// Callers are keyed by name for scenes and paths (Scene/Path equality is by
// id upstream) and by the full waypoint record for waypoints (name,
// coordinate and bezier controls; equal records on different paths collide).
//
// ENTRY_LIMIT and ACTION_LIMIT are in defs.inc.

    .text

events_init:
    PUSH1   x30
    mov     x0, #(ENTRY_LIMIT * ENTRY_SIZE)
    bl      reserve
    STX     x9, event_entries
    mov     x0, #(ACTION_LIMIT * ACTION_SIZE)
    bl      reserve
    STX     x9, event_actions
    POP1    x30
    ret

// entry_matches(x4=entry record, w1=event, w2=caller kind, x3=caller)
// -> Z set on a match. Clobbers x9, x5, x10, x11, x16, x17.
entry_matches:
    ldrb    w9, [x4, #EN_EVENT]
    cmp     w9, w1
    b.ne    entry_matches.done
    ldrb    w9, [x4, #EN_KIND]
    cmp     w9, w2
    b.ne    entry_matches.done
    cmp     w2, #CALLER_WAYPOINT
    b.eq    entry_matches.waypoint
    ldr     w9, [x4, #(EN_WAYPOINT + WP_NAME)]
    cmp     w9, w3
    ret
entry_matches.waypoint:
    // name, coordinate, then the bezier controls element by element
    ldr     w9, [x3, #WP_NAME]
    ldr     w10, [x4, #(EN_WAYPOINT + WP_NAME)]
    cmp     w10, w9
    b.ne    entry_matches.done
    ldr     x9, [x3, #WP_COORD]
    ldr     x10, [x4, #(EN_WAYPOINT + WP_COORD)]
    cmp     x10, x9
    b.ne    entry_matches.done
    ldr     w9, [x3, #WP_BEZ_COUNT]
    ldr     w10, [x4, #(EN_WAYPOINT + WP_BEZ_COUNT)]
    cmp     w10, w9
    b.ne    entry_matches.done
    ldr     x5, [x3, #WP_BEZ]
    ldr     x10, [x4, #(EN_WAYPOINT + WP_BEZ)]
    mov     w11, #0
entry_matches.control:
    cmp     w11, w9
    b.hs    entry_matches.equal
    ldr     x16, [x5, x11, lsl #3]
    ldr     x17, [x10, x11, lsl #3]
    cmp     x17, x16
    b.ne    entry_matches.done
    add     w11, w11, #1
    b       entry_matches.control
entry_matches.equal:
    cmp     w9, w9                      // Z set
entry_matches.done:
    ret

// event_find(w0=slot, w1=event, w2=caller kind, x3=caller) -> w9 =
// entry index or NONE, and x4 = its record. Preserves x0-x3. Clobbers
// x4, x5, x10-x12, x16, x17.
event_find:
    LDX     x9, ch_events
    ldr     w9, [x9, w0, uxtw #2]
    cmp     w2, #CALLER_WAYPOINT
    b.eq    event_find.next
    // a name: the (event, kind) pair as one halfword, then the name
    orr     w5, w1, w2, lsl #8
    and     w5, w5, #0xffff
    LDX     x10, event_entries
event_find.named:
    cmn     w9, #1                      // NONE
    b.eq    event_find.done
    add     x4, x10, x9, lsl #6         // ENTRY_SIZE
    ldrh    w11, [x4, #EN_EVENT]
    cmp     w11, w5
    b.ne    event_find.named_next
    ldr     w11, [x4, #(EN_WAYPOINT + WP_NAME)]
    cmp     w11, w3
    b.eq    event_find.done
event_find.named_next:
    ldr     w9, [x4, #EN_NEXT]
    b       event_find.named
event_find.next:
    PUSH1   x30
event_find.walk:
    cmn     w9, #1                      // NONE
    b.eq    event_find.walked
    LDX     x4, event_entries
    add     x4, x4, x9, lsl #6          // ENTRY_SIZE
    mov     x12, x9
    bl      entry_matches
    mov     x9, x12
    b.eq    event_find.walked
    ldr     w9, [x4, #EN_NEXT]
    b       event_find.walk
event_find.walked:
    POP1    x30
event_find.done:
    ret

// event_register(w0=slot, w1=event, w2=caller kind, x3=caller name or
//   waypoint record, w4=ACT_* kind, x5=arg0, x6=arg1).
// EventHandler.register_event: an identical action on the same (event,
// caller) is a duplicate registration error.
event_register:
    sub     sp, sp, #80
    stp     x19, x20, [sp]
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    stp     x25, x30, [sp, #48]
    str     x6, [sp, #64]               // arg1
    mov     w19, w0
    mov     w20, w1
    mov     w21, w2
    mov     x22, x3
    mov     w23, w4
    mov     x24, x5
    bl      event_find
    cmn     w9, #1                      // NONE
    b.ne    event_register.have_entry
    // a new entry at the end of the character's list
    mov     w0, w21
    bl      entry_take_free             // particles.s: a recycled entry, or NONE
    cmn     w9, #1
    b.ne    event_register.new_entry
    LDW     w9, entry_count
    mov     w8, #ENTRY_LIMIT
    cmp     w9, w8
    b.hs    event_register.full
    add     w8, w9, #1
    STW     w8, entry_count
event_register.new_entry:
    mov     w9, w9
    LDX     x4, event_entries
    add     x4, x4, x9, lsl #6
    mov     w8, #NONE
    str     w8, [x4, #EN_NEXT]
    strb    w20, [x4, #EN_EVENT]
    strb    w21, [x4, #EN_KIND]
    str     w8, [x4, #EN_FIRST]
    str     w8, [x4, #EN_LAST]
    cmp     w21, #CALLER_WAYPOINT
    b.eq    event_register.copy_waypoint
    str     w22, [x4, #(EN_WAYPOINT + WP_NAME)]
    b       event_register.link
event_register.copy_waypoint:
    ldp     x2, x3, [x22]
    ldp     x10, x11, [x22, #16]
    stp     x2, x3, [x4, #EN_WAYPOINT]
    stp     x10, x11, [x4, #(EN_WAYPOINT + 16)]
event_register.link:
    LDX     x3, ch_events
    add     x3, x3, x19, lsl #2
event_register.tail:
    ldr     w2, [x3]
    cmn     w2, #1                      // NONE
    b.eq    event_register.linked
    LDX     x8, event_entries
    add     x2, x8, x2, lsl #6
    add     x3, x2, #EN_NEXT
    b       event_register.tail
event_register.linked:
    str     w9, [x3]
event_register.have_entry:
    LDX     x25, event_entries
    add     x25, x25, x9, lsl #6        // the entry record
    // reject a duplicate action
    ldr     w9, [x25, #EN_FIRST]
    ldr     x2, [sp, #64]               // arg1
event_register.dup:
    cmn     w9, #1                      // NONE
    b.eq    event_register.append
    LDX     x3, event_actions
    add     x3, x3, x9, lsl #5          // ACTION_SIZE
    ldr     w8, [x3, #AC_KIND]
    cmp     w8, w23
    b.ne    event_register.dup_next
    ldr     x8, [x3, #AC_ARG0]
    cmp     x8, x24
    b.ne    event_register.dup_next
    ldr     x8, [x3, #AC_ARG1]
    cmp     x8, x2
    b.eq    event_register.duplicate
event_register.dup_next:
    ldr     w9, [x3, #AC_NEXT]
    b       event_register.dup
event_register.append:
    bl      action_take_free            // particles.s: a recycled action, or NONE
    cmn     w9, #1
    b.ne    event_register.new_action
    LDW     w9, action_count
    mov     w8, #ACTION_LIMIT
    cmp     w9, w8
    b.hs    event_register.full
    add     w8, w9, #1
    STW     w8, action_count
event_register.new_action:
    mov     w9, w9
    LDX     x3, event_actions
    add     x3, x3, x9, lsl #5
    str     w23, [x3, #AC_KIND]
    mov     w8, #NONE
    str     w8, [x3, #AC_NEXT]
    str     x24, [x3, #AC_ARG0]
    ldr     x2, [sp, #64]               // arg1
    str     x2, [x3, #AC_ARG1]
    ldr     w2, [x25, #EN_LAST]
    str     w9, [x25, #EN_LAST]
    cmn     w2, #1                      // NONE
    b.eq    event_register.first
    LDX     x8, event_actions
    add     x2, x8, x2, lsl #5
    str     w9, [x2, #AC_NEXT]
    b       event_register.subscribed
event_register.first:
    str     w9, [x25, #EN_FIRST]
event_register.subscribed:
    // a character that observes segments walks its own segment lists
    cmp     w20, #EV_SEGMENT_ENTERED
    b.eq    event_register.own_segments
    cmp     w20, #EV_SEGMENT_EXITED
    b.ne    event_register.mark
event_register.own_segments:
    mov     w0, w19
    bl      path_unshare_all            // motion.s
event_register.mark:
    LDX     x9, ch_subs
    ldrb    w8, [x9, x19]
    mov     w3, #1
    lsl     w3, w3, w20
    orr     w8, w8, w3
    strb    w8, [x9, x19]
    ldp     x19, x20, [sp]
    ldp     x21, x22, [sp, #16]
    ldp     x23, x24, [sp, #32]
    ldp     x25, x30, [sp, #48]
    add     sp, sp, #80
    ret
event_register.duplicate:
    ADRG    x0, msg_duplicate_event
    mov     w1, #msg_duplicate_event_len
    b       fatal
event_register.full:
    ADRG    x0, msg_events_full
    mov     w1, #msg_events_full_len
    b       fatal

// handle_event(w0=slot, w1=event, w2=caller kind, x3=caller): run every
// action registered for (event, caller) on the character, in order, inline.
// Clobbers the C caller-saved set.
handle_event:
    LDX     x9, ch_subs
    ldrb    w9, [x9, w0, uxtw]
    lsr     w9, w9, w1
    tbz     w9, #0, handle_event.none
    sub     sp, sp, #32
    stp     x19, x21, [sp]
    stp     x22, x30, [sp, #16]
    mov     w19, w0                     // slot
    bl      event_find
    cmn     w9, #1                      // NONE
    b.eq    handle_event.done
    mov     x21, x4                     // entry
    ldr     w22, [x4, #EN_FIRST]
handle_event.action:
    cmn     w22, #1                     // NONE
    b.eq    handle_event.done
    LDX     x9, event_actions
    add     x9, x9, x22, lsl #5
    ldr     w3, [x9, #AC_KIND]
    ldr     x1, [x9, #AC_ARG0]
    ldr     x2, [x9, #AC_ARG1]
    mov     w0, w19
    bl      run_action
    // the successor is read after the action ran: appended actions run too
    LDX     x9, event_actions
    add     x9, x9, x22, lsl #5
    ldr     w22, [x9, #AC_NEXT]
    b       handle_event.action
handle_event.done:
    ldp     x22, x30, [sp, #16]
    ldp     x19, x21, [sp]
    add     sp, sp, #32
handle_event.none:
    ret

// run_action(w0=slot, w3=ACT_* kind, x1=arg0, x2=arg1): tail-calls the
// action with the caller's x30, so it returns to run_action's caller.
run_action:
    // a callback, or an action on a character other than the one update is
    // ticking, may change any path: update's precomputed steps are void
    cmp     w3, #ACT_CALLBACK
    b.eq    run_action.epoch
    LDW     w9, upd_cursor
    cmp     w0, w9
    b.eq    run_action.dispatch
run_action.epoch:
    LDW     w9, motion_epoch
    add     w9, w9, #1
    STW     w9, motion_epoch
    // (TIER 3+ also calls motion_void here: no mirrored steps at tier 1)
run_action.dispatch:
    cmp     w3, #ACT_ACTIVATE_PATH
    b.ne    1f
    b       path_activate_name
1:  cmp     w3, #ACT_ACTIVATE_SCENE
    b.ne    1f
    b       scene_activate_name
1:  cmp     w3, #ACT_DEACTIVATE_PATH
    b.ne    1f
    b       path_deactivate
1:  cmp     w3, #ACT_DEACTIVATE_SCENE
    b.ne    1f
    b       scene_deactivate
1:  cmp     w3, #ACT_RESET_APPEARANCE
    b.ne    1f
    b       reset_appearance
1:  cmp     w3, #ACT_SET_LAYER
    b.ne    1f
    b       set_layer
1:  cmp     w3, #ACT_SET_COORDINATE
    b.ne    1f
    b       set_coordinate
1:  cmp     w3, #ACT_CALLBACK
    b.ne    run_action.unknown
    mov     x9, x1
    mov     x1, x2
    br      x9                          // fn(w0=slot, x1=payload)
run_action.unknown:
    ADRG    x0, msg_unknown_action
    mov     w1, #msg_unknown_action_len
    b       fatal

// event_clear(w0=slot): EventHandler.clear (particle resets). Clobbers x16.
event_clear:
    LDX     x9, ch_events
    mov     w16, #NONE
    str     w16, [x9, w0, uxtw #2]
    LDX     x9, ch_subs
    strb    wzr, [x9, w0, uxtw]
    ret

    .section .rodata
STRING msg_duplicate_event, "ttfx: asm engine: duplicate event registration\n"
STRING msg_events_full, "ttfx: asm engine: event limit reached\n"
STRING msg_unknown_action, "ttfx: asm engine: unknown event action\n"

    TSTATE
    .balign 8
event_entries:  .skip 8
event_actions:  .skip 8
entry_count:    .skip 4
action_count:   .skip 4
