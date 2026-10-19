# Porting the ttfx assembly engine to aarch64

The x86-64 engine in `asm/` is the source. This directory transcribes it file for file
into GNU as for aarch64 (ARMv8.0-A + NEON, no LSE, no SVE). Read `asm/PORTING.md` first;
everything it says about the engine (the request, state, colors, symbols, scenes, paths,
events, RNG order, parity) holds here. This file covers only what changes.

The goal is the x86 engine's contract: output **byte-identical** to the Rust engine in the
same binary (the oracle), declining only where the x86 engine declines.

## Status

The whole engine and all 37 effects are ported. Each passes
`tools/asm/oracle.sh <effect> full`, and `cargo test --release --test asm_diff` passes.
The x86 engine's AVX2/AVX-512-only paths (the batched motion blocks, the 8-lane RNG
generator) have no aarch64 counterpart; the SSE2-tier paths they accelerate are ported
instead.

## Layout and build

- `asm/aarch64/<path>.s` ports `asm/<path>.asm` (`engine/scene.s` ports
  `engine/scene.asm`). One translation unit: `lib.s` `.include`s every file in
  `lib.asm`'s order. `build.rs` assembles it with `as -march=armv8-a -I asm/aarch64` on
  aarch64 Linux and links it as tier 1 (entry points exported as `name_v1`).
- `ttfx.inc`: the register map, syscall numbers and every macro shared across files.
  `defs.inc` is **generated** (`tools/asm/aarch64/gen_defs.py`) from the x86 sources:
  every shared `%define` and `struc` as `.equ` with the same names (struc fields as
  `Struc.field`, e.g. `POOL.reset`, and `Struc_size`). Do not edit it; do not
  redefine its names. File-local constants of an effect stay in the effect's file,
  prefixed with the effect's short name.
- `tools/asm/aarch64/check.py <file.s>...` assembles one file on its own and reports
  undefined symbols the x86 engine doesn't know (typos, local labels spelled wrong)
  and x86 top-level labels the port doesn't define. Mark labels you drop on purpose
  with a `// dropped: name ... (why)` line. It must print `ok` before a file is done.
- The oracle is the **aarch64** Rust engine: `TTFX_ASM=0` vs `TTFX_ASM=force` in the
  same binary, exactly as on x86 (`tools/asm/oracle.sh`, `cargo test --release --test
  asm_diff`). To read what the oracle compiles to, build a symbolized copy:
  `CARGO_PROFILE_RELEASE_STRIP=false CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release
  --no-default-features --target-dir target/oracle-prof`.

## Register map and calling convention

Every internal routine keeps its x86 signature under a fixed renaming, so headers and
callers translate mechanically and files ported by different people link together:

| x86 | a64 | | x86 | a64 | | x86 | a64 |
|---|---|---|---|---|---|---|---|
| rdi | x0 | | rax | x9 | | rbx | x19 |
| rsi | x1 | | r10 | x10 | | rbp | x20 |
| rdx | x2 | | r11 | x11 | | r12 | x21 |
| rcx | x3 | | | | | r13 | x22 |
| r8 | x4 | | | | | r14 | x23 |
| r9 | x5 | | | | | r15 | x24 |
| xmmN | vN / dN | | | | | | |

- 32-bit x86 names map to the `w` form (`eax` -> `w9`, `r8d` -> `w4`).
- Results: `rax` -> x9, a second result `rdx` -> x2, `xmm0` -> d0. Arguments `xmm0-3` ->
  d0-d3.
- Stack arguments: the x86 engine's first stack argument (`[rsp+8]` at entry) goes in
  **x6**, the second (`[rsp+16]`) in **x7**. Alignment pads (`push 0`) disappear.
  (`event_register`'s arg1, `scene_apply_gradient`'s bg spectrum/count, vhstape's EVENT.)
- Callee-saved: x19-x24 (the x86 set) plus x25-x28 if you use them. Everything else,
  including every vector register, is clobbered by a call unless the header says so,
  exactly as on x86.
- Extra scratch with no x86 counterpart: x6-x8, x12-x15. **Never live across a `bl` or
  a macro that calls.** x16/x17 belong to macro expansions (and PLT veneers): use them
  only between two instructions you wrote, never across any macro.
- x18 is never touched. x29 is only set in C-entry frames (`CENTRY`). x30 is the link
  register: **every function that contains a `bl` (including macros that call:
  `SET_HANDLE`, `PUTS`, `CCALL`) saves x30** in its prologue and restores it before
  `ret`. Leaf functions don't.
- Flags as results work as on x86 (e.g. `check_stop` returns Z clear): make the same
  condition hold in NZCV.
- **sp is always 16-byte aligned.** x86 `push`/`pop` become `PUSH2`/`POP2` pairs (or
  `PUSH1`/`POP1` 16-byte slots). Locals at `[rsp + n]` become `[sp, #n]` in a frame you
  size to a multiple of 16. There is no return address on the stack: a local
  subroutine reached with `call .sub` that reads `[rsp + 8 + n]` reads `[sp, #n]` after
  `bl` (and the caller must have saved x30 before the `bl`).
- C entry points (`ttfx_asm_run`, `ttfx_asm_effect_supported`, the `ttfx_test_*`
  thunks, the render thread's start routine) use `CENTRY`/`CEXIT`, which save x19-x30
  and d8-d15, and move the result from x9 to x0 (or leave d0) before `CEXIT`. Declare
  them with `EXPORT name` on the line just before `name:`.
- C calls: `CCALL fn` (a `bl` plus `mov x9, x0`). Arguments are already in the right
  registers under the map. Drop `ZEROUPPER`.
- Syscalls: `SYSCALL SYS_x` with arguments in x0-x5; the x86 4th argument `r10` is
  **x3** here, so move it. Result in x9 (and x0). The aarch64 numbers are in `ttfx.inc`.
  Flag and struct values (mmap, madvise, ioctl, futex, sigaction, timespec) are the
  same as x86-64.

## Translating instructions

- **Labels:** gas has no NASM-style local scope. Spell a local label `.name` under
  `func:` as `func.name` (a real symbol, so perf shows it, as on x86); `..@x` as plain
  `x`. Numeric labels (`1:` / `1b` / `1f`) are fine for tiny jumps inside your own
  code; macros use `\@` labels. Keep functions that fall through into the next one in
  the same order.
- **Globals:** always PC-relative: `LDX x9, sym`, `STX x9, sym`, `LDW`, `LDB`, `LDD`,
  `ADRG` (see `ttfx.inc`), or `adrp` + `:lo12:` by hand for a base you keep in a
  register. `lea rax, [sym]` -> `ADRG x9, sym`. Tables of pointers go in `RELRO`
  (`.data.rel.ro`) as `.quad`.
- **Addressing:** a64 has `[base, #imm]`, `[base, idx, lsl #s]` (s = 0 or the access
  size), `[base, widx, uxtw/sxtw #s]` and pre/post-index. `[a + b*8 + 16]` needs an
  `add` first. A slot in `edi` used as `[rax + rdi*4]` is `[x9, w0, uxtw #2]` (or
  `x0, lsl #2` when x0's upper half is known zero).
- **Read-modify-write** memory ops (`add [mem], reg`, `inc qword [x]`, `bts [mem], r`)
  become load / op / store.
- **Immediates:** `add`/`sub`/`cmp` take 12 bits (optionally `lsl #12`); logical ops
  only take bitmask patterns; otherwise `mov`/`MOV64` into a scratch register first.
  `cmp x, #-n` -> `cmn x, #n`. Doubles: `fmov d0, #imm` for the few encodable values
  (0.5, 1.0, 2.0, -1.0, ...), else `FCONST d0, 1.2345` or a `.rodata` constant. x86
  `mov rax, __float64__(x)` is `FCONST` or `MOV64` with the bit pattern.
- **Partial registers:** a64 `w` writes zero-extend like x86 32-bit writes, but there
  are no 8/16-bit partial writes: `mov al, x` / `setcc al` that keep rax's upper bits
  need `bfi`, or restructure.
- **Flags:** a64 `add`/`sub` don't set flags unless `adds`/`subs`; `inc`/`dec` don't
  exist. After `cmp a, b` the x86 condition names map directly: `jb`->`b.lo`,
  `jae`->`b.hs`, `ja`->`b.hi`, `jbe`->`b.ls`, `jl`->`b.lt`, `jge`->`b.ge`, `jg`->`b.gt`,
  `jle`->`b.le`, `js`->`b.mi`, `jns`->`b.pl`, `jo`->`b.vs`. The carry flag is inverted
  for subtraction (a64 C = no borrow): never carry an x86 `adc`/`sbb`/`CF` trick across
  without rethinking it. `test a, b` -> `tst`; `bt r, n` -> `tbz/tbnz`; `setcc` ->
  `cset`; `cmovcc` -> `csel`; `neg`/`not` -> `neg`/`mvn`.
- **Multiply/divide:** `mul r` (rdx:rax) -> `umulh x2, x9, r` then `mul x9, x9, r`
  (compute the high half first); `imul` 128-bit -> `smulh`. `div r` with rdx = 0 ->
  `udiv`, remainder `msub`; `cqo; idiv` -> `sdiv` + `msub`. a64 divide by zero returns 0
  instead of trapping; the engine never divides by zero anyway.
- **Shifts** by a register mask the count (mod 64 / mod 32) like x86. `rol` ->
  `ror` by (64 - n). `bswap` -> `rev`. `popcnt` -> `fmov d, x; cnt v.8b; addv b; fmov w`.
  `lzcnt` -> `clz`; `tzcnt`/`bsf` -> `rbit` + `clz` (zero: 64, like tzcnt).
- **String instructions:** `REP_MOVSB/MOVSD/MOVSQ`, `REP_STOSB/STOSD/STOSQ` keep x86's
  register roles (x0 = rdi, x1 = rsi, x3 = rcx, x9 = rax). Not for overlapping
  copies whose destination is under 16 bytes past the source.
- **Atomics and threads:** x86 is TSO; a64 is weakly ordered. Every hand-off the x86 code
  makes with plain stores plus a `lock` op or `xchg` needs explicit ordering: publish with
  `stlr` (or a `dmb ish` before the store), consume with `ldar` (or a `dmb ish` after the
  load). A `lock inc/xadd` -> an `ldaxr`/`stlxr` loop; `xchg [m], r` -> an
  `ldaxr`/`stlxr` swap loop followed by `dmb ish` where the x86 code relies on its full
  fence (a store followed by a load of another variable: the sleeping-flag handshakes).
  No LSE atomics (`ldadd`, `swp`, `cas`): the baseline is ARMv8.0.

## Floating point

- Scalar SSE2 maps 1:1: `addsd`/`subsd`/`mulsd`/`divsd`/`sqrtsd` -> `fadd`/`fsub`/`fmul`/
  `fdiv`/`fsqrt` on `d` registers, same rounding, same results. **Never** use
  `fmadd`/`fmsub`/`fnmadd` or any fused form: Rust doesn't contract `a*b+c`.
- `ucomisd a, b` / `comisd a, b` -> `fcmp da, db` (same operand order). The x86 jumps
  after it map by meaning, **not** by name, because x86 sets ZF/PF/CF on unordered:

  | x86 after ucomisd | meaning | a64 after fcmp |
  |---|---|---|
  | `jb` / `jc` | a < b or unordered | `b.lt` |
  | `jae` / `jnc` | a >= b, ordered | `b.ge` |
  | `ja` | a > b, ordered | `b.gt` |
  | `jbe` | a <= b or unordered | `b.le` |
  | `je` / `jz` | a == b **or unordered** | `b.eq` **and** `b.vs` |
  | `jne` / `jnz` | a != b, ordered | `b.vs` skips, then `b.ne` |
  | `jp` | unordered | `b.vs` |
  | `jnp` | ordered | `b.vc` |

  For `cmovcc`/`setcc` after `ucomisd` use the same meanings with `fcsel`/`csel`/`cset`
  (ordered less-than is `mi`, ordered <= is `ls`).
- `minsd a, b` is `a < b ? a : b` (the second operand on NaN or equal): `fcmp da, db;
  fcsel da, da, db, mi`. `maxsd a, b` is `a > b ? a : b`: `fcsel ..., gt`. But where the
  Rust source says `f64::min`/`f64::max`, the aarch64 oracle compiles them to
  `fminnm`/`fmaxnm` (NaN-ignoring, -0 < +0); use those. Check the oracle disassembly
  for `clamp`, `min`, `max`.
- Conversions: `cvtsi2sd` -> `scvtf d, x` (`scvtf d, w` for a 32-bit source).
  `cvttsd2si` -> `fcvtzs`, which **saturates and maps NaN to 0: exactly Rust's `as i64`
  / `as i32`** (x86's integer-indefinite result and the guards around it disappear;
  `F64_TO_I64` is just `fcvtzs`). `as u64` -> `fcvtzu`. `cvtsd2si` (MXCSR nearest-even)
  -> `fcvtns`. `roundsd` modes 0/1/2/3 -> `frintn`/`frintm`/`frintp`/`frintz`; Rust
  `f64::round` (half away from zero) -> `frinta`. When the x86 code depended on the
  indefinite value (`0x8000...`) as a sentinel, keep the sentinel explicitly.
- libm: the same calls as x86 (`pow`, `sin`, `cos`, `sincos`, `exp2`, `hypot`) through
  `CCALL`. `sincos(x, &s, &c)` takes its two out pointers in x0/x1. Always confirm the
  aarch64 oracle makes the same call (it lowers `powf(2.0)` to `fmul`, `powf(0.5)` to
  `fsqrt`, and pairs `sin`/`cos` into `sincos` like x86):
  `objdump -d --no-show-raw-insn target/oracle-prof/release/ttfx | awk '/<.*fn_name.*>:$/{p=1} p&&/^$/{p=0} p'`.
- Packed SSE (`paddd`, `pshufd`, `pcmpeqb`, `pmovmskb`, ...) -> NEON equivalents.
  `pmovmskb` has no single instruction: use a shift-narrow (`shrn`) or an AND with a
  bit-weight vector plus `addv`.

## SIMD tiers

aarch64 has one tier. Port the **TIER 1 (SSE2) path** of every `%if TIER` block: its
semantics are the reference. Where a TIER 2+ block exists only for speed, you may write
a NEON version instead if it is exact and simple; otherwise scalar is fine. AVX-512
paths (zmm, k registers, `vpcompress`, gathers, the RNG's 8-lane generator) are
dropped. `FLOORSD d` is `frintm`.

## Data and sections

| NASM | gas |
|---|---|
| `section .text` | `.text` |
| `section .rodata` | `.section .rodata` |
| `section .bss` | `.bss` |
| `section .tstate` | `TSTATE` (`.section .tstate, "aw", %nobits`) |
| `section .data.rel.ro ...` | `RELRO` |
| `align n` / `alignb n` | `.balign n` |
| `db` / `dw` / `dd` / `dq` | `.byte` / `.2byte` / `.4byte` / `.8byte` (`.quad`) |
| `dq 1.5` | `.double 1.5` |
| `resb n` / `resd n` / `resq n` | `.skip n` / `.skip 4*n` / `.skip 8*n` |
| `times n db x` | `.fill n, 1, x` |
| `%define X v` (file-local) | `.equ X, v` |
| `STR name, 27, "[0m"` | `STRING name, "\033[0m"` |
| `%rep` / `%assign` | `.rept` / `.set`, or `.irp` |
| `%macro` ... `%%l` | `.macro` ... `.Ll\@` |
| `struc` (file-local) | `.equ Name.field, offset` and `.equ Name_size, n` |

Keep every datum naturally aligned (8-byte values on 8, doubles on 8): the `:lo12:`
load/store relocations require it. `.tstate` is zeroed at the start of every run.

## Style

Comments as in the x86 file: the Rust function each routine transcribes, and why for
anything subtle. Keep the x86 header comments, with registers renamed. Short and
factual. Don't comment the translation itself ("was rax").

## Engine notes for effect porters

Signatures that differ from a plain "arguments in, x9 out" reading of the x86 headers
(each file's header comment is authoritative):

- Results in flags: `check_stop` (Z set = keep going), `group_key` (Z set = out of
  range), `pw_lowest` (Z set = no weight), `scene_append_recycled` (C set = reused:
  test `b.cs`/`b.eq`, not `b.lo`), `apply_cursor` (C set = unsupported),
  `is_plain_space` (Z).
- `gradient_capacity(x0=steps, x3=step count, x1=stop count)`; `gradient_map` takes
  its direction in x6; `utf8_decode(x1=ptr) -> w9 codepoint, w2 length`.
- `alloc` clobbers only x0, x9, x16. The syscall wrappers clobber x0.
- The batched motion blocks (`motion_batch`, `mv_*`) were TIER 3+ only and are not
  ported; `MV_P8`/`MV_P4` are unused.
- Conditional branches (`b.cond`, `cbz`, `tbz`) reach +-1 MB / +-32 KB: a conditional
  branch to another file's label can go out of range as the unit grows. Branch to a
  local label that does a plain `b`.

Porting an effect: replace the stub `effects/<name>.s`, define `.equ HAVE_<name>, 1`,
read the config words through `[effect_config]` in the order `src/asm/effects.rs`
marshals them, and run `tools/asm/oracle.sh <name> full` on a `cargo build --release`
until it passes. Engine files are shared: fix an engine bug only when the effect proves
it, faithfully to the x86 source, and keep the fix minimal.
