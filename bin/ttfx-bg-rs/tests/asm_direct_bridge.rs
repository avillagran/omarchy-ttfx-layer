use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use ttfx_bg_rs::asm_direct_bridge::{decode_frame, emit_frame, DirectCell, FrameSink};

#[path = "../src/frame_handoff.rs"]
mod frame_handoff;

fn visual(pool: &mut [u8], offset: usize, ch: char, fg: u64, bg: u64, attrs: u64) -> u32 {
    let mut bytes = [0u8; 4];
    let len = ch.encode_utf8(&mut bytes).len();
    let symbol = u32::from_le_bytes(bytes) as u64 | ((len as u64) << 32);
    for (position, value) in [
        (offset - 32, symbol),
        (offset - 24, fg),
        (offset - 16, bg),
        (offset - 8, attrs),
    ] {
        pool[position..position + 8].copy_from_slice(&value.to_le_bytes());
    }
    offset as u32 | ((len as u32) << 24)
}

fn sink(dimensions: (usize, usize)) -> FrameSink {
    FrameSink::new(
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicBool::new(false)),
        dimensions,
    )
}

#[test]
fn decodes_borrowed_asm_grid_into_owned_sparse_top_origin_cells() {
    let mut pool = vec![0u8; 256];
    let space = visual(&mut pool, 32, ' ', u64::MAX, u64::MAX, 0);
    let lightning = visual(&mut pool, 96, '⚡', 0xaaccff, u64::MAX, 0);
    let reverse = visual(&mut pool, 160, 'é', 0xff0000, 0x1177aa, 16);
    // Producer row zero is the bottom of the frame.
    let grid = [space, lightning, space, space, space, reverse];
    let mut frame = Vec::new();
    unsafe { decode_frame(3, 2, grid.as_ptr(), pool.as_ptr(), &mut frame).unwrap() };
    assert_eq!(
        frame,
        vec![
            DirectCell {
                x: 1,
                y: 1,
                ch: '⚡',
                color: (0xaa, 0xcc, 0xff),
                background: None,
                attrs: 0,
            },
            DirectCell {
                x: 2,
                y: 0,
                ch: 'é',
                color: (0x11, 0x77, 0xaa),
                background: Some((0xff, 0, 0)),
                attrs: 16,
            },
        ]
    );
    let mut context = sink((3, 2));
    // SAFETY: context is exclusively borrowed; grid/pool and all referenced
    // visual headers live through this synchronous callback.
    assert_eq!(
        unsafe {
            emit_frame(
                (&mut context as *mut FrameSink).cast::<c_void>(),
                3,
                2,
                grid.as_ptr(),
                pool.as_ptr(),
            )
        },
        0
    );
    assert_eq!(context.generation.load(Ordering::Acquire), 1);
    assert_eq!(*context.snapshot.lock().unwrap(), frame);
    pool.fill(0); // Prove the published frame owns its decoded cells.
    assert_eq!(*context.snapshot.lock().unwrap(), frame);
}

#[test]
fn retains_underlined_and_struck_spaces_without_a_background() {
    let mut pool = vec![0; 256];
    let underline = visual(&mut pool, 32, ' ', 0x102030, u64::MAX, 4);
    let strike = visual(&mut pool, 96, ' ', 0x405060, u64::MAX, 64);
    let plain = visual(&mut pool, 160, ' ', 0xabcdef, u64::MAX, 0);
    let mut frame = Vec::new();
    unsafe {
        decode_frame(
            3,
            1,
            [underline, strike, plain].as_ptr(),
            pool.as_ptr(),
            &mut frame,
        )
        .unwrap()
    };
    assert_eq!(
        frame,
        vec![
            DirectCell {
                x: 0,
                y: 0,
                ch: ' ',
                color: (0x10, 0x20, 0x30),
                background: None,
                attrs: 4,
            },
            DirectCell {
                x: 1,
                y: 0,
                ch: ' ',
                color: (0x40, 0x50, 0x60),
                background: None,
                attrs: 64,
            },
        ]
    );
}

#[test]
fn all_default_blank_handles_decode_to_an_empty_sparse_frame() {
    let mut pool = vec![0; 64];
    let blank = visual(&mut pool, 32, ' ', u64::MAX, u64::MAX, 0);
    let mut frame = Vec::new();
    let grid = vec![blank; 236 * 78];
    unsafe { decode_frame(236, 78, grid.as_ptr(), pool.as_ptr(), &mut frame).unwrap() };
    assert!(frame.is_empty());
}

#[test]
fn retains_colored_spaces_and_swaps_reverse_default_colors() {
    let mut pool = vec![0; 256];
    let blank = visual(&mut pool, 32, ' ', u64::MAX, u64::MAX, 0);
    let colored_space = visual(&mut pool, 96, ' ', 0xabcdef, 0x123456, 0);
    let reversed = visual(&mut pool, 160, 'R', u64::MAX, u64::MAX, 16 | 1 | 2 | 4 | 64);
    let mut frame = Vec::new();
    unsafe {
        decode_frame(
            3,
            1,
            [blank, colored_space, reversed].as_ptr(),
            pool.as_ptr(),
            &mut frame,
        )
        .unwrap()
    };
    assert_eq!(frame.len(), 2);
    assert_eq!(
        (frame[0].ch, frame[0].background),
        (' ', Some((0x12, 0x34, 0x56)))
    );
    assert_eq!(
        (frame[1].color, frame[1].background, frame[1].attrs),
        ((0, 0, 0), Some((0xcc, 0xcc, 0xcc)), 16 | 1 | 2 | 4 | 64)
    );
}

#[test]
fn sink_maps_both_colors_before_publication_without_mutating_baseline() {
    let mut pool = vec![0; 96];
    let handle = visual(&mut pool, 32, 'A', 0x102030, 0x405060, 0);
    let mut context = sink((1, 1));
    let ctx = (&mut context as *mut FrameSink).cast::<c_void>();
    // SAFETY: ctx is exclusive and both frame buffers remain live for these
    // synchronous calls; every handle refers to the allocated pool.
    assert_eq!(
        unsafe { emit_frame(ctx, 1, 1, [handle].as_ptr(), pool.as_ptr()) },
        0
    );
    let original = context.snapshot.lock().unwrap()[0].clone();
    assert_eq!(original.color, (0x10, 0x20, 0x30));
    assert_eq!(original.background, Some((0x40, 0x50, 0x60)));
    context.color_transform = Some(std::rc::Rc::new(|r, g, b| (b, g, r)));
    assert_eq!(
        unsafe { emit_frame(ctx, 1, 1, [handle].as_ptr(), pool.as_ptr()) },
        0
    );
    let mapped = &context.snapshot.lock().unwrap()[0];
    assert_eq!(mapped.color, (0x30, 0x20, 0x10));
    assert_eq!(mapped.background, Some((0x60, 0x50, 0x40)));
    assert_eq!(context.generation.load(Ordering::Acquire), 2);
    assert_eq!(original.color, (0x10, 0x20, 0x30));
}

#[test]
fn rejects_unrenderable_attributes_instead_of_dropping_them() {
    let mut pool = vec![0; 160];
    let blink = visual(&mut pool, 32, 'B', 0xffffff, u64::MAX, 8);
    let mut cells = Vec::new();
    assert_eq!(
        unsafe { decode_frame(1, 1, [blink].as_ptr(), pool.as_ptr(), &mut cells) },
        Err("blink requires GTK redraw scheduling")
    );
    let unknown = visual(&mut pool, 96, 'U', 0xffffff, u64::MAX, 128);
    assert_eq!(
        unsafe { decode_frame(1, 1, [unknown].as_ptr(), pool.as_ptr(), &mut cells) },
        Err("unsupported visual attribute bits")
    );
}

#[test]
fn rejects_invalid_dimensions_symbols_handles_and_stop_without_publication() {
    let mut pool = vec![0u8; 128];
    let valid = visual(&mut pool, 32, 'A', 0x010203, u64::MAX, 0);
    let mut context = sink((2, 1));
    let ctx = (&mut context as *mut FrameSink).cast::<c_void>();
    let grid = [valid, valid];
    // SAFETY: ctx, grid and pool live for every call. Null grid is rejected
    // before dereference; malformed handles are rejected before pool access.
    assert_eq!(
        unsafe { emit_frame(ctx, 0, 1, grid.as_ptr(), pool.as_ptr()) },
        1
    );
    assert_eq!(
        unsafe { emit_frame(ctx, 2, 1, std::ptr::null(), pool.as_ptr()) },
        1
    );
    assert_eq!(
        unsafe { emit_frame(ctx, 2, 1, [1u32, valid].as_ptr(), pool.as_ptr()) },
        1
    );
    assert_eq!(
        unsafe { emit_frame(ctx, 2, 1, [0xff00_0020u32, valid].as_ptr(), pool.as_ptr()) },
        1
    );
    assert_eq!(
        context.error,
        Some("visual handle offset or length out of range")
    );
    context.error = None;
    pool[0..8].copy_from_slice(&0u64.to_le_bytes());
    let malformed = [(32, 0u64), (32, (1u64 << 32) | 0xffu64)];
    for (offset, value) in malformed {
        pool[offset - 32..offset - 24].copy_from_slice(&value.to_le_bytes());
        assert!(
            unsafe { decode_frame(1, 1, [valid].as_ptr(), pool.as_ptr(), &mut Vec::new()) }
                .is_err()
        );
    }
    assert_eq!(context.generation.load(Ordering::Acquire), 0);
    context.stop.store(true, Ordering::Relaxed);
    assert_eq!(
        unsafe { emit_frame(ctx, 2, 1, grid.as_ptr(), pool.as_ptr()) },
        1
    );
    assert_eq!(context.generation.load(Ordering::Acquire), 0);
    assert!(unsafe {
        decode_frame(
            u64::MAX,
            u64::MAX,
            grid.as_ptr(),
            pool.as_ptr(),
            &mut Vec::new(),
        )
    }
    .is_err());
}

#[test]
fn direct_thunderstorm_uses_asm_frames_or_reports_the_architecture_blocker() {
    use clap::Parser;
    use ttfx::asm::{self, DirectFrameHook};
    use ttfx::cli::Cli;
    use ttfx::engine::effect::RunOutcome;
    use ttfx::engine::terminal::TerminalConfig;
    use ttfx::utils::rng::Rng;

    let effect = Cli::try_parse_from(["ttfx", "thunderstorm"])
        .unwrap()
        .effect
        .unwrap();
    let config = TerminalConfig {
        canvas_width: 80,
        canvas_height: 30,
        ignore_terminal_dimensions: true,
        frame_rate: 60,
        ..TerminalConfig::default()
    };
    let mut rng = Rng::seeded(5);
    let mut capture = sink((80, 30));
    let hook = DirectFrameHook {
        emit: emit_frame,
        ctx: (&mut capture as *mut FrameSink).cast(),
    };
    // SAFETY: capture remains live and exclusive for the synchronous run;
    // emit_frame consumes ASM-owned frame storage without retaining pointers.
    let result = unsafe {
        asm::run_direct(
            asm::Run {
                effect: &effect,
                input: "THUNDERSTORM",
                config: &config,
                rng: &mut rng,
                parity_dump: false,
                virtual_clock: true,
                max_frames: Some(3),
                tty_output: false,
                audio: None,
            },
            hook,
        )
    };
    if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) && asm::tier() != 0 {
        assert_eq!(result.unwrap(), RunOutcome::Complete);
        assert_eq!(capture.generation.load(Ordering::Acquire), 3);
        assert!(capture.error.is_none());
        assert!(!capture.snapshot.lock().unwrap().is_empty());
    } else {
        let error = result.unwrap_err().to_string();
        assert!(error.contains("assembly engine"), "{error}");
        assert_eq!(capture.generation.load(Ordering::Acquire), 0);
    }
}

#[test]
fn latest_frame_replaces_prior_without_retain_of_asm_memory() {
    let mut pool = vec![0u8; 160];
    let a = visual(&mut pool, 32, 'A', 0x112233, u64::MAX, 0);
    let b = visual(&mut pool, 96, 'B', 0x445566, u64::MAX, 0);
    let mut context = sink((1, 1));
    let ctx = (&mut context as *mut FrameSink).cast::<c_void>();
    // SAFETY: context is exclusively borrowed and both grid temporaries and
    // referenced visual bytes remain valid through each synchronous call.
    assert_eq!(
        unsafe { emit_frame(ctx, 1, 1, [a].as_ptr(), pool.as_ptr()) },
        0
    );
    assert_eq!(
        unsafe { emit_frame(ctx, 1, 1, [b].as_ptr(), pool.as_ptr()) },
        0
    );
    assert_eq!(context.generation.load(Ordering::Acquire), 2);
    assert_eq!(context.snapshot.lock().unwrap()[0].ch, 'B');
}

#[test]
fn publication_reuses_buffers_without_mutating_visible_or_committing_partial_frames() {
    use std::cell::Cell;

    let mut pool = vec![0u8; 224];
    let a = visual(&mut pool, 32, 'A', 0x112233, 0x445566, 1);
    let b = visual(&mut pool, 96, 'B', 0x778899, u64::MAX, 0);
    let mut context = sink((3, 1));
    let snapshot = context.snapshot.clone();
    let generation = context.generation.clone();
    let publish = |context: &mut FrameSink, grid: [u32; 3], pool: &[u8]| {
        // SAFETY: context has a single producer, each grid has the declared
        // three cells, and every valid handle's header lives in pool until the
        // synchronous callback returns. Malformed handles fail before access.
        unsafe {
            emit_frame(
                (context as *mut FrameSink).cast(),
                3,
                1,
                grid.as_ptr(),
                pool.as_ptr(),
            )
        }
    };
    let consume = |visible: &mut Vec<DirectCell>, consumed: &Cell<u64>| {
        frame_handoff::consume_latest(&snapshot, &generation, consumed, visible, false)
    };
    let cell = |x, ch, color, background, attrs| DirectCell {
        x,
        y: 0,
        ch,
        color,
        background,
        attrs,
    };
    let first = vec![cell(
        0,
        'A',
        (0x11, 0x22, 0x33),
        Some((0x44, 0x55, 0x66)),
        1,
    )];
    let mut visible = Vec::new();
    let consumed = Cell::new(0);

    assert_eq!(publish(&mut context, [a, 0, 0], &pool), 0);
    assert_eq!(
        consume(&mut visible, &consumed),
        frame_handoff::FrameUpdate {
            new_frame: true,
            redraw: true,
        }
    );
    assert_eq!(visible, first);
    assert_eq!(consumed.get(), 1);

    // The producer overtakes the consumer while the first frame is visible.
    assert_eq!(publish(&mut context, [0, b, 0], &pool), 0);
    assert_eq!(visible, first);
    visual(&mut pool, 32, 'A', 0xaabbcc, 0xddeeff, 1);
    context.color_transform = Some(std::rc::Rc::new(|r, g, b| (b, g, r)));
    assert_eq!(publish(&mut context, [0, 0, a], &pool), 0);
    let recolored = vec![cell(
        2,
        'A',
        (0xcc, 0xbb, 0xaa),
        Some((0xff, 0xee, 0xdd)),
        1,
    )];
    assert_eq!(*context.snapshot.lock().unwrap(), recolored);
    assert_eq!(context.generation.load(Ordering::Acquire), 3);
    assert_eq!(visible, first);
    assert!(consume(&mut visible, &consumed).redraw);
    assert_eq!(visible, recolored);
    assert_eq!(consumed.get(), 3);

    // An identical publication advances generation while retaining the visible
    // frame; subsequent producer reuse must still leave that frame intact.
    assert_eq!(publish(&mut context, [0, 0, a], &pool), 0);
    assert_eq!(
        consume(&mut visible, &consumed),
        frame_handoff::FrameUpdate {
            new_frame: true,
            redraw: false,
        }
    );
    assert_eq!(consumed.get(), 4);
    assert_eq!(visible, recolored);
    assert_eq!(publish(&mut context, [b, 0, 0], &pool), 0);
    assert_eq!(visible, recolored);
    assert_eq!(publish(&mut context, [0, 0, 0], &pool), 0);
    assert_eq!(context.generation.load(Ordering::Acquire), 6);
    assert!(context.snapshot.lock().unwrap().is_empty());
    assert_eq!(visible, recolored);
    assert!(consume(&mut visible, &consumed).redraw);
    assert_eq!(consumed.get(), 6);
    assert!(visible.is_empty());

    let last = vec![
        cell(0, 'A', (0xcc, 0xbb, 0xaa), Some((0xff, 0xee, 0xdd)), 1),
        cell(1, 'B', (0x99, 0x88, 0x77), None, 0),
    ];
    assert_eq!(publish(&mut context, [a, b, 0], &pool), 0);
    assert_eq!(*context.snapshot.lock().unwrap(), last);
    assert_eq!(context.generation.load(Ordering::Acquire), 7);
    // The first cell decodes successfully before the second cell fails. This
    // partially filled scratch buffer must never replace the last publication.
    assert_eq!(publish(&mut context, [a, 1, 0], &pool), 1);
    assert_eq!(
        context.error,
        Some("visual handle offset or length out of range")
    );
    assert_eq!(*context.snapshot.lock().unwrap(), last);
    assert_eq!(context.generation.load(Ordering::Acquire), 7);
    assert!(visible.is_empty());
    assert!(consume(&mut visible, &consumed).redraw);
    assert_eq!(consumed.get(), 7);
    assert_eq!(visible, last);
}

#[test]
fn panicking_color_transform_stops_without_unwinding_through_c() {
    let mut pool = vec![0u8; 64];
    let handle = visual(&mut pool, 32, 'A', 0x112233, u64::MAX, 0);
    let mut context = sink((1, 1));
    context.color_transform = Some(std::rc::Rc::new(|_, _, _| panic!("transform failure")));
    // SAFETY: context is exclusively borrowed and the stack grid and pool
    // remain valid for the entire synchronous call.
    let result = unsafe {
        emit_frame(
            (&mut context as *mut FrameSink).cast(),
            1,
            1,
            [handle].as_ptr(),
            pool.as_ptr(),
        )
    };
    assert_eq!(result, 1);
    assert_eq!(context.error, Some("assembly frame callback panicked"));
    assert_eq!(context.generation.load(Ordering::Acquire), 0);
}
