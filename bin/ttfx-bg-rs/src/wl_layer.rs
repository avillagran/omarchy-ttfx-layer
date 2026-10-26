//! Background layer drawn directly on Wayland, without GTK.
//!
//! GTK reports the whole window as damaged on every frame (both its Vulkan and
//! GL renderers on mbp-intel), so Hyprland recomposites the full screen 30
//! times a second even when less than 1% of the cells change. This layer owns
//! its wl_surface: it redraws the frame with the GPU pipeline and reports only
//! the changed rectangles through eglSwapBuffersWithDamage. The surface is also
//! marked opaque so the compositor can skip what lies beneath it.
use std::ffi::{c_char, c_void, CString};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_callback::{self, WlCallback},
    wl_compositor::WlCompositor,
    wl_output::{self, WlOutput},
    wl_region::WlRegion,
    wl_registry::WlRegistry,
    wl_surface::WlSurface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

use crate::asm_direct_bridge::DirectCell;
use crate::frame_handoff;
use crate::presentation_probe::Probe;
use serde_json::json;
use wayland_protocols::wp::presentation_time::client::{
    wp_presentation::{self, WpPresentation},
    wp_presentation_feedback::{self, WpPresentationFeedback},
};
use crate::frame_wakeup::FrameWakeup;
use crate::gpu_grid::{GpuGrid, GpuPipeline};

// --- Minimal EGL / wayland-egl bindings --------------------------------------

type EglDisplay = *mut c_void;
type EglConfig = *mut c_void;
type EglContext = *mut c_void;
type EglSurface = *mut c_void;

const EGL_PLATFORM_WAYLAND_KHR: u32 = 0x31D8;
const EGL_OPENGL_ES_API: u32 = 0x30A0;
const EGL_NONE: i32 = 0x3038;
const EGL_SURFACE_TYPE: i32 = 0x3033;
const EGL_WINDOW_BIT: i32 = 0x0004;
const EGL_RENDERABLE_TYPE: i32 = 0x3040;
const EGL_OPENGL_ES3_BIT: i32 = 0x0040;
const EGL_RED_SIZE: i32 = 0x3024;
const EGL_GREEN_SIZE: i32 = 0x3023;
const EGL_BLUE_SIZE: i32 = 0x3022;
const EGL_ALPHA_SIZE: i32 = 0x3021;
const EGL_CONTEXT_MAJOR_VERSION: i32 = 0x3098;
const EGL_BUFFER_AGE_EXT: i32 = 0x313D;

#[link(name = "EGL")]
extern "C" {
    fn eglGetPlatformDisplay(platform: u32, native: *mut c_void, attribs: *const isize) -> EglDisplay;
    fn eglInitialize(display: EglDisplay, major: *mut i32, minor: *mut i32) -> u32;
    fn eglBindAPI(api: u32) -> u32;
    fn eglChooseConfig(display: EglDisplay, attribs: *const i32, configs: *mut EglConfig, size: i32, count: *mut i32) -> u32;
    fn eglCreateContext(display: EglDisplay, config: EglConfig, share: EglContext, attribs: *const i32) -> EglContext;
    fn eglCreateWindowSurface(display: EglDisplay, config: EglConfig, window: *mut c_void, attribs: *const i32) -> EglSurface;
    fn eglMakeCurrent(display: EglDisplay, draw: EglSurface, read: EglSurface, context: EglContext) -> u32;
    fn eglSwapInterval(display: EglDisplay, interval: i32) -> u32;
    fn eglSwapBuffers(display: EglDisplay, surface: EglSurface) -> u32;
    fn eglDestroySurface(display: EglDisplay, surface: EglSurface) -> u32;
    fn eglDestroyContext(display: EglDisplay, context: EglContext) -> u32;
    fn eglGetError() -> i32;
    fn eglTerminate(display: EglDisplay) -> u32;
    fn eglReleaseThread() -> u32;
    fn eglQuerySurface(display: EglDisplay, surface: EglSurface, attribute: i32, value: *mut i32) -> u32;
    fn eglGetProcAddress(name: *const c_char) -> *const c_void;
}

#[link(name = "wayland-egl")]
extern "C" {
    fn wl_egl_window_create(surface: *mut c_void, width: i32, height: i32) -> *mut c_void;
    fn wl_egl_window_resize(window: *mut c_void, width: i32, height: i32, dx: i32, dy: i32);
    fn wl_egl_window_destroy(window: *mut c_void);
}

type SwapWithDamage = unsafe extern "C" fn(EglDisplay, EglSurface, *const i32, i32) -> u32;

struct Egl {
    display: EglDisplay,
    config: EglConfig,
    context: EglContext,
    surface: EglSurface,
    window: *mut c_void,
    swap_with_damage: Option<SwapWithDamage>,
}

impl Egl {
    unsafe fn new(conn: &Connection, opaque: bool) -> Result<Self> {
        let native = conn.backend().display_ptr() as *mut c_void;
        let display = eglGetPlatformDisplay(EGL_PLATFORM_WAYLAND_KHR, native, std::ptr::null());
        if display.is_null() || eglInitialize(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
            bail!("EGL initialize failed: {:#x}", eglGetError());
        }
        match Self::with_display(display, opaque) {
            Ok(egl) => Ok(egl),
            Err(error) => {
                eglTerminate(display);
                Err(error)
            }
        }
    }

    unsafe fn with_display(display: EglDisplay, opaque: bool) -> Result<Self> {
        if eglBindAPI(EGL_OPENGL_ES_API) == 0 {
            bail!("EGL GLES API unavailable");
        }
        // No alpha channel when opaque: the compositor then knows every pixel
        // covers what is below.
        let attribs = [
            EGL_SURFACE_TYPE, EGL_WINDOW_BIT,
            EGL_RENDERABLE_TYPE, EGL_OPENGL_ES3_BIT,
            EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8,
            EGL_ALPHA_SIZE, if opaque { 0 } else { 8 },
            EGL_NONE,
        ];
        let mut config = std::ptr::null_mut();
        let mut count = 0;
        if eglChooseConfig(display, attribs.as_ptr(), &mut config, 1, &mut count) == 0 || count < 1 {
            bail!("no EGL config: {:#x}", eglGetError());
        }
        let context_attribs = [EGL_CONTEXT_MAJOR_VERSION, 3, EGL_NONE];
        let context = eglCreateContext(display, config, std::ptr::null_mut(), context_attribs.as_ptr());
        if context.is_null() {
            bail!("EGL context failed: {:#x}", eglGetError());
        }
        let swap_with_damage = ["eglSwapBuffersWithDamageKHR", "eglSwapBuffersWithDamageEXT"]
            .iter()
            .find_map(|name| {
                let name = CString::new(*name).unwrap();
                let f = eglGetProcAddress(name.as_ptr());
                (!f.is_null()).then(|| std::mem::transmute::<*const c_void, SwapWithDamage>(f))
            });
        Ok(Self {
            display,
            config,
            context,
            surface: std::ptr::null_mut(),
            window: std::ptr::null_mut(),
            swap_with_damage,
        })
    }

    /// Frames since this back buffer was last shown; 0 means unknown content.
    unsafe fn buffer_age(&self) -> usize {
        let mut age = 0;
        if eglQuerySurface(self.display, self.surface, EGL_BUFFER_AGE_EXT, &mut age) == 0 {
            eglGetError();
            return 0;
        }
        age.max(0) as usize
    }

    unsafe fn ensure_surface(&mut self, wl_surface: &WlSurface, width: i32, height: i32) -> Result<()> {
        if !self.window.is_null() {
            wl_egl_window_resize(self.window, width, height, 0, 0);
            return Ok(());
        }
        self.window = wl_egl_window_create(wl_surface.id().as_ptr() as *mut c_void, width, height);
        if self.window.is_null() {
            bail!("wl_egl_window_create failed");
        }
        self.surface = eglCreateWindowSurface(self.display, self.config, self.window, [EGL_NONE].as_ptr());
        if self.surface.is_null() {
            bail!("EGL window surface failed: {:#x}", eglGetError());
        }
        if eglMakeCurrent(self.display, self.surface, self.surface, self.context) == 0 {
            bail!("eglMakeCurrent failed: {:#x}", eglGetError());
        }
        // Pacing comes from our own frame callbacks, never from EGL blocking.
        eglSwapInterval(self.display, 0);
        Ok(())
    }

    /// `rects` are top-left based buffer pixels; `None` damages everything.
    unsafe fn swap(&self, rects: Option<&[[i32; 4]]>, height: i32) -> bool {
        match (rects, self.swap_with_damage) {
            (Some(rects), Some(swap)) if !rects.is_empty() => {
                // EGL damage rectangles have a bottom-left origin.
                let flat: Vec<i32> = rects
                    .iter()
                    .flat_map(|r| [r[0], height - r[1] - r[3], r[2], r[3]])
                    .collect();
                swap(self.display, self.surface, flat.as_ptr(), rects.len() as i32) != 0
            }
            _ => eglSwapBuffers(self.display, self.surface) != 0,
        }
    }
}

impl Drop for Egl {
    fn drop(&mut self) {
        unsafe {
            eglMakeCurrent(self.display, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            if !self.surface.is_null() {
                eglDestroySurface(self.display, self.surface);
            }
            if !self.window.is_null() {
                wl_egl_window_destroy(self.window);
            }
            eglDestroyContext(self.display, self.context);
            // Mesa keys EGL displays by the wl_display address. Each layer has
            // its own connection, so a display left initialized would leak its
            // shader threads and could be handed back, stale, to a later
            // connection that reuses the address.
            eglTerminate(self.display);
            eglReleaseThread();
        }
    }
}

// --- Wayland state ------------------------------------------------------------

#[derive(Default)]
struct OutputInfo {
    name: Option<String>,
    scale: i32,
}

#[derive(Default)]
struct State {
    outputs: Vec<(WlOutput, OutputInfo)>,
    size: Option<(u32, u32)>,
    configured: bool,
    closed: bool,
    frame_pending: bool,
    probe: Option<Probe>,
}

// El timestamp viene del compositor; no se confunde con callback ni swap.
impl Dispatch<WpPresentation, ()> for State {
    fn event(state: &mut Self, _: &WpPresentation, event: wp_presentation::Event,
             _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wp_presentation::Event::ClockId { clk_id } = event {
            if let Some(p) = state.probe.as_mut() { p.record(json!({"event":"clock", "clock_id":clk_id})); }
        }
    }
}
impl Dispatch<WpPresentationFeedback, u64> for State {
    fn event(state: &mut Self, _: &WpPresentationFeedback, event: wp_presentation_feedback::Event,
             id: &u64, _: &Connection, _: &QueueHandle<Self>) {
        let Some(p) = state.probe.as_mut() else { return };
        match event {
            wp_presentation_feedback::Event::Presented { tv_sec_hi, tv_sec_lo, tv_nsec, refresh,
                                                       seq_hi, seq_lo, flags } => {
                p.pending = p.pending.saturating_sub(1);
                let timestamp = (((tv_sec_hi as u64) << 32) | tv_sec_lo as u64) as u128 * 1_000_000_000 + tv_nsec as u128;
                p.record(json!({"event":"presented", "id":id, "timestamp_ns":timestamp as u64,
                    "refresh_ns":refresh, "sequence":((seq_hi as u64)<<32)|seq_lo as u64,
                    "flags":format!("{flags:?}")}));
            }
            wp_presentation_feedback::Event::Discarded => {
                p.pending = p.pending.saturating_sub(1);
                p.record(json!({"event":"discarded", "id":id}));
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &WlRegistry, _: wayland_client::protocol::wl_registry::Event,
             _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlOutput, usize> for State {
    fn event(state: &mut Self, _: &WlOutput, event: wl_output::Event, index: &usize,
             _: &Connection, _: &QueueHandle<Self>) {
        let Some((_, info)) = state.outputs.get_mut(*index) else { return };
        match event {
            wl_output::Event::Name { name } => info.name = Some(name),
            wl_output::Event::Scale { factor } => info.scale = factor,
            _ => {}
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for State {
    fn event(state: &mut Self, surface: &ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event,
             _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                surface.ack_configure(serial);
                state.size = Some((width, height));
                state.configured = true;
            }
            zwlr_layer_surface_v1::Event::Closed => state.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, ()> for State {
    fn event(state: &mut Self, _: &WlCallback, event: wl_callback::Event, _: &(),
             _: &Connection, _: &QueueHandle<Self>) {
        if let wl_callback::Event::Done { .. } = event {
            state.frame_pending = false;
            if let Some(p) = state.probe.as_mut() { p.record(json!({"event":"callback"})); }
        }
    }
}

delegate_noop!(State: ignore WlCompositor);
delegate_noop!(State: ignore WlSurface);
delegate_noop!(State: ignore WlRegion);
delegate_noop!(State: ignore ZwlrLayerShellV1);

/// Whether this session can run the GPU layer (EGL with GLES 3 on Wayland).
/// Probed once per process; callers fall back to the direct renderer.
pub fn available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| match Connection::connect_to_env() {
        Ok(conn) => match unsafe { Egl::new(&conn, true) } {
            Ok(_) => true,
            Err(error) => {
                eprintln!("gpu layer unavailable: {error:#}");
                false
            }
        },
        Err(error) => {
            eprintln!("gpu layer unavailable: {error}");
            false
        }
    })
}

pub struct LayerSpec {
    pub output: Option<String>,
    pub namespace: String,
    /// Overlay (screensaver) instead of bottom (background).
    pub overlay: bool,
    /// Take the keyboard so keys never reach the window underneath.
    pub keyboard_exclusive: bool,
    pub cols: usize,
    pub rows: usize,
    pub cell_px: f64,
    pub opaque: bool,
    pub show_fps: bool,
}

/// "FPS nn" in the bottom-left cells, like the GTK label it replaces.
fn fps_cells(text: &str, rows: usize) -> impl Iterator<Item = DirectCell> + '_ {
    text.chars().enumerate().map(move |(x, ch)| DirectCell {
        x,
        y: rows.saturating_sub(1),
        ch,
        color: (0xee, 0xee, 0xee),
        background: Some((0, 0, 0)),
        attrs: 0,
    })
}

/// Runs until `stop` is set or the compositor closes the layer.
pub fn run(
    spec: LayerSpec,
    snapshot: Arc<Mutex<Vec<DirectCell>>>,
    generation: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    wakeup: Arc<FrameWakeup>,
) -> Result<()> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let mut state = State { probe: Probe::open()?, ..State::default() };
    let presentation: Option<WpPresentation> = if state.probe.is_some() {
        let manager = globals.bind(&qh, 1..=1, ()).ok();
        if let Some(p) = state.probe.as_mut() { p.record(json!({"event":"capability", "presentation":manager.is_some()})); }
        manager
    } else { None };
    let compositor: WlCompositor = globals.bind(&qh, 4..=6, ())?;
    let layer_shell: ZwlrLayerShellV1 = globals.bind(&qh, 1..=4, ())?;
    let registry = globals.registry();
    globals.contents().with_list(|list| {
        for global in list.iter().filter(|g| g.interface == "wl_output") {
            let index = state.outputs.len();
            let output: WlOutput = registry.bind(global.name, global.version.min(4), &qh, index);
            state.outputs.push((output, OutputInfo { name: None, scale: 1 }));
        }
    });
    queue.roundtrip(&mut state)?;

    let output_index = state.outputs.iter().position(|(_, info)| info.name == spec.output);
    let scale = output_index.map(|i| state.outputs[i].1.scale.max(1)).unwrap_or(1);
    let output = output_index.map(|i| state.outputs[i].0.clone());
    if spec.output.is_some() && output.is_none() {
        bail!("output {:?} not found", spec.output);
    }

    let surface = compositor.create_surface(&qh, ());
    let layer = layer_shell.get_layer_surface(
        &surface,
        output.as_ref(),
        if spec.overlay { zwlr_layer_shell_v1::Layer::Overlay } else { zwlr_layer_shell_v1::Layer::Bottom },
        spec.namespace.clone(),
        &qh,
        (),
    );
    layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(if spec.keyboard_exclusive {
        KeyboardInteractivity::Exclusive
    } else {
        KeyboardInteractivity::None
    });
    // A background takes no pointer input.
    let empty = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&empty));
    empty.destroy();
    surface.set_buffer_scale(scale);
    surface.commit();

    while !state.configured && !state.closed && !stop.load(Ordering::Relaxed) {
        queue.blocking_dispatch(&mut state)?;
    }

    let mut egl = unsafe { Egl::new(&conn, spec.opaque)? };
    let mut grid = GpuGrid::new(spec.cols, spec.rows, spec.cell_px);
    let mut pipeline: Option<GpuPipeline> = None;
    let mut visible = Vec::<DirectCell>::new();
    let consumed = std::cell::Cell::new(0u64);
    let mut dirty = true;
    let mut buffer = (0, 0);
    let mut fps_text = String::from("FPS --");
    let mut fps_meter = (std::time::Instant::now(), 0u32);
    let mut frame: Vec<DirectCell> = Vec::new();
    // Damage of recent frames, newest first; None = the whole surface.
    const AGE_HISTORY: usize = 4;
    let mut history: std::collections::VecDeque<Option<Vec<bool>>> = std::collections::VecDeque::new();
    let policy = damage_policy();
    // TTFX_BG_WL_STATS=1 logs per-stage frame timings every 5 seconds.
    let mut stats = (std::env::var("TTFX_BG_WL_STATS").ok().as_deref() == Some("1")).then(Stats::default);
    // Benchmark-only knobs: TTFX_BG_WL_DRAW=0 commits frames without drawing
    // (isolates the per-frame fixed cost); TTFX_BG_WL_CLIP=none repaints the
    // whole buffer every frame (isolates the shader cost).
    let bench_draw = std::env::var("TTFX_BG_WL_DRAW").ok().as_deref() != Some("0");
    let bench_full_clip = std::env::var("TTFX_BG_WL_CLIP").ok().as_deref() == Some("none");
    let defer_consume = std::env::var("TTFX_BG_WL_DEFER_CONSUME").as_deref() == Ok("1");
    if let Some(p) = state.probe.as_mut() { p.record(json!({"event":"candidate", "defer_consume":defer_consume})); }
    let wl_fd = conn.backend().poll_fd().as_raw_fd();

    let result = {
        let mut frame_loop = || -> Result<()> {
            while !stop.load(Ordering::Relaxed) && !state.closed {
                if let Some(s) = stats.as_mut() {
                    s.iterations += 1;
                }
                queue.dispatch_pending(&mut state)?;
                if let Some((w, h)) = state.size.take() {
                    let size = ((w.max(1) as i32) * scale, (h.max(1) as i32) * scale);
                    if size != buffer {
                        buffer = size;
                        unsafe { egl.ensure_surface(&surface, size.0, size.1)? };
                        if pipeline.is_none() {
                            crate::gpu_grid::load_gl();
                            pipeline = Some(unsafe { GpuPipeline::new(true)? });
                        }
                        if spec.opaque {
                            let region = compositor.create_region(&qh, ());
                            region.add(0, 0, w as i32, h as i32);
                            surface.set_opaque_region(Some(&region));
                            region.destroy();
                        }
                        dirty = true;
                    }
                }

                if (!defer_consume || !state.frame_pending) && generation.load(Ordering::Acquire) != consumed.get() {
                    let tc = std::time::Instant::now();
                    let update = frame_handoff::consume_latest(&snapshot, &generation, &consumed, &mut visible, false);
                    if let Some(s) = stats.as_mut() {
                        s.consume_us += tc.elapsed().as_micros();
                    }
                    if let Some(p) = state.probe.as_mut() {
                        p.record(json!({"event":"consume", "generation":consumed.get(), "redraw":update.redraw,
                            "duration_us":tc.elapsed().as_micros() as u64}));
                    }
                    dirty |= update.redraw;
                    if spec.show_fps && update.new_frame {
                        fps_meter.1 += 1;
                        let elapsed = fps_meter.0.elapsed();
                        if elapsed >= std::time::Duration::from_millis(900) {
                            fps_text = format!("FPS {:.0}", fps_meter.1 as f64 / elapsed.as_secs_f64());
                            fps_meter = (std::time::Instant::now(), 0);
                            dirty = true;
                        }
                    }
                }

                if dirty && !state.frame_pending {
                    if let Some(pipeline) = pipeline.as_mut() {
                        dirty = false;
                        grid.set_size(buffer.0, buffer.1, scale as f64);
                        let cells: &[DirectCell] = if spec.show_fps {
                            frame.clear();
                            frame.extend(visible.iter().filter(|c| !(c.y + 1 == spec.rows && c.x < fps_text.len())).cloned());
                            frame.extend(fps_cells(&fps_text, spec.rows));
                            &frame
                        } else {
                            &visible
                        };
                        let t0 = std::time::Instant::now();
                        if let Err(error) = grid.fill(cells) {
                            eprintln!("wayland layer: {error:?}");
                        }
                        let t1 = std::time::Instant::now();
                        let tiles = grid.damage_tiles();
                        let damage = tiles.as_ref().map(|t| grid.tiles_to_rects(t));
                        let t2 = std::time::Instant::now();
                        if damage.as_ref().is_none_or(|rects| !rects.is_empty()) {
                            // What the compositor is told to repaint; None = everything.
                            let report: Option<Vec<[i32; 4]>> = match (policy, &damage) {
                                (DamagePolicy::Full, _) | (_, None) => None,
                                (DamagePolicy::Min, Some(_)) => Some(vec![[0, 0, 1, 1]]),
                                (DamagePolicy::Rects(max), Some(rects)) => {
                                    let mut rects = rects.clone();
                                    crate::gpu_grid::coalesce_rects(&mut rects, max);
                                    Some(rects)
                                }
                            };
                            surface.frame(&qh, ());
                            state.frame_pending = true;
                            unsafe {
                                gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                                gl::Viewport(0, 0, buffer.0, buffer.1);
                                // The back buffer is `age` frames old: repaint what changed
                                // in the frames it missed plus this frame's damage.
                                let age = egl.buffer_age();
                                let clip: Option<Vec<[i32; 4]>> = match &tiles {
                                    Some(current) if age >= 1 && age - 1 <= history.len() => {
                                        let missed: Vec<_> = history.iter().take(age - 1).collect();
                                        if missed.iter().any(|h| h.is_none()) {
                                            None
                                        } else {
                                            let mut union = current.clone();
                                            for old in missed.into_iter().flatten() {
                                                for (u, o) in union.iter_mut().zip(old) {
                                                    *u |= *o;
                                                }
                                            }
                                            Some(grid.tiles_to_rects(&union))
                                        }
                                    }
                                    _ => None,
                                };
                                let clip = if bench_full_clip { None } else { clip };
                                if bench_draw {
                                    pipeline.draw(&mut grid, true, spec.opaque, clip.as_deref());
                                }
                                let t3 = std::time::Instant::now();
                                let clip_rects = clip.as_ref().map_or(1, Vec::len);
                                history.push_front(tiles.clone());
                                history.truncate(AGE_HISTORY);
                                if let Some(p) = state.probe.as_mut() {
                                    p.submitted += 1;
                                    if let Some(manager) = presentation.as_ref() {
                                        if p.pending < 128 {
                                            manager.feedback(&surface, &qh, p.submitted);
                                            p.pending += 1;
                                        } else { p.record(json!({"event":"feedback_limit", "id":p.submitted})); }
                                    }
                                }
                                if !egl.swap(report.as_deref(), buffer.1) {
                                    bail!("eglSwapBuffers failed: {:#x}", eglGetError());
                                }
                                if let Some(p) = state.probe.as_mut() {
                                    let t4 = std::time::Instant::now();
                                    p.record(json!({"event":"submit", "id":p.submitted, "generation":consumed.get(),
                                        "cell_upload_bytes":if bench_draw {Some(pipeline.last_cell_upload_bytes())}else{None},
                                        "draw_work":if bench_draw && !pipeline.clipped_cells() {Some(pipeline.last_draw_work())}else{None},
                                        "draw_totals":if bench_draw {Some(pipeline.last_draw_totals())}else{None},
                                        "fill_us":(t1-t0).as_micros() as u64, "damage_us":(t2-t1).as_micros() as u64,
                                        "draw_submit_us":(t3-t2).as_micros() as u64, "swap_us":(t4-t3).as_micros() as u64}));
                                }
                                if let Some(s) = stats.as_mut() {
                                    let t4 = std::time::Instant::now();
                                    for (sum, d) in s.sums.iter_mut().zip([t1 - t0, t2 - t1, t3 - t2, t4 - t3]) {
                                        *sum += d.as_micros();
                                    }
                                    let surface_area = buffer.0 as i64 * buffer.1 as i64;
                                    let (sent, area) = report.as_ref().map_or((1, surface_area), |r| {
                                        (r.len(), r.iter().copied().map(crate::gpu_grid::rect_area).sum())
                                    });
                                    s.frames += 1;
                                    s.clip_rects += clip_rects;
                                    s.sent_rects += sent;
                                    s.sent_area += area;
                                    if s.since.elapsed() >= std::time::Duration::from_secs(5) {
                                        let n = s.frames.max(1);
                                        let cpu_now = thread_cpu_us();
                                        eprintln!(
                                            "wl-stats frames={} fill={}us damage={}us draw={}us swap={}us clip_rects={} sent_rects={} area={:.1}% thread_cpu={}us/frame consume={}us iters/frame={:.1}",
                                            s.frames,
                                            s.sums[0] / n as u128,
                                            s.sums[1] / n as u128,
                                            s.sums[2] / n as u128,
                                            s.sums[3] / n as u128,
                                            s.clip_rects / n as usize,
                                            s.sent_rects / n as usize,
                                            100.0 * s.sent_area as f64 / (n as f64 * surface_area as f64),
                                            (cpu_now - s.thread_cpu_start) / n as u128,
                                            s.consume_us / n as u128,
                                            s.iterations as f64 / n as f64
                                        );
                                        *s = Stats::default();
                                        s.thread_cpu_start = cpu_now;
                                    }
                                }
                            }
                        }
                    }
                }

                conn.flush()?;
                let Some(guard) = queue.prepare_read() else { continue };
                let mut fds = [
                    libc::pollfd { fd: wl_fd, events: libc::POLLIN, revents: 0 },
                    libc::pollfd { fd: wakeup.fd(), events: libc::POLLIN, revents: 0 },
                ];
                let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, 500) };
                if ready > 0 && fds[0].revents & libc::POLLIN != 0 {
                    guard.read()?;
                } else {
                    drop(guard);
                }
                if fds[1].revents & libc::POLLIN != 0 {
                    let _ = wakeup.drain();
                }
            }
            Ok(())
        };
        frame_loop()
    };

    // Unmap, then destroy, and let the compositor handle each step while this
    // client still exists. Dropping the connection with the layer mapped left
    // Hyprland showing a layer that no client owned.
    surface.attach(None, 0, 0);
    surface.commit();
    let _ = queue.roundtrip(&mut state);
    drop(egl);
    layer.destroy();
    surface.destroy();
    let _ = queue.roundtrip(&mut state);
    result
}

/// What the compositor is told to repaint. Hyprland unions the damage of its
/// last frames and, past 8 rectangles, repaints their bounding box instead;
/// a couple of merged rectangles per frame keep its partial repaint working.
/// `TTFX_BG_WL_DAMAGE` = `full` | `bbox` | `all` | a maximum rectangle count;
/// `min` reports a single pixel and exists only to measure the compositor's
/// fixed per-frame cost (the screen does not refresh properly with it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DamagePolicy {
    Full,
    Min,
    Rects(usize),
}

const DEFAULT_MAX_DAMAGE_RECTS: usize = 2;

/// CPU time consumed by the calling thread, in microseconds.
fn thread_cpu_us() -> u128 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec as u128 * 1_000_000 + ts.tv_nsec as u128 / 1000
}

fn damage_policy() -> DamagePolicy {
    parse_damage_policy(std::env::var("TTFX_BG_WL_DAMAGE").ok().as_deref())
}

fn parse_damage_policy(value: Option<&str>) -> DamagePolicy {
    match value.map(str::trim) {
        Some("full") => DamagePolicy::Full,
        Some("min") => DamagePolicy::Min,
        Some("bbox") => DamagePolicy::Rects(1),
        Some("all") => DamagePolicy::Rects(usize::MAX),
        Some(n) => n
            .parse::<usize>()
            .map_or(DamagePolicy::Rects(DEFAULT_MAX_DAMAGE_RECTS), |n| DamagePolicy::Rects(n.max(1))),
        None => DamagePolicy::Rects(DEFAULT_MAX_DAMAGE_RECTS),
    }
}

/// Per-stage timings and damage sizes, reset every report.
struct Stats {
    /// Thread CPU time at the start of the window, iterations and snapshot-consume time.
    thread_cpu_start: u128,
    iterations: u64,
    consume_us: u128,
    since: std::time::Instant,
    frames: u32,
    sums: [u128; 4],
    clip_rects: usize,
    sent_rects: usize,
    sent_area: i64,
}

impl Default for Stats {
    fn default() -> Self {
        Self { since: std::time::Instant::now(), frames: 0, sums: [0; 4], clip_rects: 0, sent_rects: 0, sent_area: 0, thread_cpu_start: thread_cpu_us(), iterations: 0, consume_us: 0 }
    }
}

#[cfg(test)]
mod damage_policy_tests {
    use super::*;

    #[test]
    fn damage_policy_parses_names_and_counts() {
        assert_eq!(parse_damage_policy(None), DamagePolicy::Rects(DEFAULT_MAX_DAMAGE_RECTS));
        assert_eq!(parse_damage_policy(Some("full")), DamagePolicy::Full);
        assert_eq!(parse_damage_policy(Some("bbox")), DamagePolicy::Rects(1));
        assert_eq!(parse_damage_policy(Some(" 4 ")), DamagePolicy::Rects(4));
        assert_eq!(parse_damage_policy(Some("0")), DamagePolicy::Rects(1));
        assert_eq!(parse_damage_policy(Some("nonsense")), DamagePolicy::Rects(DEFAULT_MAX_DAMAGE_RECTS));
    }
}
