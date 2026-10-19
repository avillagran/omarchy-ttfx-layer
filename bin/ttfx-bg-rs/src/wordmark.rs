//! Official Omarchy wordmark geometry, independent of effect palettes.
//! Bitmap sampled on the 15px lattice of Omarchy's official logo.svg (81x19).

/// Wordmark-only input for embedders that anchor the text block themselves
/// (the preview daemon). canvas_input pads the wordmark into a full-size
/// canvas, but the engine drops space-only cells and re-anchors the trimmed
/// block, so the padding either collapses (Sw) or double-centers (C). A
/// tight wordmark block lets the engine's C-anchor center it exactly.
pub fn wordmark_input(text: &str, cols: usize, rows: usize, style: &str, aspect: f32) -> String {
    if !text.trim().eq_ignore_ascii_case("OMARCHY") || cols == 0 || rows == 0 {
        return text.to_string();
    }
    let bitmap = include_str!("omarchy-bitmap.txt");
    let aspect = if aspect.is_finite() {
        aspect.clamp(0.5, 4.0)
    } else {
        2.0
    };
    let pitch = ((cols as f32 * 0.7) / (81.0 * aspect)).min((rows as f32 * 0.6) / 19.0);
    let glyph = super::styled_glyph(style, '█');
    let mut out: Vec<String> = Vec::new();
    if pitch >= 1.0 {
        let scale = pitch as usize;
        let sx = (scale as f32 * aspect).round().max(1.0) as usize;
        for line in bitmap.lines() {
            let mut row = String::new();
            for pixel in line.bytes() {
                let ch = if pixel == b'#' { glyph } else { ' ' };
                for _ in 0..sx {
                    row.push(ch);
                }
            }
            for _ in 0..scale {
                out.push(row.clone());
            }
        }
    } else {
        let pitch = pitch.max(0.05);
        let step_x = (pitch * aspect).max(0.05);
        let step_y = pitch;
        let width = (81.0 * step_x).ceil() as usize + 1;
        let height = ((19.0 * step_y).ceil() as usize).max(1);
        let mut grid = vec![vec![' '; width]; height];
        for (y, line) in bitmap.lines().enumerate() {
            for (x, pixel) in line.bytes().enumerate() {
                if pixel != b'#' {
                    continue;
                }
                let cx = (x as f32 * step_x) as usize;
                let cy = (y as f32 * step_y) as usize;
                if cy < height && cx < width {
                    grid[cy][cx] = glyph;
                }
            }
        }
        out = grid.into_iter().map(|r| r.into_iter().collect()).collect();
    }
    out.join("\n")
}

pub fn canvas_input(text: &str, cols: usize, rows: usize, style: &str, aspect: f32) -> String {
    let mut canvas = vec![vec![' '; cols]; rows];
    if text.trim().eq_ignore_ascii_case("OMARCHY") && cols > 0 && rows > 0 {
        let bitmap = include_str!("omarchy-bitmap.txt");
        let aspect = if aspect.is_finite() {
            aspect.clamp(0.5, 4.0)
        } else {
            2.0
        };
        let scale = (((cols as f32 * 0.7) / (81.0 * aspect)) as usize)
            .min((rows as f32 * 0.6 / 19.0) as usize)
            .max(1);
        let sx = (scale as f32 * aspect).round().max(1.0) as usize;
        let width = 81 * sx;
        let height = 19 * scale;
        let glyph = super::styled_glyph(style, '█');
        if width <= (cols as f32 * 1.1) as usize && height <= rows {
            let left = cols.saturating_sub(width) / 2;
            let top = rows.saturating_sub(height) / 2;
            for (y, line) in bitmap.lines().enumerate() {
                for (x, pixel) in line.bytes().enumerate() {
                    if pixel != b'#' {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..sx {
                            let (cx, cy) = (left + x * sx + dx, top + y * scale + dy);
                            if cx < cols && cy < rows {
                                canvas[cy][cx] = glyph;
                            }
                        }
                    }
                }
            }
        } else {
            // Canvas much smaller than the coarsest integer lattice (preview
            // grids): sample the bitmap at a fractional pitch so the WHOLE
            // wordmark fits inside the box, centered, instead of clipping its
            // top-left corner.
            let pitch = ((cols as f32 * 0.7) / (81.0 * aspect))
                .min((rows as f32 * 0.6) / 19.0)
                .max(0.05);
            let step_x = (pitch * aspect).max(0.05); // cells per bitmap pixel, x
            let step_y = pitch; // cells per bitmap pixel, y
            let width = (81.0 * step_x).ceil() as usize;
            let height = (19.0 * step_y).ceil() as usize;
            let left = cols.saturating_sub(width) / 2;
            let top = rows.saturating_sub(height) / 2;
            for (y, line) in bitmap.lines().enumerate() {
                for (x, pixel) in line.bytes().enumerate() {
                    if pixel != b'#' {
                        continue;
                    }
                    let cx = left + (x as f32 * step_x) as usize;
                    let cy = top + (y as f32 * step_y) as usize;
                    if cx < cols && cy < rows {
                        canvas[cy][cx] = glyph;
                    }
                }
            }
        }
    } else {
        // Custom text is literal text, not a substitute bitmap font. Keep case,
        // punctuation and line breaks; ignore the wordmark pixel-style selector.
        let lines: Vec<Vec<char>> = text
            .lines()
            .map(|line| {
                line.chars()
                    .filter(|ch| !ch.is_control())
                    .take(cols)
                    .collect()
            })
            .take(rows)
            .collect();
        let top = rows.saturating_sub(lines.len()) / 2;
        for (y, line) in lines.iter().enumerate() {
            let left = cols.saturating_sub(line.len()) / 2;
            for (x, &ch) in line.iter().enumerate() {
                canvas[top + y][left + x] = ch;
            }
        }
    }
    canvas
        .iter()
        .map(|row| row.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::super::ttfx_canvas_input;

    #[test]
    fn ordinary_text_keeps_case_punctuation_and_characters() {
        for style in ["native", "block", "hash"] {
            let canvas = ttfx_canvas_input("Hello, world!", 80, 30, style);
            let lines: Vec<_> = canvas
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].trim(), "Hello, world!");
        }
    }

    #[test]
    fn omarchy_matches_every_official_bitmap_cell() {
        let bitmap = include_str!("omarchy-bitmap.txt");
        let expected: Vec<_> = bitmap.lines().collect();
        assert_eq!(expected.len(), 19);
        assert!(expected.iter().all(|row| row.len() == 81));
        let canvas = ttfx_canvas_input("OMARCHY", 200, 64, "native");
        let rows: Vec<Vec<char>> = canvas.lines().map(|line| line.chars().collect()).collect();
        let top = rows
            .iter()
            .position(|row| row.iter().any(|&ch| ch != ' '))
            .unwrap();
        let left = rows
            .iter()
            .filter_map(|row| row.iter().position(|&ch| ch != ' '))
            .min()
            .unwrap();
        for (y, row) in expected.iter().enumerate() {
            for (x, pixel) in row.bytes().enumerate() {
                let glyph = if pixel == b'#' { '█' } else { ' ' };
                assert_eq!(rows[top + y][left + x * 2], glyph, "pixel {x},{y}");
                assert_eq!(rows[top + y][left + x * 2 + 1], glyph, "pixel {x},{y}");
            }
        }
    }
}
