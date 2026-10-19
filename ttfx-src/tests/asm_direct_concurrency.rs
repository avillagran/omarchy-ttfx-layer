#![cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]

use clap::Parser;
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use ttfx::asm::{self, DirectFrameHook};
use ttfx::cli::Cli;
use ttfx::engine::effect::RunOutcome;
use ttfx::engine::terminal::TerminalConfig;
use ttfx::utils::rng::Rng;

struct HeldFrame {
    entered: Sender<()>,
    release: Receiver<()>,
    pointer_stable: bool,
}

unsafe extern "C" fn hold_borrowed_frame(
    ctx: *mut c_void,
    cols: u64,
    rows: u64,
    grid: *const u32,
    pool: *const u8,
) -> u64 {
    // SAFETY: each run exclusively owns its boxed context and these pointers
    // remain borrowed/valid until this synchronous callback returns.
    let state = unsafe { &mut *(ctx as *mut HeldFrame) };
    if cols != 236 || rows != 78 || grid.is_null() || pool.is_null() {
        state.pointer_stable = false;
        return 1;
    }
    // SAFETY: the hook contract guarantees a readable grid and its pool's
    // initial 32-byte visual header for the entire callback.
    let handles = unsafe { std::slice::from_raw_parts(grid, (cols * rows) as usize) };
    let nonzero = handles.iter().position(|&handle| handle != 0).unwrap_or(0);
    let probes = [0, handles.len() / 2, nonzero, handles.len() - 1];
    let before = probes.map(|index| handles[index]);
    let pool_before = unsafe { std::slice::from_raw_parts(pool, 32) }.to_vec();
    if state.entered.send(()).is_err()
        || state.release.recv_timeout(Duration::from_secs(3)).is_err()
    {
        state.pointer_stable = false;
        return 1;
    }
    state.pointer_stable = probes.map(|index| handles[index]) == before
        && unsafe { std::slice::from_raw_parts(pool, 32) } == pool_before.as_slice();
    1
}

unsafe extern "C" fn note_second_frame(
    ctx: *mut c_void,
    _cols: u64,
    _rows: u64,
    _grid: *const u32,
    _pool: *const u8,
) -> u64 {
    // SAFETY: the second run exclusively owns its boxed sender until return.
    let sender = unsafe { &mut *(ctx as *mut Sender<()>) };
    let _ = sender.send(());
    1
}

fn run_one(hook: DirectFrameHook) -> RunOutcome {
    let effect = Cli::try_parse_from(["ttfx", "thunderstorm"])
        .unwrap()
        .effect
        .unwrap();
    let config = TerminalConfig {
        canvas_width: 236,
        canvas_height: 78,
        frame_rate: 60,
        ignore_terminal_dimensions: true,
        ..TerminalConfig::default()
    };
    let mut rng = Rng::seeded(7);
    // SAFETY: each caller owns its hook context until this synchronous run
    // completes; neither callback retains an assembly-owned frame pointer.
    unsafe {
        asm::run_direct(
            asm::Run {
                effect: &effect,
                input: "THUNDERSTORM",
                config: &config,
                rng: &mut rng,
                parity_dump: false,
                virtual_clock: true,
                max_frames: Some(10),
                tty_output: false,
                audio: None,
            },
            hook,
        )
    }
    .unwrap()
}

#[test]
fn concurrent_direct_runs_do_not_invalidate_borrowed_frames() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first = std::thread::spawn(move || {
        let mut state = Box::new(HeldFrame {
            entered: entered_tx,
            release: release_rx,
            pointer_stable: true,
        });
        let outcome = run_one(DirectFrameHook {
            emit: hold_borrowed_frame,
            ctx: (&mut *state as *mut HeldFrame).cast(),
        });
        (outcome, state.pointer_stable)
    });
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();

    let (attempt_tx, attempt_rx) = mpsc::channel();
    let (second_frame_tx, second_frame_rx) = mpsc::channel();
    let second = std::thread::spawn(move || {
        let mut sender = Box::new(second_frame_tx);
        attempt_tx.send(()).unwrap();
        run_one(DirectFrameHook {
            emit: note_second_frame,
            ctx: (&mut *sender as *mut Sender<()>).cast(),
        })
    });
    attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let premature_second_frame = second_frame_rx
        .recv_timeout(Duration::from_millis(100))
        .is_ok();
    release_tx.send(()).unwrap();
    let (first_outcome, pointer_stable) = first.join().unwrap();
    let second_outcome = second.join().unwrap();
    assert!(
        !premature_second_frame,
        "second run entered while first frame was borrowed"
    );
    assert!(
        pointer_stable,
        "borrowed frame changed while callback was active"
    );
    assert_eq!(first_outcome, RunOutcome::Terminated);
    assert_eq!(second_outcome, RunOutcome::Terminated);
}

#[test]
fn ordinary_asm_offer_waits_for_direct_borrowed_frame() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first = std::thread::spawn(move || {
        let mut state = Box::new(HeldFrame {
            entered: entered_tx,
            release: release_rx,
            pointer_stable: true,
        });
        let outcome = run_one(DirectFrameHook {
            emit: hold_borrowed_frame,
            ctx: (&mut *state as *mut HeldFrame).cast(),
        });
        (outcome, state.pointer_stable)
    });
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();

    let (attempt_tx, attempt_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let second = std::thread::spawn(move || {
        let effect = Cli::try_parse_from(["ttfx", "thunderstorm"])
            .unwrap()
            .effect
            .unwrap();
        let config = TerminalConfig {
            canvas_width: 236,
            canvas_height: 78,
            frame_rate: 60,
            ignore_terminal_dimensions: true,
            ..TerminalConfig::default()
        };
        let mut rng = Rng::seeded(9);
        attempt_tx.send(()).unwrap();
        let outcome = asm::try_run(asm::Run {
            effect: &effect,
            input: "THUNDERSTORM",
            config: &config,
            rng: &mut rng,
            parity_dump: false,
            virtual_clock: true,
            max_frames: Some(10),
            tty_output: false,
            audio: None,
        });
        finished_tx.send(()).unwrap();
        outcome
    });
    attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let completed_while_borrowed = finished_rx.recv_timeout(Duration::from_millis(250)).is_ok();
    release_tx.send(()).unwrap();
    let (first_outcome, pointer_stable) = first.join().unwrap();
    let second_outcome = second.join().unwrap();
    assert!(
        !completed_while_borrowed,
        "ordinary offer completed during direct callback"
    );
    assert!(
        pointer_stable,
        "borrowed grid/pool changed during ordinary offer"
    );
    assert_eq!(first_outcome, RunOutcome::Terminated);
    assert!(
        matches!(second_outcome, Some(Ok(_))),
        "ordinary run must use ASM"
    );
}
