// Keep this stateless raster kernel separate from the effect engine's four
// tiers. A missing assembler/audit or another target retains the old Rust path.
use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(ttfx_bgra_asm)");
    println!("cargo::rerun-if-changed=asm/opaque-bgra.asm");
    println!("cargo::rerun-if-changed=../../ttfx-src/tools/asm/isa-audit.sh");
    println!("cargo::rerun-if-env-changed=NASM");
    println!("cargo::rerun-if-env-changed=TTFX_BGRA_ASM");
    // Build-time opt-out for equivalence/portability checks; no runtime setting.
    if env::var("TTFX_BGRA_ASM").as_deref() == Ok("0") {
        return;
    }
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64")
        || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux")
    {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let object = out.join("ttfx_bgra_opaque.o");
    let nasm = env::var_os("NASM").unwrap_or_else(|| "nasm".into());
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
