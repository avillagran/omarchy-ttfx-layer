// ttfx.inc - register conventions and macros shared by every aarch64
// ttfx-asm source file (GNU as). The x86-64 engine in asm/ is the source
// this port transcribes; asm/aarch64/PORTING.md is the working manual.
//
// Register map (x86-64 -> aarch64). Every internal routine keeps the x86
// engine's calling convention under this renaming, so a function's header
// comment reads the same on both sides:
//
//   rdi x0   rsi x1   rdx x2   rcx x3   r8 x4    r9 x5     (arguments)
//   rax x9   r10 x10  r11 x11                               (result, scratch)
//   rbx x19  rbp x20  r12 x21  r13 x22  r14 x23  r15 x24   (callee-saved)
//   xmm0-31 v0-31 (d0-d31 for scalar doubles)
//
// - Arguments 7 and 8 (the x86 engine's [rsp+8] and [rsp+16] stack
//   arguments) go in x6 and x7.
// - A second integer result (x86 rdx) stays in x2.
// - x6-x8 and x12-x15 are extra scratch; x16/x17 are reserved for macro
//   expansions. None of them survives a bl or a macro that calls.
// - x25-x28 are extra callee-saved registers: save them if you use them.
// - x18 is never used; x29 only in entry-point frames; x30 is the link
//   register, so every function that calls saves it.
// - sp stays 16-byte aligned at all times (Linux faults sp-relative
//   accesses otherwise): push and pop in pairs (PUSH2/POP2) or 16-byte
//   slots (PUSH1/POP1).
//
// C calls (libm, pthreads, Rust callbacks) use AAPCS64, whose argument
// registers coincide with the map above: CCALL is a plain bl, and its integer
// result is copied to x9. The callee clobbers x0-x17, x30, v0-v7, v16-v31
// and the upper halves of v8-v15, and preserves x19-x28.
//
// FPCR stays at its default (round-to-nearest-even, no flush-to-zero, no
// default-NaN) and no engine code uses a fused multiply-add, so float results
// match the Rust oracle bit for bit.

.ifndef TTFX_INC
.equ TTFX_INC, 1

.include "defs.inc"

// ---------------------------------------------------------------- syscalls
// The aarch64 (asm-generic) numbers. defs.inc leaves out the x86 ones.
.equ SYS_read,              63
.equ SYS_write,             64
.equ SYS_writev,            66
.equ SYS_close,             57
.equ SYS_openat,            56
.equ SYS_mmap,              222
.equ SYS_munmap,            215
.equ SYS_madvise,           233
.equ SYS_rt_sigaction,      134
.equ SYS_rt_sigreturn,      139
.equ SYS_ioctl,             29
.equ SYS_nanosleep,         101
.equ SYS_getpid,            172
.equ SYS_kill,              129
.equ SYS_clock_gettime,     113
.equ SYS_futex,             98
.equ SYS_sched_getaffinity, 123
.equ SYS_exit_group,        94

// ------------------------------------------------------------------ macros

// EXPORT name: a C entry point. Put it immediately before the function's
// own label: it defines name_v1 (what src/asm/ffi.rs calls; aarch64 has the
// one tier) and makes name global too (tests/asm_diff.rs calls the
// ttfx_test_* thunks by their plain names).
.macro EXPORT name
    .global \name
    .global \name\()_v1
    .type \name, %function
    .type \name\()_v1, %function
\name\()_v1:
.endm

// CENTRY / CEXIT: the frame of a function C calls (the entry points, test
// thunks, the render thread's start routine). Internal code may clobber
// every vector register, as on x86, so this saves what AAPCS64 makes
// callee-saved: x19-x30 and d8-d15 (160 bytes). CEXIT restores them and
// returns. x29 points at the frame.
.macro CENTRY
    stp     x29, x30, [sp, #-160]!
    mov     x29, sp
    stp     x19, x20, [sp, #16]
    stp     x21, x22, [sp, #32]
    stp     x23, x24, [sp, #48]
    stp     x25, x26, [sp, #64]
    stp     x27, x28, [sp, #80]
    stp     d8, d9, [sp, #96]
    stp     d10, d11, [sp, #112]
    stp     d12, d13, [sp, #128]
    stp     d14, d15, [sp, #144]
.endm
.macro CEXIT
    ldp     x19, x20, [sp, #16]
    ldp     x21, x22, [sp, #32]
    ldp     x23, x24, [sp, #48]
    ldp     x25, x26, [sp, #64]
    ldp     x27, x28, [sp, #80]
    ldp     d8, d9, [sp, #96]
    ldp     d10, d11, [sp, #112]
    ldp     d12, d13, [sp, #128]
    ldp     d14, d15, [sp, #144]
    ldp     x29, x30, [sp], #160
    ret
.endm

// MOV64 reg, imm: any 64-bit constant (movz + movk for each nonzero half).
.macro MOV64 reg, imm
    movz    \reg, #((\imm) & 0xffff)
    .if (((\imm) >> 16) & 0xffff) != 0
    movk    \reg, #(((\imm) >> 16) & 0xffff), lsl #16
    .endif
    .if (((\imm) >> 32) & 0xffff) != 0
    movk    \reg, #(((\imm) >> 32) & 0xffff), lsl #32
    .endif
    .if (((\imm) >> 48) & 0xffff) != 0
    movk    \reg, #(((\imm) >> 48) & 0xffff), lsl #48
    .endif
.endm

// FCONST dreg, literal: load a double constant from a pooled .rodata slot.
// The literal is anything .double accepts (1.0e9, 0.5, -3.25); for bit
// patterns write the constant into .rodata yourself. Clobbers x16.
.macro FCONST dreg, value
    .pushsection .rodata.fconst, "a"
    .balign 8
.Lfconst\@:
    .double \value
    .popsection
    adrp    x16, .Lfconst\@
    ldr     \dreg, [x16, :lo12:.Lfconst\@]
.endm

// Globals. Symbols are PC-relative (adrp + :lo12:); keep every datum
// naturally aligned, since the scaled :lo12: forms require it.
// ADRG xreg, sym            xreg = &sym
// LDX xreg, sym             xreg = [sym] (64-bit)
// LDW wreg, sym [, xtmp]    wreg = [sym] (32-bit, zero-extended)
// LDSW xreg, sym [, xtmp]   xreg = [sym] (32-bit, sign-extended)
// LDB wreg, sym [, xtmp]    wreg = [sym] (byte)
// LDD dreg, sym [, xtmp]    dreg = [sym] (double)
// STX/STW/STB/STD reg, sym [, xtmp]   [sym] = reg
// The xtmp defaults to x16 and must differ from reg.
.macro ADRG xreg, sym
    adrp    \xreg, \sym
    add     \xreg, \xreg, :lo12:\sym
.endm
.macro LDX xreg, sym
    adrp    \xreg, \sym
    ldr     \xreg, [\xreg, :lo12:\sym]
.endm
.macro LDW wreg, sym, xtmp=x16
    adrp    \xtmp, \sym
    ldr     \wreg, [\xtmp, :lo12:\sym]
.endm
.macro LDSW xreg, sym, xtmp=x16
    adrp    \xtmp, \sym
    ldrsw   \xreg, [\xtmp, :lo12:\sym]
.endm
.macro LDB wreg, sym, xtmp=x16
    adrp    \xtmp, \sym
    ldrb    \wreg, [\xtmp, :lo12:\sym]
.endm
.macro LDD dreg, sym, xtmp=x16
    adrp    \xtmp, \sym
    ldr     \dreg, [\xtmp, :lo12:\sym]
.endm
.macro STX xreg, sym, xtmp=x16
    adrp    \xtmp, \sym
    str     \xreg, [\xtmp, :lo12:\sym]
.endm
.macro STW wreg, sym, xtmp=x16
    adrp    \xtmp, \sym
    str     \wreg, [\xtmp, :lo12:\sym]
.endm
.macro STB wreg, sym, xtmp=x16
    adrp    \xtmp, \sym
    strb    \wreg, [\xtmp, :lo12:\sym]
.endm
.macro STD dreg, sym, xtmp=x16
    adrp    \xtmp, \sym
    str     \dreg, [\xtmp, :lo12:\sym]
.endm

// The stack, 16 bytes at a time.
.macro PUSH2 a, b
    stp     \a, \b, [sp, #-16]!
.endm
.macro POP2 a, b
    ldp     \a, \b, [sp], #16
.endm
.macro PUSH1 a
    str     \a, [sp, #-16]!
.endm
.macro POP1 a
    ldr     \a, [sp], #16
.endm

// SYSCALL nr: arguments in x0-x5 (x86's r10 is x3 here: move it), result in
// x9 (x86's rax) and x0. Clobbers x8.
.macro SYSCALL nr
    mov     x8, #\nr
    svc     #0
    mov     x9, x0
.endm

// CCALL fn / CCALLR xreg: call C (libm, pthreads, a Rust callback). sp is
// always aligned, so this is a bl; the integer result is copied to x9.
// Clobbers x0-x17, x30, v0-v7, v16-v31, the upper halves of v8-v15.
.macro CCALL fn
    bl      \fn
    mov     x9, x0
.endm
.macro CCALLR xreg
    blr     \xreg
    mov     x9, x0
.endm

// FAIL label: return OUT_ERROR from ttfx_asm_run with the message at label
// (label_len must exist), unwinding whatever the engine was doing.
.macro FAIL label
    ADRG    x0, \label
    mov     x1, #\label\()_len
    b       engine_fail
.endm

// PUTS fd, label: write a constant string (label_len must exist), ignoring
// failures. A call: the enclosing function must have saved x30.
.macro PUTS fd, label
    mov     w0, #\fd
    ADRG    x1, \label
    mov     x2, #\label\()_len
    bl      write_all
.endm

// STRING name, "string": a constant string and name_len (the x86 STR;
// gas macro names ignore case, so STR would shadow the str instruction). Write bytes as
// escapes inside the string; use octal (\033) rather than \x, which
// swallows every hex digit that follows.
.macro STRING name, text:vararg
\name:
    .ascii  \text
    .equ    \name\()_len, . - \name
.endm

// String instructions, x86 register roles and results: x0 = rdi (dest),
// x1 = rsi (source), x3 = rcx (count), x9/w9 = rax/eax/al (value). After
// the macro the pointers have advanced and x3 is 0. Forward copies; the
// copies move 16 bytes at a time, so they are not for overlapping ranges
// with the destination less than 16 bytes past the source (write a byte
// loop there). Clobber x16, x17 and the flags.
.macro REP_MOVSB
    cmp     x3, #16
    b.lo    .Lrmb_tail\@
.Lrmb_16\@:
    ldp     x16, x17, [x1], #16
    stp     x16, x17, [x0], #16
    sub     x3, x3, #16
    cmp     x3, #16
    b.hs    .Lrmb_16\@
.Lrmb_tail\@:
    cbz     x3, .Lrmb_done\@
.Lrmb_1\@:
    ldrb    w16, [x1], #1
    strb    w16, [x0], #1
    subs    x3, x3, #1
    b.ne    .Lrmb_1\@
.Lrmb_done\@:
.endm
.macro REP_MOVSD                        // x3 = count of 4-byte words
    lsl     x3, x3, #2
    REP_MOVSB
.endm
.macro REP_MOVSQ                        // x3 = count of 8-byte words
    lsl     x3, x3, #3
    REP_MOVSB
.endm
.macro REP_STOSB                        // stores al (w9's low byte)
    and     x16, x9, #0xff
    orr     x16, x16, x16, lsl #8
    orr     x16, x16, x16, lsl #16
    orr     x16, x16, x16, lsl #32
    b       .Lrsb_check\@
.Lrsb_16\@:
    stp     x16, x16, [x0], #16
    sub     x3, x3, #16
.Lrsb_check\@:
    cmp     x3, #16
    b.hs    .Lrsb_16\@
    cbz     x3, .Lrsb_done\@
.Lrsb_1\@:
    strb    w16, [x0], #1
    subs    x3, x3, #1
    b.ne    .Lrsb_1\@
.Lrsb_done\@:
.endm
.macro REP_STOSD                        // stores eax (w9), x3 words
    cbz     x3, .Lrsd_done\@
.Lrsd_1\@:
    str     w9, [x0], #4
    subs    x3, x3, #1
    b.ne    .Lrsd_1\@
.Lrsd_done\@:
.endm
.macro REP_STOSQ                        // stores rax (x9), x3 words
    tbz     x3, #0, .Lrsq_pairs\@
    str     x9, [x0], #8
    sub     x3, x3, #1
.Lrsq_pairs\@:
    cbz     x3, .Lrsq_done\@
.Lrsq_2\@:
    stp     x9, x9, [x0], #16
    subs    x3, x3, #2
    b.ne    .Lrsq_2\@
.Lrsq_done\@:
.endm

// ------------------------------------------------ float-to-integer helpers
// F64_TO_I64: x9 = d0 as i64 (Rust's saturating cast: NaN -> 0). fcvtzs is
// exactly that; no slow path.
.macro F64_TO_I64
    fcvtzs  x9, d0
.endm

// ROUND_HALF_EVEN: x9 = round_half_even(d0) (utils/pycompat.rs). fcvtns
// rounds ties to even and saturates, NaN -> 0, which is the Rust function
// for every input but +inf: its floor path gives i64::MAX + 1, wrapping to
// i64::MIN. Clobbers x16, x17.
.macro ROUND_HALF_EVEN
    fcvtns  x9, d0
    mov     x16, #0x7fffffffffffffff
    cmp     x9, x16
    b.ne    .Lrhe_done\@
    fmov    x17, d0
    mov     x16, #0x7ff0000000000000
    cmp     x17, x16
    b.ne    .Lrhe_done\@
    mov     x9, #0x8000000000000000
.Lrhe_done\@:
.endm

// FLOORSD dreg: floor, exactly roundsd's mode 1.
.macro FLOORSD dreg
    frintm  \dreg, \dreg
.endm

// ------------------------------------------------------- engine addressing
// SCENE_PTR xdest, xidx: xdest = [scenes] + (u32)idx * SCENE_SIZE. xdest may
// be xidx; neither may be x16.
.macro SCENE_PTR xdest, xidx
    ubfiz   x16, \xidx, #SCENE_SHIFT, #32
    LDX     \xdest, scenes
    add     \xdest, \xdest, x16
.endm

// PATH_PTR xdest, xidx: xdest = [paths] + (u32)idx * PATH_SIZE (128).
.macro PATH_PTR xdest, xidx
    ubfiz   x16, \xidx, #7, #32
    LDX     \xdest, paths
    add     \xdest, \xdest, x16
.endm

// MV_P8 / MV_P4 xdest, xslot: the motion block's base for 8- or 4-byte
// fields (engine/motion.asm). xdest must differ from x16, x17.
.macro MV_P8 xdest, xslot
    lsr     x17, \xslot, #3
    mov     x16, #(MV_BLOCK - 64)
    mul     x17, x17, x16
    LDX     \xdest, mv_base
    add     \xdest, \xdest, x17
.endm
.macro MV_P4 xdest, xslot
    lsr     x17, \xslot, #3
    mov     x16, #(MV_BLOCK - 32)
    mul     x17, x17, x16
    LDX     \xdest, mv_base
    add     \xdest, \xdest, x17
.endm

// MARK_CANDIDATE: slot w0 may have stopped being active; the next prune
// re-checks it. Clobbers x9, x3, x2, x16.
.macro MARK_CANDIDATE
    LDX     x9, candidate_bits
    ubfx    x3, x0, #6, #26
    ldr     x2, [x9, x3, lsl #3]
    mov     x16, #1
    lsl     x16, x16, x0
    orr     x2, x2, x16
    str     x2, [x9, x3, lsl #3]
.endm

// SET_HANDLE: slot w0's current visual becomes handle w9. A character with a
// render cell logs the change for a render thread, or without one shows it
// in the grid (handle_direct, which preserves everything but x3, x16, x17).
// Contains a bl: the enclosing function must have saved x30. Clobbers x3,
// x16, x17.
.macro SET_HANDLE
    LDX     x3, ch_handle
    str     w9, [x3, w0, uxtw #2]
    LDX     x3, ch_cell
    ldr     w3, [x3, w0, uxtw #2]
    cmn     w3, #1                      // NONE
    b.eq    .Lsh_done\@
    LDB     w16, log_handles
    cbnz    w16, .Lsh_log\@
    bl      handle_direct
    b       .Lsh_done\@
.Lsh_log\@:
    LDX     x3, log_ptr
    stp     w0, w9, [x3], #8
    STX     x3, log_ptr
.Lsh_done\@:
.endm

// ---------------------------------------------------------------- the RNG
// Draws with the position in a register, for hot loops (utils/rng.asm):
//   RNG_OPEN xpos, xbase        xpos = rng_pos, xbase = &rng_buf
//   RNG_TAKE xdest, xpos, xbase  the next raw u64 draw
//   RNG_TAKE53 xdest, xpos, xbase  the next draw >> 11
//   RNG_CLOSE xpos              store the position back
// rng_refill preserves every register but x16, x17 and the flags, so the
// macros can sit anywhere; it runs through a bl with x30 saved around it.
// None of the operands may be x16 or x17; xdest differs from xpos, xbase.
.macro RNG_OPEN xpos, xbase
    LDX     \xpos, rng_pos
    ADRG    \xbase, rng_buf
.endm
.macro RNG_TAKE xdest, xpos, xbase
    cmp     \xpos, #RNG_BATCH
    b.lo    .Lrt_ready\@
    str     x30, [sp, #-16]!
    bl      rng_refill
    ldr     x30, [sp], #16
    mov     \xpos, #0
.Lrt_ready\@:
    ldr     \xdest, [\xbase, \xpos, lsl #3]
    add     \xpos, \xpos, #1
.endm
.macro RNG_TAKE53 xdest, xpos, xbase
    RNG_TAKE \xdest, \xpos, \xbase
    lsr     \xdest, \xdest, #11
.endm
.macro RNG_CLOSE xpos
    STX     \xpos, rng_pos
.endm

// RNG_BITS53: x9 = next >> 11, the integer behind rng_random. Clobbers x3,
// x16, and when the batch refills whatever rng_next clobbers (x2, x4, x5,
// x10, x11 and the scratch registers).
.macro RNG_BITS53
    LDX     x3, rng_pos
    cmp     x3, #RNG_BATCH
    b.hs    .Lrb_refill\@
    ADRG    x9, rng_buf
    ldr     x9, [x9, x3, lsl #3]
    add     x3, x3, #1
    STX     x3, rng_pos
    b       .Lrb_done\@
.Lrb_refill\@:
    str     x30, [sp, #-16]!
    bl      rng_next
    ldr     x30, [sp], #16
.Lrb_done\@:
    lsr     x9, x9, #11
.endm

// ------------------------------------------------------ character fields
// CHAR_FIELDS m: invoke macro m (name, bytes per character, initial value
// or 0 = leave zeroed) for every per-character array (engine/chars.asm).
.macro CHAR_FIELDS m
    \m ch_sym, 8, 0
    \m ch_row, 4, 0
    \m ch_col, 4, 0
    \m ch_irow, 4, 0
    \m ch_icol, 4, 0
    \m ch_id, 4, 0
    \m ch_layer, 4, 0
    \m ch_handle, 4, 0
    \m ch_scene, 4, NONE
    \m ch_scenes, 4, NONE
    \m ch_path, 4, NONE
    \m ch_done_path, 4, NONE
    \m ch_paths, 4, NONE
    \m ch_events, 4, NONE
    \m ch_subs, 1, 0
    \m ch_flags, 2, 0
    \m ch_fg, 8, NONE
    \m ch_bg, 8, NONE
    \m ch_nbr, 16, NONE
    \m ch_cell, 4, NONE
    \m ch_user0, 8, 0
    \m ch_user1, 8, 0
.endm

// Sections. Per-run state lives in .tstate (zeroed at the start of every
// run); read-only pointer tables in .data.rel.ro.
.macro TSTATE
    .section .tstate, "aw", %nobits
.endm
.macro RELRO
    .section .data.rel.ro, "aw"
.endm

.endif
