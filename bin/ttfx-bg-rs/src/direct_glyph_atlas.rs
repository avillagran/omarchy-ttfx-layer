use anyhow::Result;
use gtk4::cairo::{Context, FontSlant, FontWeight, Format, ImageSurface};
use std::collections::{HashMap, VecDeque};

use crate::DirectCell;

const MAX_COLORED_TILES: usize = 8_192;
const MAX_GLYPH_MASKS: usize = 256;
const MAX_RETAINED_CELLS: usize = 1_000_000;
const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;
type MaskKey = (char, u64);
type TileKey = (char, (u8, u8, u8), bool, u64, bool);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DamageRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub(super) struct RetainedPixels {
    // Owned storage must never alias the cache or a texture from an earlier frame.
    pub pixels: Vec<u8>,
    pub damage: Vec<DamageRect>,
    pub full_redraw: bool,
}

impl RetainedPixels {
    // Only update-region metadata changes. The retained pixel buffer and redraw
    // decision remain untouched; invalidation still bypasses update-region reuse.
    pub(super) fn bound_damage(&mut self, width: i32, height: i32) {
        if self.damage.is_empty() { return; }
        let (width, height) = (width.max(1), height.max(1));
        let mut x0 = width;
        let mut y0 = height;
        let mut x1 = 0;
        let mut y1 = 0;
        for rect in &self.damage {
            x0 = x0.min(rect.x);
            y0 = y0.min(rect.y);
            x1 = x1.max(rect.x.saturating_add(rect.width));
            y1 = y1.max(rect.y.saturating_add(rect.height));
        }
        // At 75% bounding-box coverage, use the exact full-frame region.
        let dense = i64::from(x1 - x0) * i64::from(y1 - y0) * 4
            >= i64::from(width) * i64::from(height) * 3;
        self.damage = vec![if dense {
            DamageRect { x: 0, y: 0, width, height }
        } else {
            DamageRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
        }];
    }
}

struct RetainedFrame {
    geometry: (usize, usize, i32, i32),
    pixels: Vec<u8>,
    cells: Vec<Option<DirectCell>>,
}

// Rasterize each unique glyph once for a given cell size, then colorize its
// alpha mask into either Cairo storage or an owned, premultiplied BGRA buffer.
pub(super) struct DirectGlyphAtlas {
    cell_w: i32,
    cell_h: i32,
    font_size: f64,
    geometry: (usize, usize, i32, i32),
    x_positions: Vec<i32>,
    y_positions: Vec<i32>,
    masks: HashMap<MaskKey, Vec<u8>>,
    mask_order: VecDeque<MaskKey>,
    // Premultiplied BGRA tiles avoid recoloring every glyph pixel each frame.
    // The alpha-mode key preserves opaque and transparent surface semantics.
    tiles: HashMap<TileKey, Vec<u8>>,
    tile_order: VecDeque<TileKey>,
    retained: Option<RetainedFrame>,
    // Read once per atlas, so one binary can compare both retained paths.
    tile_copy: bool,
}

#[cfg(test)]
mod pixel_buffer_tests {
    use super::*;

    #[test]
    fn full_block_covers_its_exact_cell_without_font_seams() {
        // Unicode FULL BLOCK is a filled cell, including at fractional grid
        // boundaries. The expected rectangle is independent of font metrics.
        for font_size in [3.0, 9.0, 24.0] {
            for attrs in [0, 1, 2, 4, 64] {
                for transparent in [false, true] {
                    let mut atlas = DirectGlyphAtlas::new(4, 4, font_size);
                    let mut cell = retained_cell(1, 1, '█', attrs);
                    cell.color = (200, 100, 50);
                    let pixels = atlas.rasterize_pixels(&[cell.clone()], 3, 3, 10, 10, transparent).unwrap();
                    for y in 0..10 {
                        for x in 0..10 {
                            let expected = if (3..6).contains(&x) && (3..6).contains(&y) {
                                [50, 100, 200, 255]
                            } else {
                                [0, 0, 0, if transparent { 0 } else { 255 }]
                            };
                            assert_eq!(&pixels[(y * 10 + x) * 4..][..4], &expected,
                                "full block at ({x},{y}), font={font_size}, attrs={attrs}");
                        }
                    }
                    assert_eq!(atlas.rasterize_retained_pixels(&[cell], 3, 3, 10, 10, transparent).unwrap().pixels, pixels);
                }
            }
        }
    }

    #[test]
    fn adjacent_full_blocks_fill_fractional_cells_without_bleeding() {
        for transparent in [false, true] {
            for tile_copy in [false, true] {
                let mut atlas = DirectGlyphAtlas::new(4, 4, 9.0);
                atlas.tile_copy = tile_copy;
                let cells: Vec<_> = [(0, (255, 0, 0)), (1, (0, 255, 0)), (2, (0, 0, 255))]
                    .into_iter().map(|(x, color)| DirectCell {
                        x, y: 1, ch: '█', color, background: None, attrs: 0,
                    }).collect();
                let full = atlas.rasterize_pixels(&cells, 3, 3, 10, 10, transparent).unwrap();
                let retained = atlas.rasterize_retained_pixels(&cells, 3, 3, 10, 10, transparent).unwrap();
                assert_eq!(retained.pixels, full);
                for y in 0..10 {
                    for x in 0..10 {
                        // Independent expected intervals for a 10-pixel/3-cell grid.
                        let expected = if !(3..6).contains(&y) {
                            [0, 0, 0, if transparent { 0 } else { 255 }]
                        } else if x < 3 { [0, 0, 255, 255] }
                        else if x < 6 { [0, 255, 0, 255] }
                        else { [255, 0, 0, 255] };
                        assert_eq!(&full[(y * 10 + x) * 4..][..4], &expected);
                    }
                }
            }
        }
    }

    #[test]
    fn bounded_full_blocks_recolor_remove_and_rebuild_after_resize() {
        let block = |x, y, color| DirectCell { x, y, ch: '█', color, background: None, attrs: 0 };
        for tile_copy in [false, true] {
            let mut atlas = DirectGlyphAtlas::new(4, 4, 9.0);
            atlas.tile_copy = tile_copy;
            let mut previous = Vec::new();
            for (cells, width, height) in [
                (vec![block(0, 0, (255, 0, 0)), block(2, 1, (0, 0, 255))], 10, 7),
                (vec![block(0, 0, (0, 255, 0)), block(2, 1, (0, 0, 255))], 10, 7),
                (vec![block(0, 0, (0, 255, 0))], 10, 7),
                (vec![block(0, 0, (0, 255, 0))], 12, 8),
                (vec![], 12, 8),
            ] {
                let mut frame = atlas.rasterize_retained_pixels(&cells, 3, 2, width, height, false).unwrap();
                frame.bound_damage(width, height);
                assert!(frame.damage.len() <= 1);
                // Fixed rectangles are independent of both raster implementations.
                let left_edge = if width == 10 { 3 } else { 4 };
                let right_start = if width == 10 { 6 } else { 8 };
                let row_edge = if height == 7 { 3 } else { 4 };
                for y in 0..height {
                    for x in 0..width {
                        let cell = cells.iter().find(|c| (c.x == 0 && c.y == 0 && x < left_edge && y < row_edge)
                            || (c.x == 2 && c.y == 1 && x >= right_start && y >= row_edge));
                        let expected = cell.map_or([0, 0, 0, 255], |c| [c.color.2, c.color.1, c.color.0, 255]);
                        assert_eq!(&frame.pixels[((y * width + x) * 4) as usize..][..4], &expected);
                    }
                }
                if previous.len() == frame.pixels.len() {
                    assert_damage_covers_changes(&previous, &frame, width as usize);
                } else {
                    assert!(frame.full_redraw);
                }
                previous = frame.pixels;
            }
        }
    }

    #[test]
    fn bounded_damage_covers_disjoint_checkerboard_and_edges() {
        for damage in [
            vec![DamageRect { x: 2, y: 3, width: 2, height: 2 }, DamageRect { x: 9, y: 8, width: 1, height: 1 }],
            (0..10).flat_map(|y| (0..10).filter(move |x| (x+y)%2 == 0).map(move |x| DamageRect { x, y, width: 1, height: 1 })).collect(),
            vec![DamageRect { x: 0, y: 0, width: 1, height: 1 }, DamageRect { x: 19, y: 19, width: 1, height: 1 }],
        ] {
            let mut frame = RetainedPixels { pixels: vec![7; 1600], damage: damage.clone(), full_redraw: false };
            frame.bound_damage(20, 20);
            assert_eq!(frame.damage.len(), 1);
            assert_eq!(frame.pixels, vec![7; 1600]);
            let bound = &frame.damage[0];
            for r in damage {
                assert!(bound.x <= r.x && bound.y <= r.y);
                assert!(bound.x + bound.width >= r.x + r.width);
                assert!(bound.y + bound.height >= r.y + r.height);
            }
        }
        let mut dense = RetainedPixels { pixels: vec![], damage: vec![
            DamageRect { x: 0, y: 0, width: 20, height: 15 }
        ], full_redraw: true };
        dense.bound_damage(20, 20);
        assert_eq!(dense.damage, vec![DamageRect { x: 0, y: 0, width: 20, height: 20 }]);
        assert!(dense.full_redraw);
        let mut empty = RetainedPixels { pixels: vec![], damage: vec![], full_redraw: false };
        empty.bound_damage(20, 20);
        assert!(empty.damage.is_empty());
    }

    #[test]
    fn asm_sized_retained_repaints_match_full_bytes_and_owned_frames() {
        // Widths5/6, heights10/11, glyph AA, clipping, background transitions,
        // removal and resize. Large enough to exercise the real ASM dispatch.
        for tile_copy in [false,true] {
            for transparent in [false,true] {
                let mut actual = DirectGlyphAtlas::new(6,11,9.0);
                actual.tile_copy = tile_copy;
                let mut reference = DirectGlyphAtlas::new(6,11,9.0);
                let mut frozen = None;
                let mut frozen_bytes = Vec::new();
                let mut previous = Vec::new();
                let cell = |x,y,ch,color,background| DirectCell { x,y,ch,color,background,attrs:0 };
                for (cells,w,h) in [
                    (vec![cell(0,0,'A',(240,80,170),None),cell(2,1,'█',(20,120,250),None)],17,21),
                    (vec![cell(0,0,'B',(90,240,50),None),cell(2,1,'C',(170,15,230),None)],17,21),
                    (vec![cell(0,0,'B',(90,240,50),Some((12,40,70)))],17,21),
                    (vec![cell(0,0,'D',(220,110,10),None)],17,21),
                    (vec![],17,21),
                    (vec![cell(2,1,'E',(200,180,120),None)],19,23),
                    (vec![cell(2,1,'F',(40,200,210),None)],19,23),
                ] {
                    let frame = actual.rasterize_retained_pixels(&cells,3,2,w,h,transparent).unwrap();
                    let full = reference.rasterize_pixels(&cells,3,2,w,h,transparent).unwrap();
                    assert_eq!(frame.pixels,full);
                    if previous.len()==frame.pixels.len() { assert_damage_covers_changes(&previous,&frame,w as usize); }
                    if let Some(saved) = &frozen { assert_eq!(*saved,frozen_bytes); }
                    previous=frame.pixels.clone();
                    if frozen.is_none() { frozen_bytes=frame.pixels.clone(); frozen=Some(frame.pixels); }
                }
            }
        }
    }

    #[test]
    fn bounded_retained_removals_resize_and_alpha_match_full_pixels() {
        let mut atlas = DirectGlyphAtlas::new(4, 7, 6.0);
        let mut reference = DirectGlyphAtlas::new(4, 7, 6.0);
        let mut previous = Vec::new();
        for (cells, w, h, alpha) in [
            (vec![retained_cell(0, 0, 'A', 0), retained_cell(2, 2, '█', 0)], 17, 23, false),
            (vec![retained_cell(1, 1, 'B', 0)], 17, 23, false),
            (vec![], 17, 23, false),
            (vec![], 17, 23, false),
            (vec![retained_cell(2, 2, 'C', 0)], 19, 25, false),
            (vec![], 19, 25, true),
        ] {
            let mut frame = atlas.rasterize_retained_pixels(&cells, 3, 3, w, h, alpha).unwrap();
            frame.bound_damage(w, h);
            assert!(frame.damage.len() <= 1);
            assert_eq!(frame.pixels, reference.rasterize_pixels(&cells, 3, 3, w, h, alpha).unwrap());
            if previous.len() == frame.pixels.len() { assert_damage_covers_changes(&previous, &frame, w as usize); }
            previous = frame.pixels;
        }
    }

    fn retained_cell(x: usize, y: usize, ch: char, attrs: u64) -> DirectCell {
        DirectCell {
            x,
            y,
            ch,
            attrs,
            color: (190, 80, 230),
            background: Some((12, 40, 70)),
        }
    }

    #[test]
    fn retained_opaque_black_matches_source_over_for_tints_styles_and_removal() {
        let mut retained = DirectGlyphAtlas::new(9, 16, 13.0);
        retained.tile_copy = true;
        let mut reference = DirectGlyphAtlas::new(9, 16, 13.0);
        for frame in 0..12 {
            let cells: Vec<_> = (0..18)
                .filter(|i| (i + frame) % 4 != 0)
                .map(|i| {
                    let mut cell = retained_cell(
                        i % 6,
                        i / 6,
                        ['A', '█', ' ', 'é', '░', '.'][i % 6],
                        [0, 1, 2, 4, 64, 32][(i + frame) % 6],
                    );
                    cell.color = ((frame * 21) as u8, (i * 14) as u8, (255 - frame * 19) as u8);
                    cell.background = if (i + frame) % 5 == 0 {
                        Some((0, 31, 92))
                    } else {
                        None
                    };
                    cell
                })
                .collect();
            let actual = retained
                .rasterize_retained_pixels(&cells, 6, 3, 53, 47, false)
                .unwrap();
            let expected = reference
                .rasterize_pixels(&cells, 6, 3, 53, 47, false)
                .unwrap();
            assert_eq!(actual.pixels, expected, "frame {frame}");
            assert!(actual.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        }
        let actual = retained
            .rasterize_retained_pixels(&[], 6, 3, 53, 47, false)
            .unwrap();
        assert_eq!(
            actual.pixels,
            reference
                .rasterize_pixels(&[], 6, 3, 53, 47, false)
                .unwrap()
        );
    }

    #[test]
    fn retained_background_transitions_for_hidden_and_blank_cells_match_full() {
        // A background-only change remains visible when its glyph is concealed
        // or blank, and must replace every pixel of the preceding glyph.
        for tile_copy in [false, true] {
            let mut retained = DirectGlyphAtlas::new(4, 7, 6.0);
            retained.tile_copy = tile_copy;
            let mut reference = DirectGlyphAtlas::new(4, 7, 6.0);
            let mut cells = vec![retained_cell(2, 2, 'A', 0)];
            let mut previous = Vec::new();
            let mut first: Option<RetainedPixels> = None;
            let mut first_expected = Vec::new();
            for frame in 0..10 {
                match frame {
                    1 => {
                        cells[0].attrs = 32;
                        cells[0].background = Some((80, 10, 30));
                    }
                    2 => cells[0].background = Some((20, 90, 50)),
                    3 => {
                        cells[0].ch = ' ';
                        cells[0].attrs = 0;
                        cells[0].background = Some((40, 60, 100));
                    }
                    4 => cells[0].background = Some((110, 70, 10)),
                    5 => cells[0].background = None,
                    6 => cells[0].ch = 'A',
                    7 => cells.clear(),
                    8 => cells.push(retained_cell(2, 2, 'A', 0)),
                    9 => {
                        cells[0].attrs = 32;
                        cells[0].background = Some((30, 100, 80));
                    }
                    _ => {}
                }
                let (width, height) = if frame < 8 { (17, 23) } else { (19, 25) };
                let actual = retained
                    .rasterize_retained_pixels(&cells, 3, 3, width, height, false)
                    .unwrap();
                let expected = reference
                    .rasterize_pixels(&cells, 3, 3, width, height, false)
                    .unwrap();
                assert_eq!(actual.pixels, expected, "tile_copy={tile_copy}, frame={frame}");
                assert!(actual.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
                if previous.len() == actual.pixels.len() {
                    assert_damage_covers_changes(&previous, &actual, width as usize);
                }
                if matches!(frame, 1..=5 | 7 | 9) {
                    let solid = cells.first().and_then(|cell| cell.background)
                        .map_or([0, 0, 0, 255], |(r, g, b)| [b, g, r, 255]);
                    for (index, pixel) in actual.pixels.chunks_exact(4).enumerate() {
                        let (x, y) = (index % width as usize, index / width as usize);
                        let inside = x >= (width * 2 / 3) as usize
                            && y >= (height * 2 / 3) as usize;
                        assert_eq!(pixel, if inside { &solid } else { &[0, 0, 0, 255] });
                    }
                }
                assert_eq!(actual.full_redraw, frame == 0 || frame == 8);
                if let Some(saved) = &first {
                    assert_eq!(saved.pixels, first_expected, "owned frame changed at {frame}");
                    if frame == 1 {
                        assert_ne!(actual.pixels, saved.pixels, "transition must visibly change pixels");
                    }
                }
                previous = actual.pixels.clone();
                if frame == 0 {
                    first = Some(actual);
                    first_expected = expected;
                }
            }
        }
    }

    fn assert_damage_covers_changes(before: &[u8], result: &RetainedPixels, width: usize) {
        for (index, (a, b)) in before
            .chunks_exact(4)
            .zip(result.pixels.chunks_exact(4))
            .enumerate()
        {
            if a == b {
                continue;
            }
            let (x, y) = ((index % width) as i32, (index / width) as i32);
            assert!(
                result
                    .damage
                    .iter()
                    .any(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height),
                "uncovered pixel {x},{y}"
            );
        }
    }

    #[test]
    fn retained_sequences_match_reference_with_removal_movement_and_styles() {
        let mut retained = DirectGlyphAtlas::new(4, 7, 6.0);
        retained.tile_copy = true;
        let mut reference = DirectGlyphAtlas::new(4, 7, 6.0);
        let mut cells = vec![
            retained_cell(0, 0, 'A', 0),
            retained_cell(2, 1, '█', 1),
            retained_cell(1, 0, ' ', 4 | 64),
        ];
        let mut previous = Vec::new();
        for frame in 0..16 {
            match frame {
                1 => cells[0].color = (20, 220, 60),
                2 => cells[0].background = None,
                3 => cells[0].attrs = 1 | 2 | 4 | 64,
                4 => cells[0].attrs = 32,
                5 => cells[0].color = (230, 30, 80),
                6 => cells[0].attrs = 2,
                7 => {
                    cells.remove(1);
                }
                8 => {
                    cells[0].x = 2;
                    cells[0].y = 2;
                }
                9 => {
                    cells[1].attrs = 0;
                }
                10 => {
                    cells[1].color = (10, 250, 90);
                }
                11 => {
                    cells[1].attrs = 4;
                }
                12 => {
                    cells[1].attrs = 64;
                }
                13 => cells.reverse(),
                14 => cells.clear(),
                _ => {}
            }
            let result = retained
                .rasterize_retained_pixels(&cells, 3, 3, 17, 23, false)
                .unwrap();
            assert_eq!(
                result.pixels,
                reference
                    .rasterize_pixels(&cells, 3, 3, 17, 23, false)
                    .unwrap(),
                "frame {frame}"
            );
            if !previous.is_empty() {
                assert_damage_covers_changes(&previous, &result, 17);
            }
            if frame == 5 || frame == 10 || frame == 13 || frame == 15 {
                assert!(result.damage.is_empty(), "invisible change {frame}");
            }
            previous = result.pixels;
        }
    }

    #[test]
    fn retained_tile_copy_matches_legacy_and_full_across_resize_and_alpha_modes() {
        let mut fast = DirectGlyphAtlas::new(6, 8, 7.0);
        fast.tile_copy = true;
        let mut legacy = DirectGlyphAtlas::new(6, 8, 7.0);
        legacy.tile_copy = false;
        let mut reference = DirectGlyphAtlas::new(6, 8, 7.0);
        for frame in 0..24 {
            let (width, height) = if frame < 8 || frame >= 16 {
                (17, 23)
            } else {
                (20, 19)
            };
            let transparent = frame >= 20;
            let cells: Vec<_> = (0..9)
                .filter(|i| (i + frame) % 5 != 0)
                .map(|i| {
                    let mut cell = retained_cell(
                        i % 3,
                        i / 3,
                        ['A', '█', ' ', 'é', '░', '.'][i % 6],
                        [0, 1, 2, 4, 64, 32, 1 | 2 | 4 | 64][(i + frame) % 7],
                    );
                    cell.color = ((frame * 9) as u8, (i * 27) as u8, (255 - frame * 8) as u8);
                    cell.background = if (i + frame) % 3 == 0 {
                        Some((42, 12, 98))
                    } else {
                        None
                    };
                    cell
                })
                .collect();
            let actual = fast
                .rasterize_retained_pixels(&cells, 3, 3, width, height, transparent)
                .unwrap();
            let old = legacy
                .rasterize_retained_pixels(&cells, 3, 3, width, height, transparent)
                .unwrap();
            let expected = reference
                .rasterize_pixels(&cells, 3, 3, width, height, transparent)
                .unwrap();
            assert_eq!(actual.pixels, expected, "full frame {frame}");
            assert_eq!(actual.pixels, old.pixels, "legacy frame {frame}");
            assert_eq!(actual.damage, old.damage, "damage frame {frame}");
            assert_eq!(actual.full_redraw, old.full_redraw);
            if frame == 7 {
                assert!(
                    fast.tiles.keys().any(|key| key.4),
                    "copy path was exercised"
                );
                assert!(legacy.tiles.keys().all(|key| !key.4));
            }
        }
        let empty = fast
            .rasterize_retained_pixels(&[], 3, 3, 17, 23, false)
            .unwrap();
        assert_eq!(
            empty.pixels,
            reference
                .rasterize_pixels(&[], 3, 3, 17, 23, false)
                .unwrap()
        );
    }

    #[test]
    fn retained_resize_transparency_duplicates_and_external_geometry_use_full_fallback() {
        let mut retained = DirectGlyphAtlas::new(4, 7, 6.0);
        let mut reference = DirectGlyphAtlas::new(4, 7, 6.0);
        let a = retained_cell(0, 0, 'A', 2 | 4);
        let mut b = retained_cell(0, 0, '█', 64);
        b.background = None;
        let cases = [
            (vec![a.clone()], 3, 2, 17, 19, false),
            (vec![a.clone()], 4, 3, 17, 19, false),
            (vec![a.clone()], 4, 3, 21, 17, false),
            (vec![a.clone()], 4, 3, 21, 17, true),
            (vec![a.clone()], 4, 3, 21, 17, true),
            (vec![a.clone()], 4, 3, 21, 17, false),
            (vec![a.clone(), b.clone()], 4, 3, 21, 17, false),
            (vec![b, a.clone()], 4, 3, 21, 17, false),
            (vec![a.clone()], 4, 3, 21, 17, false),
            (vec![a.clone()], 0, 0, 0, -1, false),
        ];
        for (cells, cols, rows, w, h, alpha) in cases {
            let result = retained
                .rasterize_retained_pixels(&cells, cols, rows, w, h, alpha)
                .unwrap();
            assert!(result.full_redraw);
            assert_eq!(
                result.pixels,
                reference
                    .rasterize_pixels(&cells, cols, rows, w, h, alpha)
                    .unwrap()
            );
        }
        retained
            .rasterize_retained_pixels(&[a.clone()], 4, 3, 21, 17, false)
            .unwrap();
        retained.rasterize_pixels(&[], 2, 2, 9, 9, false).unwrap();
        let result = retained
            .rasterize_retained_pixels(&[a.clone()], 4, 3, 21, 17, false)
            .unwrap();
        assert!(result.full_redraw);
        assert_eq!(
            result.pixels,
            reference
                .rasterize_pixels(&[a], 4, 3, 21, 17, false)
                .unwrap()
        );
    }

    #[test]
    fn retained_clipping_zero_intervals_and_owned_results_are_independent() {
        let mut retained = DirectGlyphAtlas::new(4, 7, 6.0);
        let mut reference = DirectGlyphAtlas::new(4, 7, 6.0);
        let mut cells = vec![
            retained_cell(0, 0, 'A', 0),
            retained_cell(4, 3, '█', 0),
            retained_cell(usize::MAX, usize::MAX, 'Z', 0),
        ];
        let first = retained
            .rasterize_retained_pixels(&cells, 5, 4, 3, 2, false)
            .unwrap();
        let saved = first.pixels.clone();
        for frame in 0..4 {
            cells[0].color = (frame * 50, 20, 30);
            cells[1].background = if frame % 2 == 0 {
                None
            } else {
                Some((100, 30, 20))
            };
            let result = retained
                .rasterize_retained_pixels(&cells, 5, 4, 3, 2, false)
                .unwrap();
            assert_eq!(
                result.pixels,
                reference
                    .rasterize_pixels(&cells, 5, 4, 3, 2, false)
                    .unwrap()
            );
            assert_eq!(first.pixels, saved);
        }
        let mut result = retained
            .rasterize_retained_pixels(&cells, 5, 4, 3, 2, false)
            .unwrap();
        result.pixels.fill(123);
        assert_eq!(
            retained
                .rasterize_retained_pixels(&cells, 5, 4, 3, 2, false)
                .unwrap()
                .pixels,
            reference
                .rasterize_pixels(&cells, 5, 4, 3, 2, false)
                .unwrap()
        );
    }

    #[test]
    fn retained_metadata_limit_does_not_keep_an_oversized_grid() {
        let mut atlas = DirectGlyphAtlas::new(1, 1, 1.0);
        atlas
            .rasterize_retained_pixels(&[], 1, 1, 1, 1, false)
            .unwrap();
        let result = atlas
            .rasterize_retained_pixels(&[], 1001, 1000, 1, 1, false)
            .unwrap();
        assert!(result.full_redraw);
        assert!(atlas.retained.is_none());
        assert_eq!(result.pixels, vec![0, 0, 0, 255]);
    }

    #[test]
    fn retained_cairo_error_discards_partial_frame_and_next_call_rebuilds() {
        // A framebuffer can fit our bounded cache while its individual glyph
        // tile exceeds Cairo's image-surface dimension limit. Empty frames do
        // not create a tile; introducing a glyph reaches the real error path.
        let mut atlas = DirectGlyphAtlas::new(1, 1, 1.0);
        atlas
            .rasterize_retained_pixels(&[], 1, 1, 40_000, 1, false)
            .unwrap();
        assert!(atlas
            .rasterize_retained_pixels(&[retained_cell(0, 0, 'A', 0)], 1, 1, 40_000, 1, false)
            .is_err());
        assert!(atlas.retained.is_none());
        let recovered = atlas
            .rasterize_retained_pixels(&[], 1, 1, 40_000, 1, false)
            .unwrap();
        assert!(recovered.full_redraw);
        assert!(recovered
            .pixels
            .chunks_exact(4)
            .all(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn colored_glyph_tiles_cache_premultiplied_bgra_by_glyph_color_and_alpha_mode() {
        let mut atlas = DirectGlyphAtlas::new(4, 7, 6.0);
        let first = atlas
            .colored_tile('█', (200, 100, 50), true)
            .unwrap()
            .as_ptr();
        let second = atlas
            .colored_tile('█', (200, 100, 50), true)
            .unwrap()
            .as_ptr();
        let opaque = atlas
            .colored_tile('█', (200, 100, 50), false)
            .unwrap()
            .as_ptr();
        let recolored = atlas
            .colored_tile('█', (50, 100, 200), true)
            .unwrap()
            .as_ptr();

        assert_eq!(first, second);
        assert_ne!(first, opaque);
        assert_ne!(first, recolored);
        assert_eq!(atlas.tiles.len(), 3);
    }

    #[test]
    fn changing_rgb_cannot_grow_the_colored_glyph_cache_without_bound() {
        let mut atlas = DirectGlyphAtlas::new(4, 7, 6.0);
        for value in 0..9_000u32 {
            atlas
                .colored_tile_styled_mode(
                    'A',
                    ((value >> 16) as u8, (value >> 8) as u8, value as u8),
                    true,
                    0,
                    value % 2 == 0,
                )
                .unwrap();
        }
        assert!(atlas.tiles.len() <= 8_192);
        let latest = atlas.colored_tile('A', (0, 35, 39), true).unwrap().as_ptr();
        assert_eq!(
            latest,
            atlas.colored_tile('A', (0, 35, 39), true).unwrap().as_ptr()
        );
    }

    #[test]
    fn opaque_copy_tiles_change_only_alpha_and_share_the_cache() {
        let mut atlas = DirectGlyphAtlas::new(9, 16, 13.0);
        for attrs in [0, 1, 2, 4, 64, 1 | 2 | 4 | 64] {
            let source = atlas
                .colored_tile_styled('A', (250, 19, 130), false, attrs)
                .unwrap()
                .to_vec();
            let opaque = atlas
                .colored_tile_styled_mode('A', (250, 19, 130), false, attrs, true)
                .unwrap();
            for (a, b) in source.chunks_exact(4).zip(opaque.chunks_exact(4)) {
                assert_eq!(&a[..3], &b[..3]);
                assert_eq!(b[3], 255);
            }
            assert!(source.chunks_exact(4).any(|p| p[3] < 255));
        }
        assert_eq!(atlas.tiles.len(), 12);
        assert_eq!(atlas.tile_order.len(), atlas.tiles.len());
    }

    #[test]
    fn cells_outside_the_grid_are_ignored_without_overflow_or_pixels() {
        let mut atlas = DirectGlyphAtlas::new(4, 7, 6.0);
        let cells = [
            DirectCell {
                x: usize::MAX,
                y: 0,
                ch: 'A',
                color: (200, 100, 50),
                background: None,
                attrs: 0,
            },
            DirectCell {
                x: 0,
                y: usize::MAX,
                ch: 'B',
                color: (50, 100, 200),
                background: None,
                attrs: 0,
            },
        ];
        for transparent in [true, false] {
            let pixels = atlas
                .rasterize_pixels(&cells, 3, 1, 9, 7, transparent)
                .unwrap();
            assert!(pixels.chunks_exact(4).all(|px| px[..3] == [0, 0, 0]));
            assert!(
                atlas.tiles.is_empty(),
                "off-grid cells must not fill the glyph cache"
            );
        }
    }

    #[test]
    fn styled_overlapping_cells_match_cairo_surface_bytes_in_both_modes() {
        let mut atlas = DirectGlyphAtlas::new(9, 13, 12.0);
        let cells = [
            DirectCell {
                x: 0,
                y: 0,
                ch: 'W',
                color: (255, 255, 255),
                background: Some((11, 22, 33)),
                attrs: 1 | 2 | 4,
            },
            DirectCell {
                x: 1,
                y: 0,
                ch: 'A',
                color: (200, 50, 80),
                background: Some((60, 90, 120)),
                attrs: 16 | 64,
            },
            DirectCell {
                x: 1,
                y: 1,
                ch: ' ',
                color: (0, 0, 0),
                background: Some((8, 16, 32)),
                attrs: 0,
            },
        ];
        for transparent in [true, false] {
            let mut surface = atlas.rasterize(&cells, 3, 2, 17, 23, transparent).unwrap();
            let expected = surface.data().unwrap().to_vec();
            assert_eq!(
                atlas
                    .rasterize_pixels(&cells, 3, 2, 17, 23, transparent)
                    .unwrap(),
                expected
            );
            // The colored space is a solid rectangle, even without a glyph.
            assert_eq!(&expected[(12 * 17 + 6) * 4..][..4], &[32, 16, 8, 255]);
        }
    }

    #[test]
    fn colored_space_paints_only_its_own_cell_even_on_a_nondivisible_surface() {
        let mut atlas = DirectGlyphAtlas::new(4, 7, 6.0);
        let cells = [DirectCell {
            x: 0,
            y: 0,
            ch: ' ',
            color: (255, 255, 255),
            background: Some((18, 52, 86)),
            attrs: 0,
        }];
        for transparent in [true, false] {
            let pixels = atlas
                .rasterize_pixels(&cells, 3, 1, 10, 7, transparent)
                .unwrap();
            for y in 0..7 {
                for x in 0..10 {
                    let px = &pixels[(y * 10 + x) * 4..][..4];
                    if x < 3 {
                        assert_eq!(px, &[86, 52, 18, 255]);
                    } else if transparent {
                        assert_eq!(px, &[0, 0, 0, 0]);
                    } else {
                        assert_eq!(px, &[0, 0, 0, 255]);
                    }
                }
            }
        }
    }

    #[test]
    fn unbacked_styled_spaces_render_visible_lines_in_both_alpha_modes() {
        let mut atlas = DirectGlyphAtlas::new(12, 16, 14.0);
        for attrs in [4, 64] {
            let cells = [DirectCell {
                x: 0,
                y: 0,
                ch: ' ',
                color: (220, 90, 30),
                background: None,
                attrs,
            }];
            for transparent in [true, false] {
                let pixels = atlas
                    .rasterize_pixels(&cells, 1, 1, 12, 16, transparent)
                    .unwrap();
                assert!(
                    pixels
                        .chunks_exact(4)
                        .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0),
                    "attrs={attrs} transparent={transparent} must paint the decoration"
                );
            }
        }
    }

    #[test]
    fn glyph_tiles_stop_at_exact_grid_boundaries_with_colored_neighbors() {
        let mut atlas = DirectGlyphAtlas::new(4, 4, 6.0);
        let cells = [
            DirectCell {
                x: 1,
                y: 0,
                ch: ' ',
                color: (0, 0, 0),
                background: Some((17, 43, 89)),
                attrs: 0,
            },
            DirectCell {
                x: 0,
                y: 1,
                ch: ' ',
                color: (0, 0, 0),
                background: Some((91, 37, 13)),
                attrs: 0,
            },
            DirectCell {
                x: 0,
                y: 0,
                ch: 'A',
                color: (255, 255, 255),
                background: None,
                attrs: 4,
            },
        ];
        // 10 / 3 places the first cell at [0,3) in both axes, but its
        // rounded-up glyph tile is 4x4. These are independent pixel oracles,
        // not comparisons to rasterize() (which also calls draw_into()).
        for transparent in [true, false] {
            let pixels = atlas
                .rasterize_pixels(&cells, 3, 3, 10, 10, transparent)
                .unwrap();
            let base = if transparent {
                [0, 0, 0, 0]
            } else {
                [0, 0, 0, 255]
            };
            let mut own_glyph_pixels = 0;
            for y in 0..10 {
                for x in 0..10 {
                    let px = &pixels[(y * 10 + x) * 4..][..4];
                    let expected = if (3..6).contains(&x) && y < 3 {
                        Some([89, 43, 17, 255])
                    } else if x < 3 && (3..6).contains(&y) {
                        Some([13, 37, 91, 255])
                    } else if x >= 3 || y >= 3 {
                        Some(base)
                    } else {
                        own_glyph_pixels += usize::from(px != base);
                        None
                    };
                    if let Some(expected) = expected {
                        assert_eq!(px, expected, "x={x} y={y} transparent={transparent}");
                    }
                }
            }
            assert!(
                own_glyph_pixels > 0,
                "the glyph must paint inside its own cell"
            );
        }
    }

    #[test]
    fn hidden_glyph_paints_background_without_foreground() {
        let mut atlas = DirectGlyphAtlas::new(8, 12, 10.0);
        let cells = [DirectCell {
            x: 0,
            y: 0,
            ch: '█',
            color: (255, 255, 255),
            background: Some((10, 20, 30)),
            attrs: 32,
        }];
        let pixels = atlas.rasterize_pixels(&cells, 1, 1, 8, 12, true).unwrap();
        assert!(pixels.chunks_exact(4).all(|px| px == [30, 20, 10, 255]));
    }

    #[test]
    fn style_mask_is_not_identical_to_unstyled_mask_and_is_bounded() {
        let mut atlas = DirectGlyphAtlas::new(20, 24, 20.0);
        let regular = atlas.mask_for('X', 0).unwrap().to_vec();
        let bold = atlas.mask_for('X', 1).unwrap().to_vec();
        let italic = atlas.mask_for('X', 2).unwrap().to_vec();
        let underline = atlas.mask_for('X', 4).unwrap().to_vec();
        let strike = atlas.mask_for('X', 64).unwrap().to_vec();
        assert_ne!(regular, bold);
        assert_ne!(regular, italic);
        assert_ne!(regular, underline);
        assert_ne!(regular, strike);
        for n in 0..400u32 {
            let _ = atlas
                .mask_for(char::from_u32(0x400 + n).unwrap(), 1)
                .unwrap();
        }
        assert!(atlas.masks.len() <= MAX_GLYPH_MASKS);
    }

    #[test]
    #[ignore = "repeatable native-only raster microbenchmark; not an on-screen comparison"]
    fn benchmark_dense_owned_pixels() {
        let mut atlas = DirectGlyphAtlas::new(3, 5, 5.0);
        let cols = 533;
        let rows = 176;
        let cells: Vec<_> = (0..rows)
            .flat_map(|y| {
                (0..cols).filter_map(move |x| {
                    if (x + y) % 4 != 0 {
                        return None;
                    }
                    Some(DirectCell {
                        x,
                        y,
                        ch: ['A', 'B', 'C', 'D'][(x + y) % 4],
                        color: ((x % 16) as u8 * 16, (y % 16) as u8 * 16, 100),
                        background: None,
                        attrs: 0,
                    })
                })
            })
            .collect();
        for _ in 0..3 {
            std::hint::black_box(
                atlas
                    .rasterize_pixels(&cells, cols, rows, 1280, 800, true)
                    .unwrap(),
            );
        }
        let start = std::time::Instant::now();
        for _ in 0..30 {
            std::hint::black_box(
                atlas
                    .rasterize_pixels(&cells, cols, rows, 1280, 800, true)
                    .unwrap(),
            );
        }
        eprintln!(
            "dense_owned_ms_per_frame={:.4}",
            start.elapsed().as_secs_f64() * 1000.0 / 30.0
        );
    }

    #[test]
    fn owned_pixels_match_cairo_surface_for_transparent_and_opaque_overlaps() {
        let cells = [
            DirectCell {
                x: 0,
                y: 0,
                ch: '█',
                color: (200, 100, 50),
                background: None,
                attrs: 0,
            },
            DirectCell {
                x: 1,
                y: 0,
                ch: 'A',
                color: (50, 100, 200),
                background: None,
                attrs: 0,
            },
        ];
        for transparent in [true, false] {
            let mut atlas = DirectGlyphAtlas::new(4, 7, 6.0);
            let mut reference = atlas.rasterize(&cells, 3, 1, 9, 7, transparent).unwrap();
            let expected = reference.data().unwrap().to_vec();
            let pixels = atlas
                .rasterize_pixels(&cells, 3, 1, 9, 7, transparent)
                .unwrap();
            assert_eq!(pixels, expected);
        }
    }

    #[test]
    fn owned_pixels_match_cairo_after_grid_and_surface_resize() {
        let mut atlas = DirectGlyphAtlas::new(4, 7, 6.0);
        let cells = [DirectCell {
            x: 2,
            y: 1,
            ch: '█',
            color: (200, 100, 50),
            background: None,
            attrs: 0,
        }];
        for (cols, rows, width, height) in [(3, 2, 9, 9), (5, 3, 13, 11), (3, 2, 9, 9)] {
            let mut reference = atlas
                .rasterize(&cells, cols, rows, width, height, true)
                .unwrap();
            let expected = reference.data().unwrap().to_vec();
            let pixels = atlas
                .rasterize_pixels(&cells, cols, rows, width, height, true)
                .unwrap();
            assert_eq!(pixels, expected);
        }
    }
}

impl DirectGlyphAtlas {
    pub(super) fn new(cell_w: i32, cell_h: i32, font_size: f64) -> Self {
        Self {
            cell_w: cell_w.max(1),
            cell_h: cell_h.max(1),
            font_size,
            geometry: (0, 0, 0, 0),
            x_positions: Vec::new(),
            y_positions: Vec::new(),
            masks: HashMap::new(),
            mask_order: VecDeque::new(),
            tiles: HashMap::new(),
            tile_order: VecDeque::new(),
            retained: None,
            tile_copy: std::env::var("TTFX_BG_TILECOPY").ok().as_deref() == Some("1"),
        }
    }

    pub(super) fn rasterize_pixels(
        &mut self,
        cells: &[DirectCell],
        cols: usize,
        rows: usize,
        width: i32,
        height: i32,
        transparent: bool,
    ) -> Result<Vec<u8>> {
        let (width, height) = (width.max(1), height.max(1));
        let stride = width as usize * 4;
        let mut pixels = vec![0; stride * height as usize];
        self.draw_into(
            &mut pixels,
            stride,
            cells,
            cols,
            rows,
            width,
            height,
            transparent,
            true,
        )?;
        Ok(pixels)
    }

    // Unique grid positions are independent: both glyphs and backgrounds are
    // clipped to their exact grid interval. Keep the reference rasterizer above
    // untouched; unsupported input and alpha modes take that full path.
    pub(super) fn rasterize_retained_pixels(
        &mut self,
        cells: &[DirectCell],
        cols: usize,
        rows: usize,
        width: i32,
        height: i32,
        transparent: bool,
    ) -> Result<RetainedPixels> {
        let (width, height) = (width.max(1), height.max(1));
        let geometry = (cols, rows, width, height);
        let full_damage = || {
            vec![DamageRect {
                x: 0,
                y: 0,
                width,
                height,
            }]
        };
        let count = cols.checked_mul(rows);
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4));
        let bounded = cols != 0
            && rows != 0
            && count.is_some_and(|n| n <= MAX_RETAINED_CELLS)
            && bytes.is_some_and(|n| n <= MAX_RETAINED_BYTES)
            && cells.len() <= MAX_RETAINED_CELLS;
        if transparent || !bounded {
            self.retained = None;
            return Ok(RetainedPixels {
                pixels: self.rasterize_pixels(cells, cols, rows, width, height, transparent)?,
                damage: full_damage(),
                full_redraw: true,
            });
        }

        let mut next = vec![None; count.unwrap()];
        for cell in cells {
            if cell.x >= cols || cell.y >= rows {
                continue;
            }
            let slot = &mut next[cell.y * cols + cell.x];
            if slot.is_some() {
                // Duplicate positions can blend in input order. Never reuse a
                // per-position cache for those frames, including the next one.
                self.retained = None;
                return Ok(RetainedPixels {
                    pixels: self.rasterize_pixels(cells, cols, rows, width, height, false)?,
                    damage: full_damage(),
                    full_redraw: true,
                });
            }
            *slot = Some(cell.clone());
        }
        let old = self.retained.take();
        let Some(mut retained) =
            old.filter(|r| r.geometry == geometry && self.geometry == geometry)
        else {
            let pixels = self.rasterize_pixels(cells, cols, rows, width, height, false)?;
            self.retained = Some(RetainedFrame {
                geometry,
                pixels: pixels.clone(),
                cells: next,
            });
            return Ok(RetainedPixels {
                pixels,
                damage: full_damage(),
                full_redraw: true,
            });
        };

        let mut damage: Vec<DamageRect> = Vec::new();
        let stride = width as usize * 4;
        let repaint = (|| -> Result<()> {
            for (index, (before, after)) in retained.cells.iter().zip(&next).enumerate() {
                if Self::same_visible_cell(before.as_ref(), after.as_ref()) {
                    continue;
                }
                let x = index % cols;
                let y = index / cols;
                let x0 = self.x_positions[x];
                let y0 = self.y_positions[y];
                let x1 = self.x_positions.get(x + 1).copied().unwrap_or(width);
                let y1 = self.y_positions.get(y + 1).copied().unwrap_or(height);
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                // An explicit background is painted over the whole interval
                // before glyphs, so it already replaces every previous pixel.
                // Removed cells and glyphs without a background still need black.
                if after.as_ref().is_none_or(|cell| cell.background.is_none()) {
                    for py in y0..y1 {
                        let row = &mut retained.pixels[py as usize * stride + x0 as usize * 4
                            ..py as usize * stride + x1 as usize * 4];
                        for pixel in row.chunks_exact_mut(4) {
                            pixel.copy_from_slice(&[0, 0, 0, 255]);
                        }
                    }
                }
                if let Some(cell) = after {
                    self.paint_retained_cell(&mut retained.pixels, stride, cell, x0, y0, x1, y1)?;
                }
                // Coalesce adjacent cells within a row without expanding damage
                // into other rows. The rectangle list is bounded by grid size.
                if let Some(last) = damage.last_mut().filter(|last| {
                    last.y == y0 && last.height == y1 - y0 && last.x + last.width == x0
                }) {
                    last.width += x1 - x0;
                } else {
                    damage.push(DamageRect {
                        x: x0,
                        y: y0,
                        width: x1 - x0,
                        height: y1 - y0,
                    });
                }
            }
            Ok(())
        })();
        if repaint.is_err() {
            // The partially updated buffer has been removed from the cache.
            // Propagate a reference-path error, or return a complete fresh frame.
            return Ok(RetainedPixels {
                pixels: self.rasterize_pixels(cells, cols, rows, width, height, false)?,
                damage: full_damage(),
                full_redraw: true,
            });
        }
        retained.cells = next;
        let pixels = retained.pixels.clone();
        self.retained = Some(retained);
        Ok(RetainedPixels {
            pixels,
            damage,
            full_redraw: false,
        })
    }

    fn same_visible_cell(before: Option<&DirectCell>, after: Option<&DirectCell>) -> bool {
        match (before, after) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.x == b.x
                    && a.y == b.y
                    && a.ch == b.ch
                    && a.attrs == b.attrs
                    && a.background == b.background
                    && (a.color == b.color
                        || a.attrs & 32 != 0
                        || (a.ch == ' ' && a.attrs & (4 | 64) == 0))
            }
            _ => false,
        }
    }

    fn paint_retained_cell(
        &mut self,
        data: &mut [u8],
        stride: usize,
        cell: &DirectCell,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
    ) -> Result<()> {
        if let Some((r, g, b)) = cell.background {
            for y in y0..y1 {
                for x in x0..x1 {
                    let start = y as usize * stride + x as usize * 4;
                    data[start..start + 4].copy_from_slice(&[b, g, r, 255]);
                }
            }
        }
        if (cell.ch == ' ' && cell.attrs & (4 | 64) == 0) || cell.attrs & 32 != 0 {
            return Ok(());
        }
        let (cell_w, cell_h) = (self.cell_w, self.cell_h);
        let opaque_copy = self.tile_copy && cell.background.is_none();
        if opaque_copy {
            // RGB is the same premultiplied source-over result on opaque black.
            // A separate cache mode supplies alpha 255, preserving source alpha
            // in the reference and colored-background paths. Both modes share
            // the same bounded tile table and FIFO eviction policy.
            let tile =
                self.colored_tile_styled_mode(cell.ch, cell.color, false, cell.attrs, true)?;
            let copy_width = cell_w.min(x1 - x0) as usize * 4;
            for sy in 0..cell_h.min(y1 - y0) as usize {
                let tile_row = &tile[sy * cell_w as usize * 4..][..copy_width];
                let pixel_row =
                    &mut data[(y0 as usize + sy) * stride + x0 as usize * 4..][..copy_width];
                pixel_row.copy_from_slice(tile_row);
            }
            return Ok(());
        }
        let tile = self.colored_tile_styled(cell.ch, cell.color, false, cell.attrs)?;
        let copy_width = cell_w.min(x1 - x0) as usize * 4;
        let copy_height = cell_h.min(y1 - y0) as usize;
        // Only this retained, previously cleared opaque interval may force
        // alpha255. Other backgrounds, transparency, and TILECOPY keep their
        // existing paths. The safe glue checks spans before the leaf ASM call.
        if cell.background.is_none() && crate::opaque_bgra::try_copy_opaque(
            &mut data[y0 as usize * stride + x0 as usize * 4..], tile,
            copy_width / 4, copy_height, stride, cell_w as usize * 4,
        ) {
            return Ok(());
        }
        for sy in 0..copy_height {
            let tile_row = &tile[sy * cell_w as usize * 4..][..copy_width];
            let pixel_row =
                &mut data[(y0 as usize + sy) * stride + x0 as usize * 4..][..copy_width];
            if cell.background.is_none() {
                // Retained cells are unique and this interval was cleared to
                // opaque black. Source-over leaves premultiplied RGB unchanged
                // and alpha exactly 255, including fully transparent mask pixels.
                for (source, pixel) in tile_row.chunks_exact(4).zip(pixel_row.chunks_exact_mut(4)) {
                    pixel[..3].copy_from_slice(&source[..3]);
                    pixel[3] = 255;
                }
                continue;
            }
            for (source, pixel) in tile_row.chunks_exact(4).zip(pixel_row.chunks_exact_mut(4)) {
                if source[3] == 0 {
                    continue;
                }
                let inv = 255 - source[3] as u16;
                for channel in 0..3 {
                    pixel[channel] =
                        (source[channel] as u16 + pixel[channel] as u16 * inv / 255).min(255) as u8;
                }
                pixel[3] = (source[3] as u16 + pixel[3] as u16 * inv / 255).min(255) as u8;
            }
        }
        Ok(())
    }

    // Retained for exact byte parity tests against Cairo's ARgb32 layout.
    #[cfg(test)]
    pub(super) fn rasterize(
        &mut self,
        cells: &[DirectCell],
        cols: usize,
        rows: usize,
        width: i32,
        height: i32,
        transparent: bool,
    ) -> Result<ImageSurface> {
        let (width, height) = (width.max(1), height.max(1));
        let mut surface = ImageSurface::create(Format::ARgb32, width, height)?;
        let stride = surface.stride() as usize;
        {
            let mut data = surface.data()?;
            self.draw_into(
                &mut data,
                stride,
                cells,
                cols,
                rows,
                width,
                height,
                transparent,
                false,
            )?;
        }
        surface.mark_dirty();
        Ok(surface)
    }

    fn draw_into(
        &mut self,
        data: &mut [u8],
        stride: usize,
        cells: &[DirectCell],
        cols: usize,
        rows: usize,
        width: i32,
        height: i32,
        transparent: bool,
        zeroed: bool,
    ) -> Result<()> {
        let cell_w = ((width as f64 / cols.max(1) as f64).ceil() as i32).max(1);
        let cell_h = ((height as f64 / rows.max(1) as f64).ceil() as i32).max(1);
        if self.cell_w != cell_w || self.cell_h != cell_h {
            self.cell_w = cell_w;
            self.cell_h = cell_h;
            self.masks.clear();
            self.mask_order.clear();
            self.tiles.clear();
            self.tile_order.clear();
        }
        if self.geometry != (cols, rows, width, height) {
            self.geometry = (cols, rows, width, height);
            self.x_positions = (0..cols)
                .map(|x| (x as f64 * width as f64 / cols.max(1) as f64).floor() as i32)
                .collect();
            self.y_positions = (0..rows)
                .map(|y| (y as f64 * height as f64 / rows.max(1) as f64).floor() as i32)
                .collect();
        }
        if transparent {
            if !zeroed {
                data.fill(0);
            }
        } else {
            for pixel in data.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        for cell in cells {
            let (Some(&x0), Some(&y0)) =
                (self.x_positions.get(cell.x), self.y_positions.get(cell.y))
            else {
                continue;
            };
            if x0 >= width || y0 >= height {
                continue;
            }
            let x1 = self.x_positions.get(cell.x + 1).copied().unwrap_or(width);
            let y1 = self.y_positions.get(cell.y + 1).copied().unwrap_or(height);
            if let Some((r, g, b)) = cell.background {
                // Fill only the actual grid interval, not the rounded-up glyph tile.
                for y in y0.max(0)..y1.min(height) {
                    for x in x0.max(0)..x1.min(width) {
                        let start = y as usize * stride + x as usize * 4;
                        data[start..start + 4].copy_from_slice(&[b, g, r, 255]);
                    }
                }
            }
            if (cell.ch == ' ' && cell.attrs & (4 | 64) == 0) || cell.attrs & 32 != 0 {
                continue;
            }
            let tile = self.colored_tile_styled(cell.ch, cell.color, transparent, cell.attrs)?;
            let copy_width = cell_w.min(x1 - x0).min(width - x0) as usize * 4;
            for sy in 0..cell_h.min(y1 - y0).min(height - y0) as usize {
                let tile_row = &tile[sy * cell_w as usize * 4..][..copy_width];
                let pixel_row =
                    &mut data[(y0 as usize + sy) * stride + x0 as usize * 4..][..copy_width];
                for (source, pixel) in tile_row.chunks_exact(4).zip(pixel_row.chunks_exact_mut(4)) {
                    if source[3] == 0 {
                        continue;
                    }
                    // Cairo ARgb32: premultiplied source-over, for both modes.
                    let inv = 255 - source[3] as u16;
                    for channel in 0..3 {
                        pixel[channel] = (source[channel] as u16
                            + pixel[channel] as u16 * inv / 255)
                            .min(255) as u8;
                    }
                    pixel[3] = (source[3] as u16 + pixel[3] as u16 * inv / 255).min(255) as u8;
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn colored_tile(&mut self, ch: char, color: (u8, u8, u8), transparent: bool) -> Result<&[u8]> {
        self.colored_tile_styled(ch, color, transparent, 0)
    }

    fn colored_tile_styled(
        &mut self,
        ch: char,
        color: (u8, u8, u8),
        transparent: bool,
        attrs: u64,
    ) -> Result<&[u8]> {
        self.colored_tile_styled_mode(ch, color, transparent, attrs, false)
    }

    fn colored_tile_styled_mode(
        &mut self,
        ch: char,
        color: (u8, u8, u8),
        transparent: bool,
        attrs: u64,
        opaque_black: bool,
    ) -> Result<&[u8]> {
        let style = attrs & (1 | 2 | 4 | 64);
        let key = (ch, color, transparent, style, opaque_black);
        let mask_key = (ch, style);
        if self.tiles.len() >= MAX_COLORED_TILES && !self.tiles.contains_key(&key) {
            if let Some(oldest) = self.tile_order.pop_front() {
                self.tiles.remove(&oldest);
            }
        }
        // Ensure the mask exists before borrowing the tile table, so a hit
        // needs one tile lookup and misses colorize directly from the mask.
        if !self.masks.contains_key(&mask_key) {
            self.mask_for(ch, style)?;
        }
        let masks = &self.masks;
        let order = &mut self.tile_order;
        let tile = self.tiles.entry(key).or_insert_with(|| {
            order.push_back(key);
            let mask = &masks[&mask_key];
            let (r, g, b) = color;
            let mut tile = Vec::with_capacity(mask.len() * 4);
            for &alpha in mask {
                let alpha = alpha as u16;
                tile.push((b as u16 * alpha / 255) as u8);
                tile.push((g as u16 * alpha / 255) as u8);
                tile.push((r as u16 * alpha / 255) as u8);
                // Preserve source alpha for blank-pixel skips on opaque frames.
                tile.push(if opaque_black { 255 } else { alpha as u8 });
            }
            tile
        });
        Ok(tile)
    }

    fn mask_for(&mut self, ch: char, attrs: u64) -> Result<&[u8]> {
        let key = (ch, attrs & (1 | 2 | 4 | 64));
        if !self.masks.contains_key(&key) {
            let mut surface = ImageSurface::create(Format::ARgb32, self.cell_w, self.cell_h)?;
            {
                let cr = Context::new(&surface)?;
                if ch == '█' {
                    // FULL BLOCK is the entire cell, independent of font
                    // bearings and antialiasing. Exact grid clipping below
                    // handles cells whose width/height round down by a pixel.
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    cr.paint()?;
                } else {
                    let slant = FontSlant::Normal;
                    let weight = if attrs & 1 != 0 {
                        FontWeight::Bold
                    } else {
                        FontWeight::Normal
                    };
                    cr.select_font_face("monospace", slant, weight);
                    let font = self.font_size.min(self.cell_h as f64 * 0.95);
                    cr.set_font_size(font);
                    let baseline = (self.cell_h as f64 - font) * 0.5 + font;
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    if attrs & 2 != 0 {
                        // Cairo's toy monospace often lacks a distinct italic face.
                        // Shear the glyph geometry so italic remains visible there.
                        cr.transform(gtk4::cairo::Matrix::new(
                            1.0,
                            0.0,
                            -0.22,
                            1.0,
                            self.cell_h as f64 * 0.22,
                            0.0,
                        ));
                    }
                    cr.move_to(0.0, baseline);
                    cr.show_text(&ch.to_string())?;
                    if attrs & (4 | 64) != 0 {
                        cr.set_line_width((font / 12.0).max(1.0));
                        if attrs & 4 != 0 {
                            let y = (baseline + (self.cell_h as f64 - baseline) * 0.45)
                                .min(self.cell_h as f64 - 0.5);
                            cr.move_to(0.0, y);
                            cr.line_to(self.cell_w as f64, y);
                            cr.stroke()?;
                        }
                        if attrs & 64 != 0 {
                            cr.move_to(0.0, baseline - font * 0.35);
                            cr.line_to(self.cell_w as f64, baseline - font * 0.35);
                            cr.stroke()?;
                        }
                    }
                }
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data()?;
            let mut mask = Vec::with_capacity((self.cell_w * self.cell_h) as usize);
            for y in 0..self.cell_h as usize {
                for x in 0..self.cell_w as usize {
                    mask.push(data[y * stride + x * 4 + 3]);
                }
            }
            if self.masks.len() >= MAX_GLYPH_MASKS {
                if let Some(oldest) = self.mask_order.pop_front() {
                    self.masks.remove(&oldest);
                }
            }
            self.mask_order.push_back(key);
            self.masks.insert(key, mask);
        }
        Ok(self.masks.get(&key).expect("glyph inserted"))
    }
}
