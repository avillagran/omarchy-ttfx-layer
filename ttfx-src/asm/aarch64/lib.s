// lib.s - the ttfx assembly engine, linked into the Rust binary
// (plans/asm-x86.md; the aarch64 port, asm/aarch64/PORTING.md). Rust parses
// the command line, reads and validates the input, seeds the RNG and
// installs signal handlers; it then offers the run to ttfx_asm_run, which
// either declines before doing anything observable or runs the effect to
// completion and reports how it ended.
//
// One translation unit: every component is included here. Entry points follow
// AAPCS64; everything internal uses the conventions in ttfx.inc. build.rs
// assembles it once (aarch64 has one tier); the entry points are exported
// with a _v1 suffix (EXPORT), and ttfx_asm_tier reports tier 1.

.include "ttfx.inc"

// Per-run state lives in one section that ttfx_asm_run zeroes on entry, so a
// rerun after a terminal resize starts clean.
    TSTATE
    .balign 64
tstate_begin:

// The frame ring's atomics, as in engine/render.s (which skips its copy).
.ifndef RENDER_ATOMICS
.equ RENDER_ATOMICS, 1
.macro R_XCHGB wold, xaddr, wnew
.Lrx\@:
    ldaxrb  \wold, [\xaddr]
    stlxrb  w17, \wnew, [\xaddr]
    cbnz    w17, .Lrx\@
    dmb     ish
.endm
.macro R_ATOMIC_INC xaddr, wtmp
.Lri\@:
    ldaxr   \wtmp, [\xaddr]
    add     \wtmp, \wtmp, #1
    stlxr   w17, \wtmp, [\xaddr]
    cbnz    w17, .Lri\@
.endm
.endif

    .text

// ttfx_asm_tier() -> w0 = 1: aarch64 has the one tier (src/asm/ffi.rs calls
// it unsuffixed; asm/tier.asm's CPU detection has no counterpart here).
    .global ttfx_asm_tier
    .type   ttfx_asm_tier, %function
ttfx_asm_tier:
    mov     w0, #1
    ret

// ttfx_asm_effect_supported(x0=effect id) -> w0 = 1 when this build has it.
EXPORT ttfx_asm_effect_supported
ttfx_asm_effect_supported:
    CENTRY
    mov     x9, #0
    cmp     x0, #EFFECT_COUNT
    b.hs    ttfx_asm_effect_supported.done
    ADRG    x9, effect_table
    lsl     x0, x0, #4
    ldr     x9, [x9, x0]
    cmp     x9, #0
    cset    w9, ne
ttfx_asm_effect_supported.done:
    mov     x0, x9
    CEXIT

// ttfx_asm_run(x0=request) -> x0 = outcome (OUT_*, or -errno).
EXPORT ttfx_asm_run
ttfx_asm_run:
    CENTRY
    // a fresh run: release the previous run's memory, zero the state
    mov     x19, x0
    bl      release_regions
    ADRG    x0, tstate_begin
    ADRG    x3, tstate_end
    sub     x3, x3, x0
    mov     w9, #0
    REP_STOSB
    STX     x19, request
    // engine_fail unwinds to here: the CENTRY frame is at sp
    mov     x9, sp
    STX     x9, fail_rsp
    // the effect must be one this build has
    ldr     x9, [x19, #RQ_EFFECT]
    cmp     x9, #EFFECT_COUNT
    b.hs    ttfx_asm_run.declined
    ADRG    x3, effect_table
    add     x3, x3, x9, lsl #4
    ldr     x2, [x3]
    cbz     x2, ttfx_asm_run.declined
    STX     x2, effect_build
    ldr     x2, [x3, #8]
    STX     x2, effect_next_frame
    bl      load_request
    mov     x0, #(1 << 38)
    bl      reserve
    STX     x9, arena_ptr
    bl      visual_init
    bl      chars_init
    bl      terminal_init
    bl      monotonic_ns
    STX     x9, last_frame_ns           // Terminal::new's last_time_printed
    bl      scenes_init
    bl      paths_init
    bl      events_init
    bl      update_init
    bl      render_init
    bl      clock_init
    bl      pipeline_plan
    // Scenes intern their visuals during the effect's build (engine/scene.s's
    // scene_add_frame), so the audio transform must be live before it: build
    // sees this frame's parameters and the per-frame poll in next_frame keeps
    // later visuals current. Byte-identical when the callbacks report inactive.
    bl      audio_frame_poll
    LDX     x9, effect_build
    blr     x9
    LDX     x9, request
    ldr     x10, [x9, #RQ_DIRECT_EMIT]
    cbnz    x10, ttfx_asm_run.direct
    LDB     w9, cfg_parity_dump
    cbnz    w9, ttfx_asm_run.dump
    bl      run_effect
    b       run_return
ttfx_asm_run.direct:
    bl      run_direct_frames
    b       run_return
ttfx_asm_run.dump:
    bl      dump_effect
ttfx_asm_run.return:
run_return:
    mov     x19, x9
    bl      store_rng_state
    mov     x9, x19
ttfx_asm_run.epilogue:
    mov     x0, x9
    CEXIT
ttfx_asm_run.declined:
    mov     x9, #0
    b       ttfx_asm_run.epilogue

// engine_fail(x0=message, x1=length): the effect or engine hit an error
// that Rust reports as "Error: <message>". Unwinds to ttfx_asm_run.
engine_fail:
    LDX     x9, request
    str     x0, [x9, #RQ_ERROR_PTR]
    str     x1, [x9, #RQ_ERROR_LEN]
    mov     x2, #ERR_MESSAGE
    str     x2, [x9, #RQ_ERROR_KIND]
    LDX     x9, fail_rsp
    mov     sp, x9
    // the frames already handed to the render thread come first; if one of
    // them failed to write, the run ended there, before this error
    bl      pipeline_finish
    cbnz    x9, engine_fail.write_failed
    mov     x9, #OUT_ERROR
    b       run_return
engine_fail.write_failed:
    LDW     w3, r_submitted             // the main thread's open ring entry
    and     w3, w3, #(FRAME_RING - 1)
    lsl     w3, w3, #FS_SHIFT
    ADRG    x2, ring
    add     x2, x2, x3
    ldr     x22, [x2, #FS_PREFIX]
    bl      teardown_failed
    b       run_return

// fail_with_number(x0=message, w2=length, x1=number): FAIL with the
// message followed by the number in decimal ("... Received: -3").
fail_with_number:
    str     x1, [sp, #-16]!
    mov     x1, x0
    ADRG    x0, fail_buffer
    mov     w3, w2
    REP_MOVSB
    ldr     x1, [sp]
    str     x0, [sp]
    bl      format_i64
    ldr     x0, [sp], #16
    add     x0, x0, x9
    ADRG    x1, fail_buffer
    sub     x1, x0, x1
    ADRG    x0, fail_buffer
    b       engine_fail

// load_request: copy the request's settings into the cfg_* globals.
load_request:
    LDX     x1, request
    ldr     x9, [x1, #RQ_TAB_WIDTH]
    STX     x9, cfg_tab_width
    ldr     x9, [x1, #RQ_FRAME_RATE]
    STX     x9, cfg_frame_rate
    ldr     x9, [x1, #RQ_CANVAS_WIDTH]
    STX     x9, cfg_canvas_width
    ldr     x9, [x1, #RQ_CANVAS_HEIGHT]
    STX     x9, cfg_canvas_height
    ldr     x9, [x1, #RQ_ANCHOR_CANVAS]
    STX     x9, cfg_anchor_canvas
    ldr     x9, [x1, #RQ_ANCHOR_TEXT]
    STX     x9, cfg_anchor_text
    ldr     x9, [x1, #RQ_EXISTING_COLORS]
    STX     x9, cfg_existing_colors
    ldr     x9, [x1, #RQ_MAX_FRAMES]
    STX     x9, cfg_max_frames
    ldr     x9, [x1, #RQ_TERM_WIDTH]
    STX     x9, term_width
    ldr     x9, [x1, #RQ_TERM_HEIGHT]
    STX     x9, term_height
    ldr     x9, [x1, #RQ_INPUT_PTR]
    STX     x9, input_ptr
    ldr     x9, [x1, #RQ_INPUT_LEN]
    STX     x9, input_len
    ldr     x9, [x1, #RQ_EFFECT_CONFIG]
    STX     x9, effect_config
    ldr     x9, [x1, #RQ_FLAGS]
    ADRG    x0, cfg_flag_bytes
    mov     w3, #0
load_request.flag:
    lsr     x2, x9, x3
    and     w2, w2, #1
    strb    w2, [x0, x3]
    add     w3, w3, #1
    cmp     w3, #16
    b.lo    load_request.flag
    // the RNG continues from Rust's state
    add     x0, x1, #RQ_RNG_STATE
    b       rng_load

// store_rng_state: hand the RNG state back (a resize continues the stream).
store_rng_state:
    LDX     x0, request
    add     x0, x0, #RQ_RNG_STATE
    b       rng_store

// stop_requested -> w9 = STOP_*: asks Rust (interrupt, terminate, a settled
// resize on a tty), exactly at run_effect's requested_stop points.
stop_requested:
    str     x30, [sp, #-16]!
    LDX     x9, request
    ldr     x0, [x9, #RQ_STOP_CTX]
    ldr     x9, [x9, #RQ_STOP_CHECK]
    CCALLR  x9
    ldr     x30, [sp], #16
    ret

// audio_frame_poll: sample this frame's audio once (asm/ttfx.inc): the poll
// word (volume, bass, beat) into audio_word, and the color parameters
// (brightness + 3x3 hue matrix) into audio_params, with audio_color_on set
// from the callback's active flag. The default callbacks report silence and
// an inactive transform, so a run without them is byte-identical.
audio_frame_poll:
    str     x30, [sp, #-16]!
    LDX     x9, request
    ldr     x0, [x9, #RQ_AUDIO_CTX]
    ldr     x9, [x9, #RQ_AUDIO_POLL]
    CCALLR  x9
    // A remembered BPM is not live energy. The analyzer retains its tempo
    // estimate through silence: do not keep accelerating rain with that value
    // after volume, bass and treble have all returned to zero. Beat alone may
    // also be the tail of the analyzer's held flag. No callback/ABI change.
    movz    w10, #0xffff
    movk    w10, #0xff00, lsl #16       // volume, bass, treble (exclude beat)
    tst     w9, w10
    csel    x9, x9, xzr, ne
    STX     x9, audio_word
    LDX     x9, request
    ldr     x0, [x9, #RQ_AUDIO_CTX]
    ADRG    x1, audio_params
    ldr     x9, [x9, #RQ_AUDIO_PARAMS]
    CCALLR  x9
    STB     w9, audio_color_on
    ldr     x30, [sp], #16
    ret

// clock_init: Clock::real() / Clock::virtual_with_frame_rate().
clock_init:
    str     x30, [sp, #-16]!
    LDB     w9, cfg_parity_dump
    cbnz    w9, clock_init.virtual
    LDB     w9, cfg_virtual_clock
    cbnz    w9, clock_init.virtual
    STB     wzr, clock_is_virtual
    bl      realtime_s
    STD     d0, clock_wall_start
    bl      monotonic_ns
    STX     x9, clock_start_ns
    b       clock_init.done
clock_init.virtual:
    mov     w9, #1
    STB     w9, clock_is_virtual
    STX     xzr, clock_now              // 0.0
    LDX     x9, cfg_frame_rate
    cmp     x9, #0
    b.le    clock_init.default_rate
    scvtf   d1, x9
    b       clock_init.dt
clock_init.default_rate:
    mov     x9, #60
    scvtf   d1, x9
clock_init.dt:
    fmov    d0, #1.0
    fdiv    d0, d0, d1
    STD     d0, clock_dt
clock_init.done:
    ldr     x30, [sp], #16
    ret

// clock_advance_frame: virtual time moves one frame (repeated addition).
clock_advance_frame:
    LDB     w9, clock_is_virtual
    cbz     w9, clock_advance_frame.done
    LDD     d0, clock_now
    LDD     d1, clock_dt
    fadd    d0, d0, d1
    STD     d0, clock_now
clock_advance_frame.done:
    ret

// clock_wall -> d0 (time.time() analog); clock_monotonic -> d0.
clock_wall:
    LDB     w9, clock_is_virtual
    cbnz    w9, clock_virtual_now
    str     x30, [sp, #-16]!
    bl      monotonic_ns
    LDX     x3, clock_start_ns
    sub     x9, x9, x3
    scvtf   d0, x9
    LDD     d1, one_billion
    fdiv    d0, d0, d1
    LDD     d1, clock_wall_start
    fadd    d0, d0, d1
    ldr     x30, [sp], #16
    ret
clock_monotonic:
    LDB     w9, clock_is_virtual
    cbnz    w9, clock_virtual_now
    str     x30, [sp, #-16]!
    bl      monotonic_ns
    LDX     x3, clock_start_ns
    sub     x9, x9, x3
    scvtf   d0, x9
    LDD     d1, one_billion
    fdiv    d0, d0, d1
    ldr     x30, [sp], #16
    ret
clock_virtual_now:
    LDD     d0, clock_now
    ret

// next_frame -> w9 = 1 when the effect produced a frame (the effect's
// next_frame followed by ctx.frame(): pacing, then the virtual clock).
next_frame:
    str     x30, [sp, #-16]!
    bl      recycle_flush               // engine/particles.s
    bl      audio_frame_poll
    LDX     x9, effect_next_frame
    blr     x9
    cbz     w9, next_frame.done
    bl      enforce_framerate
    bl      clock_advance_frame
    mov     w9, #1
next_frame.done:
    ldr     x30, [sp], #16
    ret

// Deliver the resolved ASM grid to a synchronous Rust callback. The grid and
// interned visual pool are borrowed and cannot be retained by the caller.
// No ANSI rows, cursor protocol, writev, or render thread are used here.
run_direct_frames:
    stp     x19, x20, [sp, #-64]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    str     x30, [sp, #48]
    mov     x23, #0                     // frames emitted
    mov     w24, #OUT_COMPLETE          // check_stop's outcome register
run_direct_frames.frame:
    bl      check_stop
    b.ne    run_direct_frames.done
    bl      next_frame                // preserve ASM effect audio and pacing
    cbz     w9, run_direct_frames.done
    bl      render_catch_up           // resolve movement and occlusion
    bl      check_stop
    b.ne    run_direct_frames.done
    bl      visual_export_audio       // current-frame ASM color, including cached scenes
    // x3/x4 are the frame-local borrowed grid/pool from visual_export_audio.
    LDX     x9, request
    ldr     x0, [x9, #RQ_DIRECT_CTX]
    ldr     x10, [x9, #RQ_DIRECT_EMIT]
    LDX     x1, grid_width
    LDX     x2, grid_height
    CCALLR  x10
    cbnz    x9, run_direct_frames.stopped // nonzero asks the producer to stop
    add     x23, x23, #1
    LDX     x9, cfg_max_frames
    cmp     x23, x9
    b.lo    run_direct_frames.frame
    b       run_direct_frames.done
run_direct_frames.stopped:
    mov     w24, #OUT_TERMINATED
run_direct_frames.done:
    mov     x9, x24
    ldr     x30, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #64
    ret

// run_effect -> x9 = outcome. Prep the canvas, stream frames, always
// restore the cursor. x21 = pending-bytes cursor, x22 = pending base (the
// open ring entry's prefix buffer; the first is at out_base).
run_effect:
    stp     x19, x21, [sp, #-48]!
    stp     x22, x23, [sp, #16]
    stp     x24, x30, [sp, #32]
    bl      pipeline_start
    LDX     x22, out_base
    mov     x21, x22
    mov     w24, #OUT_COMPLETE
    // prep_canvas
    ADRG    x1, ansi_hide_cursor
    mov     w3, #ansi_hide_cursor_len
    bl      out_bytes
    bl      build_move_to_top
    LDB     w9, cfg_reuse_canvas
    cbz     w9, run_effect.prep_rows
    bl      out_move_to_top
run_effect.prep_rows:
    LDX     x19, visible_top
run_effect.prep_row:
    cmp     x19, #0
    b.le    run_effect.prep_done
    LDX     x3, visible_right
    cmp     x3, #0
    b.le    run_effect.prep_newline
    mov     x0, x21
    mov     w9, #0x20                   // ' '
    REP_STOSB
    mov     x21, x0
run_effect.prep_newline:
    mov     w9, #10
    strb    w9, [x21], #1
    sub     x19, x19, #1
    b       run_effect.prep_row
run_effect.prep_done:
    ADRG    x1, ansi_dec_save
    mov     w3, #ansi_dec_save_len
    bl      out_bytes
run_effect.frame:
    bl      check_stop
    b.ne    run_effect.teardown
    bl      next_frame
    cbz     w9, run_effect.teardown
    mov     x23, x21                    // where this frame's prefix starts
    bl      out_move_to_top
    bl      check_stop
    b.ne    run_effect.discard
    // pending bytes (prep, cursor move) and the frame's rows in one writev,
    // here or on the render thread
    bl      frame_submit
    cbnz    x9, run_effect.write_failed
    b       run_effect.frame
run_effect.discard:
    mov     x21, x23
run_effect.teardown:
    // every frame handed over is out before the teardown bytes
    bl      pipeline_finish
    cbnz    x9, run_effect.write_failed
    cmp     w24, #OUT_RESIZED
    b.eq    run_effect.resized
    bl      restore_cursor
    bl      flush_output
    cbnz    x9, run_effect.write_failed_late
    b       run_effect.finish
run_effect.resized:
    // leave the cursor hidden, parked at the top of the wiped area
    ADRG    x1, ansi_dec_restore
    mov     w3, #ansi_dec_restore_len
    bl      out_bytes
    LDX     x1, visible_top
    cmp     x1, #0
    b.le    run_effect.clear
    mov     w9, #0x1b
    strb    w9, [x21]
    mov     w9, #0x5b                   // '['
    strb    w9, [x21, #1]
    add     x0, x21, #2
    bl      format_u64
    add     x21, x21, x9
    add     x21, x21, #2
    mov     w9, #0x41                   // 'A'
    strb    w9, [x21], #1
run_effect.clear:
    ADRG    x1, ansi_clear_to_end
    mov     w3, #ansi_clear_to_end_len
    bl      out_bytes
    bl      flush_output
    cbnz    x9, run_effect.write_failed_late
    b       run_effect.finish
run_effect.write_failed:
    mov     x19, x9
    bl      pipeline_finish
    mov     x9, x19
    bl      teardown_failed
    mov     x24, x9
    b       run_effect.finish
run_effect.write_failed_late:
    bl      write_outcome
    mov     x24, x9
run_effect.finish:
    mov     x9, x24
    ldp     x24, x30, [sp, #32]
    ldp     x22, x23, [sp, #16]
    ldp     x19, x21, [sp], #48
    ret

// teardown_failed(x9=-errno of a failed frame) -> x9 = outcome. A failed
// frame still gets its teardown attempt, as run_effect does. x22 = pending
// base; the pending bytes are dropped. Clobbers C but x20, x22-x24 (and
// x19, which it restores).
teardown_failed:
    stp     x19, x30, [sp, #-16]!
    mov     x19, x9
    mov     x21, x22
    bl      restore_cursor
    bl      flush_output
    mov     x9, x19
    ldp     x19, x30, [sp], #16
// write_outcome(x9=-errno) -> x9 = outcome: the terminal going away (EIO,
// EPIPE) is a quiet end, not an error.
write_outcome:
    LDB     w3, cfg_tty_output
    cbz     w3, write_outcome.io_error
    cmn     x9, #EIO
    b.eq    write_outcome.closed
    cmn     x9, #EPIPE
    b.ne    write_outcome.io_error
write_outcome.closed:
    mov     x9, #OUT_OUTPUT_CLOSED
write_outcome.io_error:
    ret

// frame_submit -> x9 = 0, or -errno of a failed write (on the render thread
// possibly an earlier frame's). The frame is complete: its change log ends at
// log_ptr, its pending bytes are x22..x21. It is rendered and written here,
// or handed to the render thread, and x21/x22 move to the next ring entry,
// waiting for it to be free. Clobbers C but x19, x20, x23, x24.
//
// The hand-over publishes the entry, the log and the prefix bytes with the
// release increment of r_submitted, and reuses an entry only after an
// acquire load of r_completed shows the renderer is done with it. Each
// sleeping-flag check follows a full fence (see engine/render.s).
frame_submit:
    stp     x30, xzr, [sp, #-16]!
    LDW     w9, r_submitted
    and     w9, w9, #(FRAME_RING - 1)
    lsl     w9, w9, #FS_SHIFT
    ADRG    x0, ring
    add     x0, x0, x9
    LDX     x9, log_ptr
    str     x9, [x0, #FS_LOG_END]
    sub     x9, x21, x22
    str     x9, [x0, #FS_PREFIX_LEN]
    LDB     w9, pipe_running
    cbnz    w9, frame_submit.hand_over
    // single-threaded: entry 0, over and over
    str     x0, [sp, #8]
    bl      render_emit
    ldr     x0, [sp, #8]
    ldr     x3, [x0, #FS_LOG]
    STX     x3, log_ptr
    mov     x21, x22
    ldr     x30, [sp], #16
    ret
frame_submit.hand_over:
    ADRG    x0, r_submitted
    R_ATOMIC_INC x0, w9
    dmb     ish                         // the count before render_sleeping's load
    // a sleeping renderer wakes once a few frames wait
    LDB     w9, render_sleeping
    cbz     w9, frame_submit.room
    LDW     w9, r_submitted
    ADRG    x16, r_completed
    ldar    w2, [x16]
    sub     w9, w9, w2
    cmp     w9, #FRAME_RING_WAKE
    b.lo    frame_submit.room
    ADRG    x3, render_sleeping
    R_XCHGB w9, x3, wzr
    cbz     w9, frame_submit.room
    ADRG    x0, render_seq
    R_ATOMIC_INC x0, w9
    bl      futex_wake
frame_submit.room:
    // the next entry is free once fewer than FRAME_RING frames are in flight
    LDW     w9, r_submitted
    ADRG    x16, r_completed
    ldar    w2, [x16]
    sub     w9, w9, w2
    cmp     w9, #FRAME_RING
    b.lo    frame_submit.free
    LDW     w1, main_seq
    mov     w9, #1
    ADRG    x3, main_sleeping
    R_XCHGB w2, x3, w9
    LDW     w9, r_submitted
    ADRG    x16, r_completed
    ldar    w2, [x16]
    sub     w9, w9, w2
    cmp     w9, #FRAME_RING
    b.lo    frame_submit.awake
    ADRG    x0, main_seq
    bl      futex_wait
frame_submit.awake:
    STB     wzr, main_sleeping
    b       frame_submit.room
frame_submit.free:
    LDW     w9, r_submitted
    and     w9, w9, #(FRAME_RING - 1)
    lsl     w9, w9, #FS_SHIFT
    ADRG    x0, ring
    add     x0, x0, x9
    ldr     x9, [x0, #FS_LOG]
    STX     x9, log_ptr
    ldr     x22, [x0, #FS_PREFIX]
    mov     x21, x22
    LDX     x9, render_err
    ldr     x30, [sp], #16
    ret

// check_stop -> Z clear (and x24 = outcome) when the run must stop.
check_stop:
    str     x30, [sp, #-16]!
    bl      stop_requested
    ldr     x30, [sp], #16
    cmp     w9, #STOP_INTERRUPT
    b.eq    check_stop.interrupt
    cmp     w9, #STOP_TERMINATE
    b.eq    check_stop.terminate
    cmp     w9, #STOP_RESIZE
    b.eq    check_stop.resize
    mov     w9, #0
    cmp     w9, #0                      // Z set: keep going
    ret
check_stop.interrupt:
    mov     w24, #OUT_INTERRUPTED
    b       check_stop.stop
check_stop.terminate:
    mov     w24, #OUT_TERMINATED
    b       check_stop.stop
check_stop.resize:
    mov     w24, #OUT_RESIZED
check_stop.stop:
    orr     w9, w9, #1
    cmp     w9, #0                      // Z clear
    ret

// restore_cursor: show the cursor and end the line unless configured not to.
restore_cursor:
    str     x30, [sp, #-16]!
    LDB     w9, cfg_no_restore_cursor
    cbnz    w9, restore_cursor.eol
    ADRG    x1, ansi_show_cursor
    mov     w3, #ansi_show_cursor_len
    bl      out_bytes
restore_cursor.eol:
    LDB     w9, cfg_no_eol
    cbnz    w9, restore_cursor.done
    mov     w9, #10
    strb    w9, [x21], #1
restore_cursor.done:
    ldr     x30, [sp], #16
    ret

// dump_effect -> x9: length-prefixed frames, no tty framing, "frames=N" on
// stderr (effect.rs dump_effect).
dump_effect:
    stp     x22, x24, [sp, #-32]!
    str     x30, [sp, #16]
    LDX     x22, out_base
    mov     x24, #0                     // frame count
dump_effect.frame:
    bl      next_frame
    cbz     w9, dump_effect.done
    bl      render_catch_up
    bl      render_frame
    // "<len>\n" header, the rows, then the frame's trailing newline
    mov     x0, x22
    mov     x1, x9
    bl      format_u64
    mov     w3, #10
    strb    w3, [x22, x9]
    add     x9, x9, #1
    LDX     x0, frame_iov
    stp     x22, x9, [x0]
    LDX     x3, grid_height
    add     x3, x3, #1
    add     x3, x0, x3, lsl #4
    ADRG    x2, newline
    mov     x4, #1
    stp     x2, x4, [x3]
    // the total: header, rows, newline (the x86 code adds the newline's
    // address instead of the header's length, which only sends writev_keep
    // down its copy path; the bytes written are the same)
    LDX     x2, frame_len
    add     x2, x2, x9
    add     x2, x2, #1
    LDW     w1, grid_height
    add     w1, w1, #2
    LDX     x3, iov_scratch
    bl      writev_keep
    cbnz    x9, dump_effect.failed
    add     x24, x24, #1
    LDX     x9, cfg_max_frames
    cmp     x24, x9                     // unsigned: -1 (no limit) is never reached
    b.lo    dump_effect.frame
dump_effect.done:
    ADRG    x1, msg_frames_eq
    mov     x0, x22
    mov     x3, #msg_frames_eq_len
    REP_MOVSB
    mov     x1, x24
    bl      format_u64
    add     x2, x9, #msg_frames_eq_len
    mov     w3, #10
    strb    w3, [x22, x2]
    add     x2, x2, #1
    mov     x1, x22
    mov     w0, #2
    bl      write_all
    mov     x9, #OUT_COMPLETE
dump_effect.failed:
    ldr     x30, [sp, #16]
    ldp     x22, x24, [sp], #32
    ret

// out_bytes(x1=src, w3=len): append to the pending buffer at x21.
// Clobbers x0, x1, x3, x16, x17.
out_bytes:
    mov     x0, x21
    REP_MOVSB
    mov     x21, x0
    ret

// flush_output -> x9 = 0 or -errno: write x22..x21 and rewind.
flush_output:
    str     x30, [sp, #-16]!
    mov     w0, #1
    mov     x1, x22
    sub     x2, x21, x22
    bl      write_all
    mov     x21, x22
    ldr     x30, [sp], #16
    ret

// build_move_to_top: "\x1b8\x1b7\x1b[<visible_top.max(0)>A".
build_move_to_top:
    str     x30, [sp, #-16]!
    ADRG    x0, move_to_top
    mov     w9, #0x381b                 // ESC 8 ESC 7
    movk    w9, #0x371b, lsl #16
    str     w9, [x0]
    mov     w9, #0x5b1b                 // ESC [
    strh    w9, [x0, #4]
    add     x0, x0, #6
    LDX     x1, visible_top
    cmp     x1, #0
    csel    x1, xzr, x1, mi
    bl      format_u64
    ADRG    x0, move_to_top
    add     x0, x0, #6
    add     x0, x0, x9
    mov     w3, #0x41                   // 'A'
    strb    w3, [x0]
    add     w9, w9, #7
    STW     w9, move_to_top_len
    ldr     x30, [sp], #16
    ret

out_move_to_top:
    ADRG    x1, move_to_top
    LDW     w3, move_to_top_len
    b       out_bytes

// enforce_framerate: Terminal.enforce_framerate on the real clock only.
// The timestamp is taken after the sleep, so drift accumulates, faithfully.
enforce_framerate:
    LDB     w9, clock_is_virtual
    cbnz    w9, enforce_framerate.done
    LDX     x3, cfg_frame_rate
    cbz     x3, enforce_framerate.done
    str     x30, [sp, #-16]!
    MOV64   x9, 1000000000
    udiv    x9, x9, x3
    str     x9, [sp, #8]                // frame delay in ns
    bl      monotonic_ns
    LDX     x3, last_frame_ns
    sub     x9, x9, x3
    ldr     x0, [sp, #8]
    cmp     x9, x0
    b.ge    enforce_framerate.stamp
    sub     x0, x0, x9
    bl      sleep_ns
enforce_framerate.stamp:
    bl      monotonic_ns
    STX     x9, last_frame_ns
    ldr     x30, [sp], #16
enforce_framerate.done:
    ret

.include "rt/sys.s"
.include "utils/rng.s"
.include "utils/graphics.s"
.include "utils/pycompat.s"
.include "utils/geometry.s"
.include "utils/color.s"
.include "utils/hexterm.s"
.include "utils/easing.s"
.include "engine/visual.s"
.include "engine/audio_export.s"
.include "engine/chars.s"
.include "engine/input.s"
.include "engine/terminal.s"
.include "engine/scene.s"
.include "engine/events.s"
.include "engine/motion.s"
.include "engine/update.s"
.include "engine/particles.s"
.include "engine/render.s"
.include "utils/spanning_tree.s"
.include "effects/registry.s"
.include "tests.s"

    .section .rodata
    STRING msg_frames_eq, "frames="
    STRING ansi_hide_cursor, "\033[?25l"
    STRING ansi_show_cursor, "\033[?25h"
    STRING ansi_dec_save, "\0337"
    STRING ansi_dec_restore, "\0338"
    STRING ansi_clear_to_end, "\033[0J"
newline:
    .byte   10

    .bss
    .balign 8
request:            .skip 8             // persists: engine_fail needs it
fail_rsp:           .skip 8             // ttfx_asm_run's sp after CENTRY

    TSTATE
    .balign 8
effect_build:       .skip 8
effect_next_frame:  .skip 8
effect_config:      .skip 8
input_ptr:          .skip 8
input_len:          .skip 8
last_frame_ns:      .skip 8
clock_start_ns:     .skip 8
clock_wall_start:   .skip 8
clock_now:          .skip 8
clock_dt:           .skip 8
cfg_tab_width:      .skip 8
cfg_frame_rate:     .skip 8
cfg_canvas_width:   .skip 8
cfg_canvas_height:  .skip 8
cfg_anchor_canvas:  .skip 8
cfg_anchor_text:    .skip 8
cfg_existing_colors: .skip 8
cfg_max_frames:     .skip 8
move_to_top:        .skip 32
fail_buffer:        .skip 256
move_to_top_len:    .skip 4
clock_is_virtual:   .skip 1
// RQ_FLAGS, one byte per bit, in FL_* order
cfg_flag_bytes:
cfg_xterm_colors:       .skip 1
cfg_no_color:           .skip 1
cfg_wrap_text:          .skip 1
cfg_ignore_dims:        .skip 1
cfg_reuse_canvas:       .skip 1
cfg_no_eol:             .skip 1
cfg_no_restore_cursor:  .skip 1
cfg_parity_dump:        .skip 1
cfg_virtual_clock:      .skip 1
cfg_tty_output:         .skip 1
                        .skip 6
    .balign 8                           // scaled :lo12: stores need natural alignment
audio_word:             .skip 8         // this frame's packed audio_poll word
audio_color_on:         .skip 4         // the params callback's active flag
    .balign 16
audio_params:                           // brightness f32, then 9 matrix f32
audio_bright:           .skip 4
audio_matrix:           .skip 36

    TSTATE
    .balign 64
tstate_end:

    .text
