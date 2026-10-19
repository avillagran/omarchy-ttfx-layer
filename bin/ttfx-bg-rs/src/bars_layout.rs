//! Horizontal allocation for the 24 audio bands used by Bars.
//! Every column belongs to a proportional band interval. A multi-column
//! interval ends with one blank separator; a single-column bar remains visible.
//! With fewer than 24 columns some intervals are necessarily empty. This does
//! not merge bands or alter their energy, smoothing, height, tint or palette.

use std::ops::Range;

pub const BANDS: usize = 24;

// floor(boundary * columns / BANDS), without overflowing boundary * columns.
fn boundary(boundary: usize, columns: usize) -> usize {
    boundary * (columns / BANDS) + boundary * (columns % BANDS) / BANDS
}

pub fn columns_for_band(band: usize, columns: usize) -> Range<usize> {
    if band >= BANDS {
        return 0..0;
    }
    let start = boundary(band, columns);
    let mut end = boundary(band + 1, columns);
    if end - start >= 2 {
        end -= 1;
    }
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proportional_intervals_cover_grid_and_only_intended_separators_are_blank() {
        for columns in 0..=1000 {
            let mut allocated_end = 0;
            let mut painted = vec![false; columns];
            for band in 0..BANDS {
                let start = boundary(band, columns);
                let end = boundary(band + 1, columns);
                assert_eq!(start, allocated_end);
                allocated_end = end;
                let span = columns_for_band(band, columns);
                assert_eq!(span.start, start);
                assert_eq!(span.end, if end - start >= 2 { end - 1 } else { end });
                for x in span {
                    assert!(!painted[x], "overlap width {columns} band {band}");
                    painted[x] = true;
                }
            }
            assert_eq!(allocated_end, columns);
            // At small widths every available column is a visible one-wide bar.
            if columns <= BANDS {
                assert!(painted.iter().all(|v| *v));
            }
        }
    }

    #[test]
    fn divisible_widths_preserve_original_ranges() {
        for columns in [48, 72, 120, 240, 480, 960] {
            let width = columns / BANDS;
            for band in 0..BANDS {
                assert_eq!(
                    columns_for_band(band, columns),
                    band * width..band * width + width - 1
                );
            }
        }
    }

    #[test]
    fn width_236_reaches_high_bands_and_has_no_unused_trailing_strip() {
        assert_eq!(columns_for_band(0, 236), 0..8);
        assert_eq!(columns_for_band(22, 236), 216..225);
        assert_eq!(columns_for_band(23, 236), 226..235);
        assert_eq!(boundary(BANDS, 236), 236); // only column235 is the intentional gap
    }

    #[test]
    fn zero_invalid_band_and_maximum_width_are_safe() {
        for band in 0..BANDS {
            assert!(columns_for_band(band, 0).is_empty());
        }
        assert!(columns_for_band(BANDS, 236).is_empty());
        assert!(columns_for_band(usize::MAX, 236).is_empty());
        let mut previous = 0;
        for band in 0..BANDS {
            let start = boundary(band, usize::MAX);
            let end = boundary(band + 1, usize::MAX);
            assert_eq!(start, previous);
            assert!(start <= end);
            let span = columns_for_band(band, usize::MAX);
            assert!(span.start <= span.end && span.end <= end);
            previous = end;
        }
        assert_eq!(previous, usize::MAX);
    }
}
