//! Safe glue for the stateless SysV/SSE2 rectangle kernel. No overlapping
//! source/destination slices may exist: ordinary Rust borrows enforce this.
//! The source bytes are already premultiplied BGRA; copy BGR and force A=255.
//! Only the retained opaque/BackgroundNone path has these semantics.

#[cfg(ttfx_bgra_asm)]
unsafe extern "C" {
    fn ttfx_bgra_copy_opaque(
        dst: *mut u8,
        src: *const u8,
        width: usize,
        height: usize,
        dst_stride: usize,
        src_stride: usize,
    );
}

/// Return true only when the complete rectangle was copied by ASM.
/// False leaves the destination untouched, so the caller can use its existing
/// Rust loop. Small rectangles avoid FFI; the threshold must be locally tested.
#[inline]
pub fn try_copy_opaque(
    dst: &mut [u8],
    src: &[u8],
    width: usize,
    height: usize,
    dst_stride: usize,
    src_stride: usize,
) -> bool {
    #[cfg(ttfx_bgra_asm)]
    {
        // At least one vector per row, amortized over multiple rows. This
        // deliberately excludes 1x1 and other small/untested geometries.
        if width < 4 || height < 8 {
            return false;
        }
        let Some(row_bytes) = width.checked_mul(4) else {
            return false;
        };
        if dst_stride < row_bytes || src_stride < row_bytes {
            return false;
        }
        let span = |stride: usize| (height - 1).checked_mul(stride)?.checked_add(row_bytes);
        let (Some(dst_span), Some(src_span)) = (span(dst_stride), span(src_stride)) else {
            return false;
        };
        if dst.len() < dst_span || src.len() < src_span {
            return false;
        }
        // SAFETY: spans cannot overflow and fit the borrowed slices. Strides
        // keep rows disjoint. Shared source and exclusive destination borrows
        // guarantee non-overlap. The kernel uses no state or callbacks.
        unsafe {
            ttfx_bgra_copy_opaque(
                dst.as_mut_ptr(),
                src.as_ptr(),
                width,
                height,
                dst_stride,
                src_stride,
            );
        }
        true
    }
    #[cfg(not(ttfx_bgra_asm))]
    {
        let _ = (dst, src, width, height, dst_stride, src_stride);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_frontier_rejects_without_writes() {
        let source = [13u8; 128];
        for (w, h, ds, ss, dl, sl) in [
            (0, usize::MAX, 16, 16, 128, 128),
            (1, 1, 4, 4, 128, 128),
            (3, 8, 12, 12, 128, 128),
            (4, 7, 16, 16, 128, 128),
            (usize::MAX, 8, usize::MAX, usize::MAX, 128, 128),
            (4, usize::MAX, 16, 16, 128, 128),
            (4, 8, usize::MAX, 16, 128, 128),
            (4, 8, 16, usize::MAX, 128, 128),
            (4, 8, 15, 16, 128, 128),
            (4, 8, 16, 15, 128, 128),
            (4, 8, 16, 16, 127, 128),
            (4, 8, 16, 16, 128, 127),
        ] {
            let mut destination = [0xa5u8; 128];
            assert!(!try_copy_opaque(
                &mut destination[..dl],
                &source[..sl],
                w,
                h,
                ds,
                ss
            ));
            assert_eq!(destination, [0xa5; 128]);
        }
    }

    #[test]
    fn exact_last_row_spans_all_alpha_and_disjoint_borrows_match() {
        // 256 alpha phases, vector+tail, source pitch24 and destination pitch40.
        // Last row ends at the buffer end, without trailing stride padding.
        let (w, h, ss, ds) = (5, 11, 24, 40);
        let sl = (h - 1) * ss + w * 4;
        let dl = (h - 1) * ds + w * 4;
        for salt in 0..256u16 {
            let mut storage = vec![0xa5; sl + dl];
            let (src, dst) = storage.split_at_mut(sl); // one allocation, no overlap
            for y in 0..h {
                for x in 0..w {
                    let alpha = ((salt + y as u16 * 17 + x as u16 * 31) % 256) as u8;
                    src[y * ss + x * 4..][..4].copy_from_slice(&[
                        alpha / 2,
                        alpha / 3,
                        alpha / 4,
                        alpha,
                    ]);
                }
            }
            let before = src.to_vec();
            let mut expected = dst.to_vec();
            for y in 0..h {
                for x in 0..w {
                    expected[y * ds + x * 4..][..3].copy_from_slice(&src[y * ss + x * 4..][..3]);
                    expected[y * ds + x * 4 + 3] = 255;
                }
            }
            let copied = try_copy_opaque(dst, src, w, h, ds, ss);
            assert_eq!(copied, cfg!(ttfx_bgra_asm));
            assert_eq!(src, before);
            if copied {
                assert_eq!(dst, expected);
            } else {
                assert!(dst.iter().all(|b| *b == 0xa5));
            }
        }
    }
}
