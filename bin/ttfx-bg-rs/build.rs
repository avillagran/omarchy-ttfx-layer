// Keep this stateless raster kernel separate from the effect engine's four
// tiers. A missing assembler/audit or another target retains the old Rust path.
use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(ttfx_bgra_asm)");
    println!("cargo::rerun-if-changed=asm/opaque-bgra.asm");
    println!("cargo::rerun-if-changed=../../ttfx-src/tools/asm/isa-audit.sh");
    println!("cargo::rerun-if-env-changed=NASM");
    println!("cargo::rerun-if-env-changed=TTFX_BGRA_ASM");
    println!("cargo::rerun-if-env-changed=TTFX_ALLOW_NO_ASM");
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64")
        || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux")
    {
        return;
    }
    // The effect engine needs NASM on x86_64. Without it ttfx-src quietly
    // builds its Rust engine, and the direct renderer then draws nothing.
    let nasm = env::var_os("NASM").unwrap_or_else(|| "nasm".into());
    let nasm_runs = Command::new(&nasm).arg("-v").output().is_ok_and(|r| r.status.success());
    if !nasm_runs && env::var("TTFX_ALLOW_NO_ASM").as_deref() != Ok("1") {
        panic!(
            "ttfx-bg-rs on x86_64 needs NASM >= 3.0 (pacman -S nasm, or NASM=/path/to/nasm); \
             set TTFX_ALLOW_NO_ASM=1 to build without the effect engine anyway"
        );
    }
    // Build-time opt-out for equivalence/portability checks; no runtime setting.
    if env::var("TTFX_BGRA_ASM").as_deref() == Ok("0") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let object = out.join("ttfx_bgra_opaque.o");
    let assembled = Command::new(nasm)
        .args(["-f", "elf64", "-O3", "-o"])
        .arg(&object)
        .arg("asm/opaque-bgra.asm")
        .output();
    let audited = assembled.is_ok_and(|r| r.status.success())
        && Command::new("bash")
            .args(["../../ttfx-src/tools/asm/isa-audit.sh", "-q", "1"])
            .arg(&object)
            .output()
            .is_ok_and(|r| r.status.success());
    if !audited {
        println!(
            "cargo::warning=BGRA ASM unavailable or ISA1 audit failed; retaining Rust raster loop"
        );
        return;
    }
    let archived = Command::new("ar")
        .arg("crs")
        .arg(out.join("libttfx_bgra_opaque.a"))
        .arg(object)
        .status()
        .is_ok_and(|r| r.success());
    if !archived {
        println!("cargo::warning=BGRA ASM archive unavailable; retaining Rust raster loop");
        return;
    }
    println!("cargo::rustc-link-search=native={}", out.display());
    println!("cargo::rustc-link-lib=static=ttfx_bgra_opaque");
    println!("cargo::rustc-cfg=ttfx_bgra_asm");
}
