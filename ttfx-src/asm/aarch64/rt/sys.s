// rt/sys.s - raw syscall runtime: output, memory, tty queries, signals,
// clock. Replaces what std and libc provide to the Rust binary.
// (MAX_REGIONS and REGION_STAGGER come from defs.inc.)

    .text

// write_all(w0=fd, x1=ptr, x2=len) -> x9 = 0, or -errno on failure.
// Retries short writes and EINTR, like Write::write_all.
write_all:
    stp     x19, x20, [sp, #-32]!
    str     x21, [sp, #16]
    mov     w19, w0
    mov     x20, x1
    mov     x21, x2
write_all.loop:
    cbz     x21, write_all.done
    mov     w0, w19
    mov     x1, x20
    mov     x2, x21
    SYSCALL SYS_write
    cmn     x9, #EINTR
    b.eq    write_all.loop
    tbnz    x9, #63, write_all.out
    add     x20, x20, x9
    sub     x21, x21, x9
    b       write_all.loop
write_all.done:
    mov     x9, #0
write_all.out:
    ldr     x21, [sp, #16]
    ldp     x19, x20, [sp], #32
    ret

// writev_all(x0=iovec array, w1=count) -> x9 = 0, or -errno on failure.
// Retries short writes (advancing through the vectors) and EINTR. The array
// is consumed in place.
writev_all:
    PUSH2   x19, x20
    mov     x19, x0
    mov     w20, w1
writev_all.loop:
    // skip vectors that are already empty
    cbz     w20, writev_all.done
    ldr     x9, [x19, #8]
    cbnz    x9, writev_all.write
    add     x19, x19, #16
    sub     w20, w20, #1
    b       writev_all.loop
writev_all.write:
    mov     w0, #1
    mov     x1, x19
    mov     w2, w20
    mov     w9, #1024                   // IOV_MAX
    cmp     w2, w9
    csel    w2, w9, w2, hi
    SYSCALL SYS_writev
    cmn     x9, #EINTR
    b.eq    writev_all.loop
    tbnz    x9, #63, writev_all.out
writev_all.consume:
    cbz     x9, writev_all.loop
    ldr     x3, [x19, #8]
    cmp     x9, x3
    b.lo    writev_all.partial
    sub     x9, x9, x3
    str     xzr, [x19, #8]
    add     x19, x19, #16
    sub     w20, w20, #1
    b       writev_all.consume
writev_all.partial:
    ldr     x3, [x19]
    add     x3, x3, x9
    str     x3, [x19]
    ldr     x3, [x19, #8]
    sub     x3, x3, x9
    str     x3, [x19, #8]
    b       writev_all.loop
writev_all.done:
    mov     x9, #0
writev_all.out:
    POP2    x19, x20
    ret

// writev_keep(x0=iovec array, w1=count, x2=their total length, x3=scratch
// array of count entries) -> x9 = 0, or -errno on failure. writev_all that
// leaves the array as it is: when one writev does not do it all (a short
// write, EINTR, more than IOV_MAX vectors), the rest goes on from a copy.
writev_keep:
    stp     x19, x20, [sp, #-48]!
    stp     x21, x22, [sp, #16]
    str     x30, [sp, #32]
    mov     x19, x0
    mov     w20, w1
    mov     x21, x2
    mov     x22, x3
    mov     x9, #0
    cmp     w1, #1024                   // IOV_MAX
    b.hi    writev_keep.copy
    mov     w0, #1
    mov     x1, x19
    mov     w2, w20
    SYSCALL SYS_writev
    cmp     x9, x21
    b.eq    writev_keep.done
    tbz     x9, #63, writev_keep.copy
    cmn     x9, #EINTR
    b.ne    writev_keep.out
    mov     x9, #0
writev_keep.copy:
    // x9 bytes are out; the copy is consumed past them
    mov     x0, x22
    mov     x1, x19
    lsl     w3, w20, #4
    REP_MOVSB
    mov     x0, x22
    mov     w1, w20
writev_keep.consume:
    cbz     x9, writev_keep.rest
    ldr     x3, [x0, #8]
    cmp     x9, x3
    b.lo    writev_keep.partial
    sub     x9, x9, x3
    add     x0, x0, #16
    sub     w1, w1, #1
    b       writev_keep.consume
writev_keep.partial:
    ldr     x3, [x0]
    add     x3, x3, x9
    str     x3, [x0]
    ldr     x3, [x0, #8]
    sub     x3, x3, x9
    str     x3, [x0, #8]
writev_keep.rest:
    bl      writev_all
    b       writev_keep.out
writev_keep.done:
    mov     x9, #0
writev_keep.out:
    ldr     x30, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #48
    ret

// exit(w0=code) - never returns.
exit:
    SYSCALL SYS_exit_group
    udf     #0

// reserve(x0=bytes) -> x9 = base of a lazily committed read/write mapping.
// Regions are sized far beyond need and never move, so pointers into them
// stay valid for the whole run (plan §7.1). They are recorded so the next run
// (after a terminal resize) can release them first.
//
// The regions are huge and mmap packs them together, so without care every
// region's base has the same low 28 bits: a slot's entries in all the
// character field arrays would share one cache set, and a load from one
// array would wait on a store to another (4K aliasing). Each region's base
// is therefore staggered by region_index * (4096 + 192) bytes, which gives
// every region its own 64-byte line offset within a page.
// Clobbers x0-x5, x8, x10-x13.
reserve:
    LDW     w9, region_count
    mov     w10, #REGION_STAGGER
    mul     w9, w9, w10
    add     x0, x0, x9                  // the stagger comes out of the region
    mov     x10, x9                     // stagger
    mov     x11, x0                     // length
    mov     x1, x0
    mov     x0, #0
    mov     w2, #PROT_RW
    mov     w3, #(MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE)
    mov     x4, #-1
    mov     x5, #0
    SYSCALL SYS_mmap
    cmn     x9, #4096                   // above -4096: an error
    b.hi    reserve.fail
    ADRG    x12, region_count
    ldr     w3, [x12]
    cmp     w3, #MAX_REGIONS
    b.hs    reserve.fail
    ADRG    x2, regions
    add     x2, x2, x3, lsl #4
    stp     x9, x11, [x2]
    add     w3, w3, #1
    str     w3, [x12]
    add     x9, x9, x10
    ret
reserve.fail:
    ADRG    x0, msg_oom
    mov     w1, #msg_oom_len
    b       fatal

// release_regions: unmap everything a previous run reserved.
release_regions:
    PUSH2   x19, x20
    mov     w19, #0
    ADRG    x20, regions
release_regions.next:
    LDW     w9, region_count
    cmp     w19, w9
    b.hs    release_regions.done
    add     x9, x20, x19, lsl #4
    ldp     x0, x1, [x9]
    SYSCALL SYS_munmap
    add     w19, w19, #1
    b       release_regions.next
release_regions.done:
    STW     wzr, region_count
    POP2    x19, x20
    ret

// fatal(x0=message, w1=length): an internal limit was hit; there is no
// sensible way to continue, so report it and exit like a Rust panic would
// (after the frames already handed to the render thread are out). Never
// returns, so x30 is not kept.
fatal:
    PUSH2   x0, x1
    bl      pipeline_finish
    POP2    x0, x1
    mov     x2, x1
    mov     x1, x0
    mov     w0, #2
    bl      write_all
    mov     w0, #101
    b       exit

// alloc(x0=bytes) -> x9 = 64-byte aligned, zeroed memory from the arena.
// Clobbers x0 and x16 only.
alloc:
    adrp    x16, arena_ptr
    ldr     x9, [x16, :lo12:arena_ptr]
    add     x0, x0, #63
    and     x0, x0, #-64
    add     x0, x0, x9
    str     x0, [x16, :lo12:arena_ptr]
    ret

// isatty(w0=fd) -> w9 = 1 when the descriptor is a terminal (TCGETS works).
isatty:
    sub     sp, sp, #64
    mov     w1, #TCGETS
    mov     x2, sp
    SYSCALL SYS_ioctl
    add     sp, sp, #64
    cmp     x9, #0
    cset    w9, eq
    ret

// winsize(w0=fd) -> w9 = columns, w2 = rows; both 0 when unavailable.
winsize:
    sub     sp, sp, #16
    mov     w1, #TIOCGWINSZ
    mov     x2, sp
    SYSCALL SYS_ioctl
    cbnz    x9, winsize.none
    ldrh    w2, [sp]                    // ws_row
    ldrh    w9, [sp, #2]                // ws_col
    add     sp, sp, #16
    ret
winsize.none:
    mov     w9, #0
    mov     w2, #0
    add     sp, sp, #16
    ret

// monotonic_ns -> x9 = CLOCK_MONOTONIC in nanoseconds.
monotonic_ns:
    sub     sp, sp, #16
    mov     w0, #CLOCK_MONOTONIC
    mov     x1, sp
    SYSCALL SYS_clock_gettime
    ldr     x9, [sp]
    MOV64   x0, 1000000000
    mul     x9, x9, x0
    ldr     x1, [sp, #8]
    add     x9, x9, x1
    add     sp, sp, #16
    ret

// realtime_s -> d0 = CLOCK_REALTIME in seconds (SystemTime::now()).
realtime_s:
    sub     sp, sp, #16
    mov     w0, #CLOCK_REALTIME
    mov     x1, sp
    SYSCALL SYS_clock_gettime
    LDD     d0, one_billion
    ldr     x0, [sp, #8]
    scvtf   d1, x0
    fdiv    d1, d1, d0
    ldr     x0, [sp]
    scvtf   d0, x0
    fadd    d0, d0, d1
    add     sp, sp, #16
    ret

// sleep_ns(x0=nanoseconds). Resumes after signals, like thread::sleep.
sleep_ns:
    sub     sp, sp, #32
    MOV64   x3, 1000000000
    udiv    x9, x0, x3
    msub    x2, x9, x3, x0
    stp     x9, x2, [sp]
sleep_ns.again:
    mov     x0, sp
    add     x1, sp, #16
    SYSCALL SYS_nanosleep
    cmn     x9, #EINTR
    b.ne    sleep_ns.done
    ldp     x9, x2, [sp, #16]
    stp     x9, x2, [sp]
    b       sleep_ns.again
sleep_ns.done:
    add     sp, sp, #32
    ret

// ------------------------------------------------------------------ threads
// The render thread (render.s) is a pthread: glibc is linked, and a raw
// clone would bypass its thread setup inside the Rust process.

// futex_wait(x0=32-bit word, w1=expected): sleep while the word holds the
// expected value, until a futex_wake. May return early (a signal, a race);
// callers recheck.
futex_wait:
    mov     w2, w1
    mov     w1, #FUTEX_WAIT_PRIVATE
    mov     x3, #0
    SYSCALL SYS_futex
    ret

// futex_wake(x0=32-bit word): wake a thread sleeping on it.
futex_wake:
    mov     w1, #FUTEX_WAKE_PRIVATE
    mov     w2, #1
    SYSCALL SYS_futex
    ret

// thread_start(x0=start routine, x1=pthread_t out) -> w9 = 0, or an
// error number. The thread starts with every asynchronous signal blocked:
// the Rust handlers only set flags that the main thread's stop checks read,
// so SIGINT, SIGTERM and SIGWINCH must reach the main thread. Signals the
// thread raises itself stay deliverable: SIGPIPE from a write to a closed
// pipe must still end the process by default, as it does single-threaded,
// and faults must not be held pending. Clobbers C.
thread_start:
    stp     x19, x20, [sp, #-32]!
    str     x30, [sp, #16]
    sub     sp, sp, #256                // the thread's mask, then main's
    mov     x19, x0
    mov     x20, x1
    mov     x0, sp
    mov     x9, #-1
    mov     x3, #16
    REP_STOSQ
    ldr     x9, [sp]
    mov     x10, #((1 << (SIGPIPE - 1)) | (1 << (SIGSEGV - 1)) | (1 << (SIGBUS - 1)) | (1 << (SIGILL - 1)) | (1 << (SIGFPE - 1)) | (1 << (SIGTRAP - 1)))
    bic     x9, x9, x10
    str     x9, [sp]
    mov     w0, #SIG_SETMASK
    mov     x1, sp
    add     x2, sp, #128
    CCALL   pthread_sigmask
    mov     x0, x20
    mov     x1, #0
    mov     x2, x19
    mov     x3, #0
    CCALL   pthread_create
    mov     w19, w9
    mov     w0, #SIG_SETMASK
    add     x1, sp, #128
    mov     x2, #0
    CCALL   pthread_sigmask
    mov     w9, w19
    add     sp, sp, #256
    ldr     x30, [sp, #16]
    ldp     x19, x20, [sp], #32
    ret

// thread_join(x0=pthread_t): wait for the thread to end. Clobbers C.
thread_join:
    PUSH1   x30
    mov     x1, #0
    CCALL   pthread_join
    POP1    x30
    ret

// ----------------------------------------------------------------- utilities

// utf8_decode(x1=ptr to valid UTF-8) -> w9 = codepoint, w2 = byte length.
// Clobbers x3, x4.
utf8_decode:
    ldrb    w9, [x1]
    cmp     w9, #0x80
    b.lo    utf8_decode.one
    cmp     w9, #0xE0
    b.lo    utf8_decode.two
    cmp     w9, #0xF0
    b.lo    utf8_decode.three
    and     w9, w9, #0x07
    mov     w2, #4
    b       utf8_decode.tail
utf8_decode.three:
    and     w9, w9, #0x0F
    mov     w2, #3
    b       utf8_decode.tail
utf8_decode.two:
    and     w9, w9, #0x1F
    mov     w2, #2
utf8_decode.tail:
    mov     w3, #1
utf8_decode.more:
    lsl     w9, w9, #6
    ldrb    w4, [x1, x3]
    and     w4, w4, #0x3F
    orr     w9, w9, w4
    add     w3, w3, #1
    cmp     w3, w2
    b.lo    utf8_decode.more
    ret
utf8_decode.one:
    mov     w2, #1
    ret

// format_u64(x0=buffer with >= 20 bytes, x1=value) -> x9 = length written.
// Clobbers x2, x3, x4, x12.
format_u64:
    sub     sp, sp, #32
    add     x4, sp, #32
    mov     x9, x1
    mov     x3, #10
format_u64.loop:
    udiv    x12, x9, x3
    msub    x2, x12, x3, x9
    mov     x9, x12
    add     w2, w2, #'0'
    strb    w2, [x4, #-1]!
    cbnz    x9, format_u64.loop
    add     x3, sp, #32
    sub     x3, x3, x4
    mov     x2, #0
format_u64.copy:
    ldrb    w9, [x4, x2]
    strb    w9, [x0, x2]
    add     x2, x2, #1
    cmp     x2, x3
    b.lo    format_u64.copy
    mov     x9, x3
    add     sp, sp, #32
    ret

// format_i64(x0=buffer with >= 21 bytes, x1=value) -> x9 = length written.
format_i64:
    tbz     x1, #63, format_u64
    mov     w9, #'-'
    strb    w9, [x0], #1
    neg     x1, x1
    PUSH1   x30
    bl      format_u64
    POP1    x30
    add     x9, x9, #1
    ret

    .section .rodata
    .balign 8
one_billion:    .double 1.0e9
STRING msg_oom, "ttfx: out of memory (asm engine)\n"


// Persistent across runs (not in the per-run state).
    .bss
    .balign 8
regions:        .skip 16 * MAX_REGIONS
region_count:   .skip 4

    TSTATE
    .balign 8
arena_ptr:      .skip 8
