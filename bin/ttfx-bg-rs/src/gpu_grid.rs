//! GPU presentation for the direct renderer.
//!
//! The CPU path rasterizes every changed frame into a full-surface BGRA buffer
//! and hands it to GSK as a new texture. This path keeps glyph masks resident
//! in a GL atlas and uploads only two `cols x rows` RGBA cell textures per
//! frame (a few hundred KB instead of the whole surface). One full-screen
//! fragment shader resolves cell -> glyph mask -> color.
//!
//! Cell texture layout (RGBA8, one texel per grid cell):
//!   fg = (r, g, b, slot & 0xff)
//!   bg = (r, g, b, flags) where flags bit 0 = has background,
//!        bits 1..7 = slot >> 8. Slot 0 means "no glyph".
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};

use anyhow::{anyhow, Result};
use gtk4::cairo::{Context, Format, ImageSurface};
use gtk4::pango;

use crate::DirectCell;

const STYLE_BITS: u64 = 1 | 2 | 4 | 64;
const HIDDEN: u64 = 32;
const MAX_ATLAS_PX: i32 = 4096;
const MAX_ATLAS_COLS: i32 = 64;
const TILE_W: usize = 8;
const TILE_H: usize = 4;

#[link(name = "EGL")]
extern "C" {
    fn eglGetProcAddress(name: *const c_char) -> *const c_void;
}

pub(crate) unsafe fn egl_timer_proc(name:*const c_char)->*const c_void{eglGetProcAddress(name)}

pub fn load_gl() {
    gl::load_with(|name| {
        let name = CString::new(name).unwrap();
        unsafe { eglGetProcAddress(name.as_ptr()) }
    });
}

/// CPU-side grid state. Lives on the render thread next to the GL context.
pub struct GpuGrid {
    cols: usize,
    rows: usize,
    width: i32,
    height: i32,
    font_size: f64,
    device_font: f64,
    cell_w: i32,
    cell_h: i32,
    atlas_cols: i32,
    atlas_rows: i32,
    slots: HashMap<(char, u64), u16>,
    pending_masks: Vec<(u16, Vec<u8>)>,
    atlas_reset: bool,
    fg: Vec<u8>,
    bg: Vec<u8>,
    prev_fg: Vec<u8>,
    prev_bg: Vec<u8>,
    force_full: bool,
}


/// Merge rectangles until at most `max` remain, each step joining the pair
/// whose bounding box adds the least area. Hyprland unions the damage of its
/// last frames and, past 8 rectangles, repaints their bounding box instead;
/// a few larger rectangles keep its partial repaint working.
pub fn coalesce_rects(rects: &mut Vec<[i32; 4]>, max: usize) {
    let max = max.max(1);
    while rects.len() > max {
        let mut best = (i64::MAX, 0, 1);
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let added = rect_area(rect_union(rects[i], rects[j])) - rect_area(rects[i]) - rect_area(rects[j]);
                if added < best.0 {
                    best = (added, i, j);
                }
            }
        }
        let (_, i, j) = best;
        let merged = rect_union(rects[i], rects[j]);
        rects.swap_remove(j);
        rects[i] = merged;
    }
}

pub fn rect_area(r: [i32; 4]) -> i64 {
    r[2] as i64 * r[3] as i64
}

fn rect_union(a: [i32; 4], b: [i32; 4]) -> [i32; 4] {
    let x0 = a[0].min(b[0]);
    let y0 = a[1].min(b[1]);
    let x1 = (a[0] + a[2]).max(b[0] + b[2]);
    let y1 = (a[1] + a[3]).max(b[1] + b[3]);
    [x0, y0, x1 - x0, y1 - y0]
}

impl GpuGrid {
    /// `font_size` is in logical pixels, like the CPU atlas.
    pub fn new(cols: usize, rows: usize, font_size: f64) -> Self {
        Self {
            cols: cols.max(1),
            rows: rows.max(1),
            width: 0,
            height: 0,
            font_size,
            device_font: font_size,
            cell_w: 1,
            cell_h: 1,
            atlas_cols: 1,
            atlas_rows: 1,
            slots: HashMap::new(),
            pending_masks: Vec::new(),
            atlas_reset: true,
            fg: vec![0; cols.max(1) * rows.max(1) * 4],
            bg: vec![0; cols.max(1) * rows.max(1) * 4],
            prev_fg: Vec::new(),
            prev_bg: Vec::new(),
            force_full: true,
        }
    }

    /// Match the framebuffer size in device pixels. Glyph masks are cell-sized,
    /// so any change rebuilds the atlas.
    pub fn set_size(&mut self, width: i32, height: i32, scale: f64) {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == (self.width, self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        self.cell_w = ((width as f64 / self.cols as f64).ceil() as i32).max(1);
        self.cell_h = ((height as f64 / self.rows as f64).ceil() as i32).max(1);
        self.atlas_cols = (MAX_ATLAS_PX / self.cell_w).clamp(1, MAX_ATLAS_COLS);
        self.atlas_rows = (MAX_ATLAS_PX / self.cell_h).clamp(1, MAX_ATLAS_COLS);
        self.font_size = self.font_size.max(1.0);
        self.device_font = self.font_size * scale.max(1.0);
        self.force_full = true;
        self.reset_atlas();
    }

    #[cfg(test)]
    /// Device-pixel rectangles that differ from the previous call. `None`
    /// means the whole surface.
    pub fn damage(&mut self) -> Option<Vec<[i32; 4]>> {
        self.damage_tiles().map(|tiles| self.tiles_to_rects(&tiles))
    }

    /// Changed tiles (TILE_W x TILE_H cells, row-major) since the previous
    /// call; `None` means the whole surface. Glyphs never leave their cell,
    /// so cell bounds are exact.
    pub fn damage_tiles(&mut self) -> Option<Vec<bool>> {
        let full = self.force_full || self.prev_fg.len() != self.fg.len();
        let (cols, rows) = (self.cols, self.rows);
        let (tx, ty) = (cols.div_ceil(TILE_W), rows.div_ceil(TILE_H));
        let mut tiles = vec![false; tx * ty];
        if !full {
            let row_bytes = cols * 4;
            for y in 0..rows {
                let span = y * row_bytes..(y + 1) * row_bytes;
                // Most rows are unchanged: one memcmp each settles them.
                if self.fg[span.clone()] == self.prev_fg[span.clone()]
                    && self.bg[span.clone()] == self.prev_bg[span.clone()]
                {
                    continue;
                }
                for t in 0..tx {
                    let a = span.start + t * TILE_W * 4;
                    let b = span.start + ((t + 1) * TILE_W).min(cols) * 4;
                    if self.fg[a..b] != self.prev_fg[a..b] || self.bg[a..b] != self.prev_bg[a..b] {
                        tiles[(y / TILE_H) * tx + t] = true;
                    }
                }
            }
        }
        self.prev_fg.clone_from(&self.fg);
        self.prev_bg.clone_from(&self.bg);
        self.force_full = false;
        (!full).then_some(tiles)
    }

    /// Merge tiles into few rectangles: horizontal runs per tile row, then
    /// identical runs on consecutive tile rows become one taller rectangle.
    pub fn tiles_to_rects(&self, tiles: &[bool]) -> Vec<[i32; 4]> {
        let tx = self.cols.div_ceil(TILE_W);
        let ty = self.rows.div_ceil(TILE_H);
        // (start, end, first_row, last_row) of runs still growing downward.
        let mut open: Vec<(usize, usize, usize, usize)> = Vec::new();
        let mut done = Vec::new();
        for row in 0..ty {
            let mut runs = Vec::new();
            let mut t = 0;
            while t < tx {
                if tiles.get(row * tx + t).copied().unwrap_or(false) {
                    let start = t;
                    while t < tx && tiles[row * tx + t] {
                        t += 1;
                    }
                    runs.push((start, t));
                } else {
                    t += 1;
                }
            }
            let mut next = Vec::new();
            for (start, end) in runs {
                if let Some(i) = open.iter().position(|o| o.0 == start && o.1 == end && o.3 + 1 == row) {
                    let mut run = open.swap_remove(i);
                    run.3 = row;
                    next.push(run);
                } else {
                    next.push((start, end, row, row));
                }
            }
            done.append(&mut open);
            open = next;
        }
        done.append(&mut open);
        done.into_iter()
            .map(|(s, e, r0, r1)| self.cell_rect(s * TILE_W, r0 * TILE_H, e * TILE_W, (r1 + 1) * TILE_H))
            .collect()
    }

    fn cell_rect(&self, c0: usize, r0: usize, c1: usize, r1: usize) -> [i32; 4] {
        let px = |c: usize| (c.min(self.cols) as f64 * self.width as f64 / self.cols as f64).floor() as i32;
        let py = |r: usize| (r.min(self.rows) as f64 * self.height as f64 / self.rows as f64).floor() as i32;
        let (x0, y0) = (px(c0), py(r0));
        let x1 = if c1 >= self.cols { self.width } else { px(c1) };
        let y1 = if r1 >= self.rows { self.height } else { py(r1) };
        [x0, y0, x1 - x0, y1 - y0]
    }

    fn reset_atlas(&mut self) {
        self.slots.clear();
        self.pending_masks.clear();
        self.atlas_reset = true;
    }

    fn capacity(&self) -> usize {
        // Slot 0 is reserved; bg flags hold 7 high bits.
        ((self.atlas_cols * self.atlas_rows) as usize - 1).min(128 * 256 - 1)
    }

    /// Rebuild both cell textures from a sparse frame.
    pub fn fill(&mut self, cells: &[DirectCell]) -> Result<()> {
        if self.fill_once(cells)? {
            return Ok(());
        }
        // Atlas overflowed: start over with only this frame's glyphs.
        self.reset_atlas();
        if self.fill_once(cells)? {
            Ok(())
        } else {
            Err(anyhow!("frame uses more distinct glyphs than the atlas holds"))
        }
    }

    fn fill_once(&mut self, cells: &[DirectCell]) -> Result<bool> {
        self.fg.fill(0);
        self.bg.fill(0);
        for cell in cells {
            if cell.x >= self.cols || cell.y >= self.rows {
                continue;
            }
            let i = (cell.y * self.cols + cell.x) * 4;
            let mut flags = 0u8;
            if let Some((r, g, b)) = cell.background {
                self.bg[i..i + 3].copy_from_slice(&[r, g, b]);
                flags |= 1;
            }
            let blank = cell.ch == ' ' && cell.attrs & (4 | 64) == 0;
            let mut slot = 0u16;
            if !blank && cell.attrs & HIDDEN == 0 {
                match self.slot_for(cell.ch, cell.attrs & STYLE_BITS)? {
                    Some(s) => slot = s,
                    None => return Ok(false),
                }
            }
            let (r, g, b) = cell.color;
            self.fg[i..i + 4].copy_from_slice(&[r, g, b, (slot & 0xff) as u8]);
            self.bg[i + 3] = flags | (((slot >> 8) as u8) << 1);
        }
        Ok(true)
    }

    fn slot_for(&mut self, ch: char, style: u64) -> Result<Option<u16>> {
        if let Some(&slot) = self.slots.get(&(ch, style)) {
            return Ok(Some(slot));
        }
        let slot = self.slots.len() + 1;
        if slot > self.capacity() {
            return Ok(None);
        }
        let slot = slot as u16;
        let mask = self.rasterize_mask(ch, style)?;
        self.slots.insert((ch, style), slot);
        self.pending_masks.push((slot, mask));
        Ok(Some(slot))
    }

    // Same glyph construction as the CPU atlas, at device resolution. Pango
    // does the font fallback: the cairo toy API draws one face only, which
    // turns katakana (matrix) into boxes on systems whose monospace font
    // lacks them.
    fn rasterize_mask(&self, ch: char, attrs: u64) -> Result<Vec<u8>> {
        let (cw, chh) = (self.cell_w, self.cell_h);
        let mut surface = ImageSurface::create(Format::A8, cw, chh)?;
        {
            let cr = Context::new(&surface)?;
            cr.set_source_rgb(1.0, 1.0, 1.0);
            if ch == '█' {
                cr.paint()?;
            } else {
                let font = self.device_font.min(chh as f64 * 0.95);
                let baseline = (chh as f64 - font) * 0.5 + font;
                let mut desc = pango::FontDescription::from_string("monospace");
                desc.set_absolute_size(font * pango::SCALE as f64);
                if attrs & 1 != 0 {
                    desc.set_weight(pango::Weight::Bold);
                }
                if attrs & 2 != 0 {
                    desc.set_style(pango::Style::Italic);
                }
                let layout = pangocairo::functions::create_layout(&cr);
                layout.set_font_description(Some(&desc));
                layout.set_text(&ch.to_string());
                let layout_baseline = layout.baseline() as f64 / pango::SCALE as f64;
                cr.move_to(0.0, baseline - layout_baseline);
                pangocairo::functions::show_layout(&cr, &layout);
                if attrs & (4 | 64) != 0 {
                    cr.set_line_width((font / 12.0).max(1.0));
                    if attrs & 4 != 0 {
                        let y = (baseline + (chh as f64 - baseline) * 0.45).min(chh as f64 - 0.5);
                        cr.move_to(0.0, y);
                        cr.line_to(cw as f64, y);
                        cr.stroke()?;
                    }
                    if attrs & 64 != 0 {
                        cr.move_to(0.0, baseline - font * 0.35);
                        cr.line_to(cw as f64, baseline - font * 0.35);
                        cr.stroke()?;
                    }
                }
            }
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data()?;
        let mut mask = Vec::with_capacity((cw * chh) as usize);
        for y in 0..chh as usize {
            mask.extend_from_slice(&data[y * stride..y * stride + cw as usize]);
        }
        Ok(mask)
    }
}

/// GL objects. Create and use only while their context is current.
pub struct GpuPipeline {
    timer: Option<crate::gpu_timer::Timer>,
    timer_events: Vec<serde_json::Value>,
    program: u32,
    vao: u32,
    fg_tex: u32,
    bg_tex: u32,
    atlas_tex: u32,
    atlas_size: (i32, i32),
    grid: (i32, i32),
    u_size: i32,
    u_grid: i32,
    u_cell: i32,
    u_atlas_cols: i32,
    u_opaque: i32,
    cell_upload_cache: Option<crate::gpu_upload_cache::UploadCache>,
    last_cell_upload_bytes: [usize;2],
    instanced_cells: bool,
    u_cells_mode: i32,
    u_cell_range: i32,
    clipped_cells: bool,
    last_draw_totals: [u64;2],
    last_draw_work: [u64;3],
}


const VERTEX: &str = r#"
void main() {
    vec2 v = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(v * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const FRAGMENT: &str = r#"
uniform sampler2D u_fg;
uniform sampler2D u_bg;
uniform sampler2D u_atlas;
uniform vec2 u_size;
uniform vec2 u_grid;
uniform vec2 u_cell;
uniform int u_atlas_cols;
uniform int u_opaque;
out vec4 frag;
void main() {
    // Cells are laid out from the top; GL window coordinates start at the bottom.
    vec2 p = vec2(gl_FragCoord.x, u_size.y - gl_FragCoord.y);
    // Cell k spans [floor(k*size/grid), floor((k+1)*size/grid)): the exact
    // boundaries the damage rectangles use, so partial redraws never leave a
    // one-pixel seam of a neighbouring cell behind.
    vec2 px = floor(p);
    vec2 cell = clamp(ceil((px + 1.0) * u_grid / u_size) - 1.0, vec2(0.0), u_grid - 1.0);
    cell -= step(px + 1.0, floor(cell * u_size / u_grid)) * step(0.5, cell);
    cell += step(floor((cell + 1.0) * u_size / u_grid), px) * step(cell, u_grid - 1.5);
    vec2 origin = floor(cell * u_size / u_grid);
    ivec2 local = ivec2(p - origin);
    ivec2 ic = ivec2(cell);
    vec4 fg = texelFetch(u_fg, ic, 0);
    vec4 bg = texelFetch(u_bg, ic, 0);
    int flags = int(bg.a * 255.0 + 0.5);
    int slot = int(fg.a * 255.0 + 0.5) + (flags >> 1) * 256;
    float base_a = ((flags & 1) != 0 || u_opaque != 0) ? 1.0 : 0.0;
    vec3 base = (flags & 1) != 0 ? bg.rgb : vec3(0.0);
    float a = 0.0;
    ivec2 cs = ivec2(u_cell);
    if (slot > 0 && local.x < cs.x && local.y < cs.y) {
        ivec2 s = ivec2(slot % u_atlas_cols, slot / u_atlas_cols);
        a = texelFetch(u_atlas, s * cs + local, 0).r;
    }
    frag = vec4(fg.rgb * a + base * base_a * (1.0 - a), a + base_a * (1.0 - a));
}
"#;

unsafe fn compile(kind: u32, header: &str, body: &str) -> Result<u32> {
    let shader = gl::CreateShader(kind);
    let source = CString::new(format!("{header}{body}"))?;
    gl::ShaderSource(shader, 1, &source.as_ptr(), std::ptr::null());
    gl::CompileShader(shader);
    let mut ok = 0;
    gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut ok);
    if ok == 0 {
        let mut log = vec![0u8; 4096];
        let mut len = 0;
        gl::GetShaderInfoLog(shader, log.len() as i32, &mut len, log.as_mut_ptr() as *mut c_char);
        gl::DeleteShader(shader);
        return Err(anyhow!("shader: {}", String::from_utf8_lossy(&log[..len.max(0) as usize])));
    }
    Ok(shader)
}

unsafe fn new_texture() -> u32 {
    let mut tex = 0;
    gl::GenTextures(1, &mut tex);
    gl::BindTexture(gl::TEXTURE_2D, tex);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
    tex
}

impl GpuPipeline {
    /// # Safety
    /// The context must be current and `load_gl` must have run.
    pub unsafe fn new(gles: bool) -> Result<Self> {
        let header = if gles {
            "#version 300 es\nprecision highp float;\nprecision highp int;\n"
        } else {
            "#version 330 core\n"
        };
        let clipped_cells=std::env::var("TTFX_BG_GPU_CLIPPED_QUADS").as_deref()==Ok("1");
        let instanced_cells=clipped_cells || std::env::var("TTFX_BG_GPU_CELL_QUADS").as_deref()==Ok("1");
        let fragment=instanced_cells.then(||crate::gpu_cell_quads::fragment(FRAGMENT));
        let vs = compile(gl::VERTEX_SHADER, header, if instanced_cells{crate::gpu_cell_quads::VERTEX}else{VERTEX})?;
        let fs = compile(gl::FRAGMENT_SHADER, header, fragment.as_deref().unwrap_or(FRAGMENT))?;
        let program = gl::CreateProgram();
        gl::AttachShader(program, vs);
        gl::AttachShader(program, fs);
        gl::LinkProgram(program);
        gl::DeleteShader(vs);
        gl::DeleteShader(fs);
        let mut ok = 0;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut ok);
        if ok == 0 {
            gl::DeleteProgram(program);
            return Err(anyhow!("shader program failed to link"));
        }
        let loc = |name: &str| {
            let name = CString::new(name).unwrap();
            gl::GetUniformLocation(program, name.as_ptr())
        };
        gl::UseProgram(program);
        gl::Uniform1i(loc("u_fg"), 0);
        gl::Uniform1i(loc("u_bg"), 1);
        gl::Uniform1i(loc("u_atlas"), 2);
        let mut vao = 0;
        gl::GenVertexArrays(1, &mut vao);
        Ok(Self {
            timer:None, timer_events:vec![],
            program,
            vao,
            fg_tex: new_texture(),
            bg_tex: new_texture(),
            atlas_tex: new_texture(),
            atlas_size: (0, 0),
            grid: (0, 0),
            u_size: loc("u_size"),
            u_grid: loc("u_grid"),
            u_cell: loc("u_cell"),
            u_atlas_cols: loc("u_atlas_cols"),
            u_opaque: loc("u_opaque"),
            cell_upload_cache: (std::env::var("TTFX_BG_GPU_UPLOAD_DIRTY").as_deref()==Ok("1"))
                .then(crate::gpu_upload_cache::UploadCache::default),
            last_cell_upload_bytes: [0;2],
            instanced_cells,
            u_cells_mode: loc("u_cells_mode"),
            u_cell_range: loc("u_cell_range"),
            clipped_cells,
            last_draw_totals: [0;2],
            last_draw_work: [0;3],
        })
    }

    pub unsafe fn enable_timing(&mut self){
        match crate::gpu_timer::Timer::new(){Ok(timer)=>self.timer=Some(timer),Err(reason)=>self.timer_events.push(serde_json::json!({"event":"gpu_timer_unavailable","reason":reason}))}
    }
    pub fn timing_frame(&self)->Option<u64>{self.timer.as_ref().and_then(|t|t.frame())}
    pub fn take_timing_events(&mut self)->Vec<serde_json::Value>{let mut events=std::mem::take(&mut self.timer_events);if let Some(t)=self.timer.as_mut(){events.extend(t.take_events())}events}
    pub unsafe fn finish_timing(&mut self)->Vec<serde_json::Value>{if let Some(t)=self.timer.take(){self.timer_events.extend(t.finish())}self.take_timing_events()}

    pub fn last_cell_upload_bytes(&self)->[usize;2] {self.last_cell_upload_bytes}
    // vertices por instancia, instancias por draw, llamadas realmente emitidas.
    pub fn last_draw_work(&self)->[u64;3] {self.last_draw_work}
    pub fn clipped_cells(&self)->bool {self.clipped_cells}
    pub fn last_draw_totals(&self)->[u64;2] {self.last_draw_totals}
    unsafe fn draw_cells(&mut self,instances:Option<i32>) {
        if instances==Some(0) {return;}
        if let Some(count)=instances {gl::DrawArraysInstanced(gl::TRIANGLES,0,6,count);}
        else {gl::DrawArrays(gl::TRIANGLES,0,3);}
        self.last_draw_work[2]+=1;
        self.last_draw_totals[0]+=1;
        self.last_draw_totals[1]+=instances.map(|n|6*n as u64).unwrap_or(3);
    }

    /// Upload changed state and shade `clip` (top-left based framebuffer
    /// pixels), or everything when `None`; the rest keeps its previous content.
    ///
    /// # Safety
    /// The pipeline's context must be current.
    pub unsafe fn draw(&mut self, grid: &mut GpuGrid, upload_cells: bool, opaque: bool, clip: Option<&[[i32; 4]]>) {
        if let Some(timer)=self.timer.as_mut(){timer.begin();}
        self.last_cell_upload_bytes=[0;2];
        let atlas_was_reset=grid.atlas_reset;
        gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
        let (cols, rows) = (grid.cols as i32, grid.rows as i32);
        if grid.atlas_reset {
            grid.atlas_reset = false;
            let size = (grid.atlas_cols * grid.cell_w, grid.atlas_rows * grid.cell_h);
            gl::BindTexture(gl::TEXTURE_2D, self.atlas_tex);
            gl::TexImage2D(gl::TEXTURE_2D, 0, gl::R8 as i32, size.0, size.1, 0,
                gl::RED, gl::UNSIGNED_BYTE, std::ptr::null());
            self.atlas_size = size;
        }
        if !grid.pending_masks.is_empty() {
            gl::BindTexture(gl::TEXTURE_2D, self.atlas_tex);
            for (slot, mask) in grid.pending_masks.drain(..) {
                let slot = slot as i32;
                let x = (slot % grid.atlas_cols) * grid.cell_w;
                let y = (slot / grid.atlas_cols) * grid.cell_h;
                gl::TexSubImage2D(gl::TEXTURE_2D, 0, x, y, grid.cell_w, grid.cell_h,
                    gl::RED, gl::UNSIGNED_BYTE, mask.as_ptr() as *const c_void);
            }
        }
        let resized = self.grid != (cols, rows);
        if resized || upload_cells {
            let sent=self.cell_upload_cache.as_ref()
                .map(|cache|cache.plan((cols,rows),&grid.fg,&grid.bg,resized||atlas_was_reset))
                .unwrap_or([true,true]);
            for (index,(tex, data)) in [(self.fg_tex, &grid.fg), (self.bg_tex, &grid.bg)].into_iter().enumerate() {
                if !sent[index] {continue;}
                self.last_cell_upload_bytes[index]=data.len();
                gl::BindTexture(gl::TEXTURE_2D, tex);
                if resized {
                    gl::TexImage2D(gl::TEXTURE_2D, 0, gl::RGBA8 as i32, cols, rows, 0,
                        gl::RGBA, gl::UNSIGNED_BYTE, data.as_ptr() as *const c_void);
                } else {
                    gl::TexSubImage2D(gl::TEXTURE_2D, 0, 0, 0, cols, rows,
                        gl::RGBA, gl::UNSIGNED_BYTE, data.as_ptr() as *const c_void);
                }
            }
            if let Some(cache)=self.cell_upload_cache.as_mut() {
                cache.commit((cols,rows),&grid.fg,&grid.bg,sent);
            }
            self.grid = (cols, rows);
        }
        if let Some(timer)=self.timer.as_mut(){timer.after_upload();}
        gl::Disable(gl::BLEND);
        gl::UseProgram(self.program);
        gl::Uniform2f(self.u_size, grid.width as f32, grid.height as f32);
        gl::Uniform2f(self.u_grid, cols as f32, rows as f32);
        gl::Uniform2f(self.u_cell, grid.cell_w as f32, grid.cell_h as f32);
        gl::Uniform1i(self.u_atlas_cols, grid.atlas_cols);
        gl::Uniform1i(self.u_opaque, opaque as i32);
        let instances=if self.instanced_cells {
            let count=crate::gpu_cell_quads::instance_count(cols,rows,grid.width,grid.height);
            gl::Uniform1i(self.u_cells_mode,count.is_some() as i32);
            count
        } else {None};
        if instances.is_some() {gl::Uniform4i(self.u_cell_range,0,0,cols,rows);}
        self.last_draw_totals=[0;2];
        self.last_draw_work=instances.map(|n|[6,n as u64,0]).unwrap_or([3,1,0]);
        for (unit, tex) in [(0, self.fg_tex), (1, self.bg_tex), (2, self.atlas_tex)] {
            gl::ActiveTexture(gl::TEXTURE0 + unit);
            gl::BindTexture(gl::TEXTURE_2D, tex);
        }
        gl::BindVertexArray(self.vao);
        match clip {
            Some(rects) => {
                gl::Enable(gl::SCISSOR_TEST);
                for r in rects {
                    // Scissor boxes have a bottom-left origin.
                    gl::Scissor(r[0], grid.height - r[1] - r[3], r[2], r[3]);
                    let count=if self.clipped_cells && instances.is_some() {
                        let range=crate::gpu_cell_quads::clipped_range(cols,rows,grid.width,grid.height,*r).unwrap();
                        gl::Uniform4i(self.u_cell_range,range[0],range[1],range[2],range[3]);
                        Some(range[2]*range[3])
                    } else {instances};
                    self.draw_cells(count);
                }
                gl::Disable(gl::SCISSOR_TEST);
            }
            None => self.draw_cells(instances),
        }
        gl::BindVertexArray(0);
        gl::ActiveTexture(gl::TEXTURE0);
        if let Some(timer)=self.timer.as_mut(){timer.end();}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_with(cells: &[(usize, usize)]) -> GpuGrid {
        let mut grid = GpuGrid::new(40, 20, 8.0);
        grid.width = 400;
        grid.height = 200;
        grid.force_full = false;
        grid.prev_fg = vec![0; 40 * 20 * 4];
        grid.prev_bg = vec![0; 40 * 20 * 4];
        for &(x, y) in cells {
            grid.fg[(y * 40 + x) * 4] = 255;
        }
        grid
    }

    #[test]
    fn unchanged_frame_has_no_damage() {
        let mut grid = grid_with(&[]);
        assert_eq!(grid.damage(), Some(vec![]));
    }

    #[test]
    fn first_frame_and_resize_are_full() {
        let mut grid = grid_with(&[(1, 1)]);
        grid.force_full = true;
        assert_eq!(grid.damage(), None);
    }

    #[test]
    fn a_changed_cell_damages_its_tile_only() {
        let mut grid = grid_with(&[(9, 5)]);
        // Tile (1, 1): cells x 8..16, y 4..8 -> pixels 80..160, 40..80.
        assert_eq!(grid.damage(), Some(vec![[80, 40, 80, 40]]));
    }

    #[test]
    fn stacked_tiles_merge_into_one_rectangle() {
        let mut grid = grid_with(&[(0, 0), (9, 0), (0, 5), (9, 5)]);
        assert_eq!(grid.damage(), Some(vec![[0, 0, 160, 80]]));
    }

    #[test]
    fn merged_rectangles_cover_every_changed_tile_exactly() {
        let mut tiles = vec![false; 5 * 5];
        for i in [0, 1, 6, 7, 12, 18, 24] {
            tiles[i] = true;
        }
        let grid = grid_with(&[]);
        let rects = grid.tiles_to_rects(&tiles);
        let mut covered = vec![false; 25];
        for r in &rects {
            for ty in (r[1] / 40) as usize..((r[1] + r[3]) / 40) as usize {
                for tx in (r[0] / 80) as usize..((r[0] + r[2]) / 80) as usize {
                    assert!(!covered[ty * 5 + tx], "overlap");
                    covered[ty * 5 + tx] = true;
                }
            }
        }
        assert_eq!(covered, tiles);
    }
}

#[cfg(test)]
mod coalesce_tests {
    use super::*;

    fn contains(outer: [i32; 4], inner: [i32; 4]) -> bool {
        inner[0] >= outer[0]
            && inner[1] >= outer[1]
            && inner[0] + inner[2] <= outer[0] + outer[2]
            && inner[1] + inner[3] <= outer[1] + outer[3]
    }

    #[test]
    fn few_rectangles_are_left_alone() {
        let mut rects = vec![[0, 0, 10, 10], [50, 50, 10, 10]];
        coalesce_rects(&mut rects, 2);
        assert_eq!(rects, vec![[0, 0, 10, 10], [50, 50, 10, 10]]);
    }

    #[test]
    fn merged_rectangles_cover_every_original() {
        let original: Vec<[i32; 4]> = (0..30).map(|i| [(i * 37) % 500, (i * 53) % 300, 8, 6]).collect();
        let mut rects = original.clone();
        coalesce_rects(&mut rects, 3);
        assert_eq!(rects.len(), 3);
        for r in &original {
            assert!(rects.iter().any(|m| contains(*m, *r)), "{r:?} uncovered");
        }
    }

    #[test]
    fn nearby_rectangles_merge_before_distant_ones() {
        let mut rects = vec![[0, 0, 10, 10], [12, 0, 10, 10], [500, 400, 10, 10], [514, 400, 10, 10]];
        coalesce_rects(&mut rects, 2);
        rects.sort();
        assert_eq!(rects, vec![[0, 0, 22, 10], [500, 400, 24, 10]]);
    }

    #[test]
    fn a_single_bounding_box_is_the_floor() {
        let mut rects = vec![[0, 0, 1, 1], [99, 99, 1, 1]];
        coalesce_rects(&mut rects, 0);
        assert_eq!(rects, vec![[0, 0, 100, 100]]);
    }
}

#[cfg(test)]
#[path="gpu_equivalence_test.rs"]
mod equivalence_test;
