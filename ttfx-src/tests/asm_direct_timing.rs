#![cfg(target_arch = "x86_64")]
use clap::Parser;
use std::ffi::c_void;
use ttfx::{asm::{self, DirectFrameHook}, cli::Cli, engine::{effect::RunOutcome, terminal::TerminalConfig}, utils::rng::Rng};

unsafe extern "C" fn capture(ctx: *mut c_void, _: u64, _: u64, _: *const u32, _: *const u8) -> u64 {
    let samples = unsafe { &mut *(ctx as *mut Vec<Option<[u64; 5]>>) };
    samples.push(asm::direct_timing_snapshot());
    0
}
#[test]
fn timing_observer_is_scoped_and_observes_real_wait_without_changing_run_outcome() {
    if asm::tier() == 0 { panic!("esta prueba requiere NASM y ASM x86 habilitado") }
    assert!(asm::direct_timing_snapshot().is_none());
    let effect = Cli::try_parse_from(["ttfx", "matrix"]).unwrap().effect.unwrap();
    let config = TerminalConfig { canvas_width:40, canvas_height:24, frame_rate:60,
        ignore_terminal_dimensions:true, ..TerminalConfig::default() };
    for enabled in [false, true] {
        std::env::set_var("TTFX_ASM_TIMING", if enabled { "1" } else { "0" });
        let mut samples: Vec<Option<[u64; 5]>> = Vec::new();
        let mut rng = Rng::seeded(12345);
        let result = unsafe { asm::run_direct(asm::Run {
            effect:&effect, input:"OMARCHY", config:&config, rng:&mut rng,
            parity_dump:false, virtual_clock:false, max_frames:Some(4), tty_output:false, audio:None,
        }, DirectFrameHook {emit:capture, ctx:(&mut samples as *mut Vec<Option<[u64;5]>>).cast()}) }.unwrap();
        assert_eq!(result, RunOutcome::Complete);
        assert_eq!(samples.len(), 4);
        for raw in samples {
            let v = raw.expect("getter del tier activo disponible dentro del callback");
            if enabled {
                assert!(v[0]>0 && v[1]>=v[0] && v[3]>=v[1] && v[4]>=v[3]);
                assert!(v[2]<=16_666_666);
                assert!(v[3]-v[1]>=v[2]);
            } else { assert_eq!(v, [0;5]); }
        }
        assert!(asm::direct_timing_snapshot().is_none());
    }
    std::env::remove_var("TTFX_ASM_TIMING");
}
