#!/usr/bin/env bash
# isa-audit.sh - check that each CPU tier's object uses only its tier's
# instructions (asm/PORTING.md "CPU tiers"): x86-64 v1 (SSE2), v2, v3, v4.
#
#   tools/asm/isa-audit.sh              assemble asm/lib.asm at TIER 1-4 (and
#                                       asm/tier.asm at the baseline) with line
#                                       info, list every instruction above its
#                                       tier as file:line, summarize per file
#   tools/asm/isa-audit.sh -q TIER OBJ  audit one built object (build.rs);
#                                       prints one summary line on failure
#
# Classification goes by encoding first (EVEX = v4, VEX = v3, VEX opmask = v4,
# REX2 = APX, legacy 0F 38 / 0F 3A maps = v2+), then by mnemonic for the
# legacy-encoded extensions (POPCNT, SSE3, LAHF, CMPXCHG16B = v2; LZCNT,
# TZCNT, MOVBE = v3) and by register class (ymm = v3; zmm, xmm16+, k = v4).
# FMA is rejected at every tier: its fused rounding breaks bit parity.
# Exit status 1 when anything is above its tier.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OBJDUMP=${OBJDUMP:-objdump}
NASM=${NASM:-nasm}

audit() {                               # audit TIER OBJECT QUIET
    # -d skips executable ranges marked OBJECT by NASM, including real code;
    # -D on .text decodes those ranges without misreading non-code sections.
    text_size=$("$OBJDUMP" -h "$2" | python3 -c '
import re, sys
sizes = [int(m.group(1), 16) for line in sys.stdin
         if (m := re.match(r"^\s*\d+\s+\.text\s+([0-9a-fA-F]+)\s", line))]
if len(sizes) != 1 or sizes[0] == 0:
    sys.exit("ISA audit: missing, duplicate, or empty .text section")
print(sizes[0])
')
    # -z ensures objdump shows zero-filled padding, which must be accounted
    # for in the byte coverage check rather than silently skipped.
    "$OBJDUMP" -D -j .text -z -l -w -M intel "$2" | python3 -c '
import re, sys
tier, obj, quiet, text_size = int(sys.argv[1]), sys.argv[2], sys.argv[3] == "1", int(sys.argv[4])
PREFIXES = {0x66, 0x67, 0xf2, 0xf3, 0x2e, 0x3e, 0x26, 0x36, 0x64, 0x65, 0xf0}
V2 = {"popcnt", "crc32", "lahf", "sahf", "cmpxchg16b", "addsubpd", "addsubps",
      "haddpd", "haddps", "hsubpd", "hsubps", "lddqu", "movddup", "movshdup",
      "movsldup", "fisttp", "monitor", "mwait"}
V3 = {"lzcnt", "tzcnt", "movbe"}
BEYOND = re.compile(r"^(adcx|adox|aes|vaes|pclmul|vpclmul|sha|gf2p8|vgf2p8|rdrand|rdseed|"
                    r"prefetchw|vpdp|vcvtne|tile|ld|vp2intersect|xsave|xrstor|clflushopt|clwb)")
# Match both three-operand FMA3 and four-operand FMA4, including scalar forms.
FMA = re.compile(r"^v?f(?:n)?m(?:add|sub)")
# Fail closed on new EVEX operations: an EVEX prefix alone does not imply the
# x86-64-v4 baseline (AVX512F/BW/CD/DQ/VL; not VBMI or later extensions).
# This is the reviewed mnemonic set of the current tier-4 assembly object,
# not a claim to enumerate every valid v4 instruction.
EVEX_V4 = set("""
vaddpd vbroadcastsd vcmpeqpd vcmpgt_oqpd vcmple_oqpd vcmplt_oqpd
vcmpordpd vcvtdq2pd vcvtpd2dq vcvttpd2dq vdivpd vextracti64x4
vmaxpd vminpd vmovapd vmovdqa32 vmovdqa64 vmovdqu32 vmovdqu64
vmovq vmovupd vmulpd vpabsd vpaddd vpaddq vpandd vpbroadcastb
vpbroadcastd vpbroadcastq vpcmpeqb vpcmpeqd vpcmpneqq vpermi2q
vpgatherdq vpmaxsd vpminud vpmovd2m vpmovq2m vpmovqd vpmovzxdq
vporq vprolq vpsllq vpsrld vpsrlq vpsubd vpternlogd vpternlogq
vptestmb vptestmd vptestmq vptestnmq vpunpckhqdq vpunpcklqdq
vpxord vpxorq vsubpd
""".split())
# The reviewed decoded mnemonic vocabulary of the current NASM engine, across
# all four tiers, plus baseline nop. A new mnemonic must be reviewed before
# linking: unknown legacy/XOP instructions must never default to tier 1.
KNOWN = set("""
adc add addpd addsd addss and andn andpd blsr bsf bsr bt btr bts
call cdqe clc cmova cmovae cmovb cmove cmovg cmovl cmovne cmovns
cmovp cmovs cmp comisd cpuid cqo cvtdq2pd cvtpd2dq cvtsd2si cvtsi2sd
cvtsi2ss cvttsd2si cvttss2si dec div divsd idiv imul inc ja jae
jb jbe je jg jge jl jle jmp jne jno jnp jns jo jp js kandb
kandnb kmovb kmovq kmovw knotb korb kortestb kortestq lea lock
lzcnt maxsd maxss minsd minss mov movabs movapd movd movdqa movdqu
movmskps movq movs movsd movss movsxd movups movzx mulpd mulsd
mulss neg nop not or paddd pause pcmpeqb pcmpeqd pmovmskb pop
popcnt por prefetcht0 pshufd pshuflw punpcklbw push pxor rep ret
rol ror roundsd sar setb sete setge setne shl shr shrx sqrtsd stc
stos sub subsd syscall test tzcnt ucomisd ud2 unpckhpd unpcklpd
vaddpd vandnpd vandpd vblendvpd vbroadcastsd vcmpeqpd
vcmpgt_oqpd vcmple_oqpd vcmplt_oqpd vcmpordpd vcvtdq2pd
vcvtpd2dq vcvttpd2dq vdivpd vextracti128 vextracti64x4
vmaskmovpd vmaxpd vminpd vmovapd vmovd vmovdqa vmovdqa32 vmovdqa64
vmovdqu vmovdqu32 vmovdqu64 vmovmskpd vmovmskps vmovq vmovupd
vmulpd vorpd vpabsd vpaddd vpaddq vpand vpandd vpandn
vpbroadcastb vpbroadcastd vpbroadcastq vpcmpeqb vpcmpeqd
vpcmpeqq vpcmpgtq vpcmpneqq vpermd vpermi2q vpgatherdq
vpmaskmovd vpmaxsd vpminsd vpminud vpmovd2m vpmovq2m vpmovqd
vpmovsxdq vpmovzxdq vpor vporq vprolq vpshufd vpsllq vpsrld
vpsrlq vpsubd vpternlogd vpternlogq vptest vptestmb vptestmd
vptestmq vptestnmq vpunpckhqdq vpunpcklqdq vpxor vpxord
vpxorq vsubpd vtestpd vxorpd vzeroupper xchg xgetbv xor xorpd xorps
""".split())
NAMES = {1: "v1", 2: "v2", 3: "v3", 4: "v4", 99: "beyond v4"}

def need(raw, mnem, ops):
    """(level, reason) for one instruction."""
    i = 0
    while i < len(raw) and raw[i] in PREFIXES:
        i += 1
    if i < len(raw) and 0x40 <= raw[i] <= 0x4f:
        i += 1
    if i >= len(raw):
        return 1, ""
    op = raw[i]
    if FMA.match(mnem):
        return 99, "FMA (fused rounding breaks bit parity)"
    if mnem not in KNOWN:
        return 99, "instruction mnemonic not reviewed for this engine"
    if BEYOND.match(mnem) and mnem != "lddqu":
        return 99, "not in x86-64-v4"
    if op == 0xd5:
        return 99, "REX2 (APX)"
    if op == 0x8f and i + 1 < len(raw) and raw[i + 1] & 0x1f >= 8:
        return 99, "XOP instruction requires a separate CPU feature"
    if 0xf3 in raw[:i] and raw[i:i+2] == b"\x0f\xae" and i + 2 < len(raw) and (raw[i+2] >> 3) & 7 <= 3:
        return 99, "FSGSBASE requires a separate CPU feature"
    if op == 0x62:
        if mnem not in EVEX_V4:
            return 99, "EVEX instruction not reviewed for x86-64-v4"
        return 4, "EVEX"
    if op in (0xc4, 0xc5):
        if mnem.startswith("k"):
            return 4, "opmask"
        if re.search(r"\bzmm|\b[xy]mm(1[6-9]|2\d|3[01])\b", ops):
            return 4, "EVEX register"
        return 3, "VEX"
    if re.search(r"\bzmm\d|\b[xy]mm(1[6-9]|2\d|3[01])\b|\{k[0-7]\}", ops):
        return 4, "AVX-512 register"
    if re.search(r"\bymm\d", ops):
        return 3, "ymm register"
    if mnem in V3:
        return 3, mnem
    if mnem in V2:
        return 2, mnem
    if op == 0x0f and i + 1 < len(raw) and raw[i + 1] in (0x38, 0x3a):
        return 2, "SSSE3/SSE4 opcode map"
    return 1, ""

where = "?"
fn = "?"
bad = []
decoded = 0
covered = 0
unparsed = []
for line in sys.stdin:
    line = line.rstrip("\n")
    m = re.search(r"(\S*asm/[\w/]+\.(?:asm|inc)):(\d+)", line)
    if m and not line.startswith(" "):
        where = re.sub(r".*asm//", "", m.group(1)) + ":" + m.group(2)
        continue
    m = re.match(r"^\s+([0-9a-f]+):\t([0-9a-f ]+)\t(\S+)\s*(.*)$", line)
    if not m:
        if re.match(r"^\s*[0-9a-f]+:\s", line):
            unparsed.append(line)
        m2 = re.match(r"^[0-9a-f]+ <(.*)>:$", line)
        if m2:
            fn = m2.group(1)
        continue
    raw = bytes(int(b, 16) for b in m.group(2).split())
    mnem, ops = m.group(3), m.group(4)
    offset = int(m.group(1), 16)
    if offset != covered:
        unparsed.append(f"disassembly gap at {offset:#x}, expected {covered:#x}")
    covered = offset + len(raw)
    if mnem == "(bad)" or mnem.startswith("."):
        # Objdump could not classify this opcode; it is not a tier-1 pass.
        unparsed.append(line)
        continue
    decoded += 1
    if mnem in ("rep", "repz", "repnz", "lock", "data16", "notrack", "bnd") and ops:
        mnem, _, ops = ops.partition(" ")
    level, reason = need(raw, mnem, ops)
    if level > tier:
        bad.append((where, fn, mnem, ops.strip(), level, reason))

if decoded == 0 or unparsed or covered != text_size:
    print(f"ISA audit incomplete: {decoded} decoded instructions, {len(unparsed)} unparsed lines, {covered}/{text_size} .text bytes covered", file=sys.stderr)
    for line in unparsed[:3]:
        print(f"  {line}", file=sys.stderr)
    sys.exit(2)
if not bad:
    sys.exit(0)
files = {}
for where, fn, *_ in bad:
    # objects without line info (build.rs) are summarized by function
    f = where.rsplit(":", 1)[0] if where != "?" else fn.split(".")[0]
    files[f] = files.get(f, 0) + 1
summary = ", ".join(f"{f} {n}" for f, n in sorted(files.items())[:12])
if len(files) > 12:
    summary += f", ... ({len(files)} in all)"
if quiet:
    print(f"{len(bad)} instruction(s) above tier {tier}: {summary}")
    for where, fn, mnem, ops, level, reason in bad[:5]:
        print(f"  {where}: {mnem} {ops} [{NAMES[level]}: {reason}; in {fn}]")
    sys.exit(1)
print(f"== tier {tier} ({obj}): {len(bad)} instruction(s) above x86-64-{NAMES[tier]}")
for where, fn, mnem, ops, level, reason in bad:
    print(f"  {where}: {mnem} {ops}   [{NAMES[level]}: {reason}; in {fn}]")
print(f"  per file: {summary}")
sys.exit(1)
' "$1" "$2" "$3" "$text_size"
}

if [ "${1:-}" = "-q" ]; then
    audit "$2" "$3" 1
    exit
fi

OUT="$ROOT/target/isa-audit"
mkdir -p "$OUT"
cd "$ROOT"
: > "$OUT/test_thunks.inc"
status=0
for t in 1 2 3 4; do
    if ! "$NASM" -f elf64 -O3 -g -F dwarf -I asm/ -DTIER=$t -DTTFX_NO_CPU_CHECK \
        -o "$OUT/lib-v$t.o" asm/lib.asm 2> "$OUT/nasm-v$t.log"; then
        echo "== tier $t: NASM failed"; grep error "$OUT/nasm-v$t.log" | head -20
        status=1
        continue
    fi
    if audit $t "$OUT/lib-v$t.o" 0; then
        echo "== tier $t: clean"
    else
        status=1
    fi
done
"$NASM" -f elf64 -O3 -g -F dwarf -I asm/ -I "$OUT/" -DTIER=1 -o "$OUT/tier.o" asm/tier.asm
if audit 1 "$OUT/tier.o" 0; then echo "== tier.asm (baseline): clean"; else status=1; fi
exit $status
