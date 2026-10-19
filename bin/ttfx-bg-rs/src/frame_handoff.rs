//! Consume the latest publication without requesting a redundant raster/upload.
//!
//! The producer must write the slot and increment generation under the same
//! mutex. Equality is exact; no hashes or lossy frame signatures are used.
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct FrameUpdate {
    /// A publication was consumed, even if its cells matched the visible frame.
    pub new_frame: bool,
    /// The presentation needs new pixels (or the caller invalidated geometry).
    pub redraw: bool,
}

pub fn consume_latest<T: PartialEq>(
    snapshot: &Mutex<Vec<T>>,
    generation: &AtomicU64,
    consumed: &Cell<u64>,
    visible: &mut Vec<T>,
    force_redraw: bool,
) -> FrameUpdate {
    let Ok(mut frame) = snapshot.lock() else {
        return FrameUpdate {
            new_frame: false,
            redraw: false,
        };
    };
    // Recheck under the publication lock; an earlier tick check may be stale.
    let next = generation.load(Ordering::Acquire);
    if next == consumed.get() {
        return FrameUpdate {
            new_frame: false,
            redraw: force_redraw,
        };
    }
    consumed.set(next);
    // A changed last cell is common in row-major animated frames. Detect it
    // before walking an otherwise identical prefix; still compare every cell
    // when this sentinel matches, so interior changes can never be skipped.
    let changed = frame.len() != visible.len()
        || frame.last() != visible.last()
        || frame.as_slice() != visible.as_slice();
    if changed {
        std::mem::swap(visible, &mut *frame);
    }
    FrameUpdate {
        new_frame: true,
        redraw: force_redraw || changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publish<T: Clone>(slot: &Mutex<Vec<T>>, generation: &AtomicU64, cells: &[T]) {
        let mut frame = slot.lock().unwrap();
        frame.clear();
        frame.extend_from_slice(cells);
        generation.fetch_add(1, Ordering::Release);
    }

    #[test]
    fn identical_publication_advances_generation_without_swapping_buffers() {
        let slot = Mutex::new(vec![1, 2]);
        let generation = AtomicU64::new(1);
        let consumed = Cell::new(0);
        let mut visible = vec![1, 2];
        let visible_ptr = visible.as_ptr();
        let slot_ptr = slot.lock().unwrap().as_ptr();
        let update = consume_latest(&slot, &generation, &consumed, &mut visible, false);
        assert_eq!(
            update,
            FrameUpdate {
                new_frame: true,
                redraw: false
            }
        );
        assert_eq!(consumed.get(), 1);
        assert_eq!(visible.as_ptr(), visible_ptr);
        assert_eq!(slot.lock().unwrap().as_ptr(), slot_ptr);
    }

    #[test]
    fn changed_frame_swaps_without_copying_and_can_change_back() {
        let slot = Mutex::new(vec![2]);
        let generation = AtomicU64::new(1);
        let consumed = Cell::new(0);
        let mut visible = vec![1];
        let published_ptr = slot.lock().unwrap().as_ptr();
        assert!(consume_latest(&slot, &generation, &consumed, &mut visible, false).redraw);
        assert_eq!(visible, [2]);
        assert_eq!(visible.as_ptr(), published_ptr);
        publish(&slot, &generation, &[1]);
        assert!(consume_latest(&slot, &generation, &consumed, &mut visible, false).redraw);
        assert_eq!(visible, [1]);
    }

    #[test]
    fn latest_identical_frame_cancels_an_unpresented_intermediate_change() {
        let slot = Mutex::new(vec![1]);
        let generation = AtomicU64::new(0);
        let consumed = Cell::new(0);
        let mut visible = vec![1];
        publish(&slot, &generation, &[2]);
        publish(&slot, &generation, &[1]);
        let update = consume_latest(&slot, &generation, &consumed, &mut visible, false);
        assert!(update.new_frame);
        assert!(!update.redraw);
        assert_eq!(consumed.get(), 2);
        assert_eq!(visible, [1]);
    }

    #[test]
    fn matching_boundary_cells_never_hide_an_interior_change() {
        let slot = Mutex::new(vec![1, 9, 3]);
        let generation = AtomicU64::new(1);
        let consumed = Cell::new(0);
        let mut visible = vec![1, 2, 3];
        assert!(consume_latest(&slot, &generation, &consumed, &mut visible, false).redraw);
        assert_eq!(visible, [1, 9, 3]);
    }

    #[test]
    fn geometry_invalidation_redraws_without_a_new_publication() {
        let slot = Mutex::new(vec![99]); // This is the recycled older buffer.
        let generation = AtomicU64::new(3);
        let consumed = Cell::new(3);
        let mut visible = vec![1];
        let update = consume_latest(&slot, &generation, &consumed, &mut visible, true);
        assert_eq!(
            update,
            FrameUpdate {
                new_frame: false,
                redraw: true
            }
        );
        assert_eq!(visible, [1]); // Never replay the recycled slot.
    }

    #[test]
    fn geometry_invalidation_redraws_an_identical_new_publication() {
        let slot = Mutex::new(vec![1]);
        let generation = AtomicU64::new(2);
        let consumed = Cell::new(1);
        let mut visible = vec![1];
        assert_eq!(
            consume_latest(&slot, &generation, &consumed, &mut visible, true),
            FrameUpdate {
                new_frame: true,
                redraw: true
            }
        );
        assert_eq!(consumed.get(), 2);
    }

    #[test]
    fn a_publish_after_an_outside_generation_check_cannot_replay_a_stale_frame() {
        let slot = Mutex::new(vec![1]);
        let generation = AtomicU64::new(1);
        let consumed = Cell::new(0);
        let mut visible = vec![0];
        let outside_observation = generation.load(Ordering::Acquire);
        publish(&slot, &generation, &[2]);
        assert_eq!(outside_observation, 1);
        assert!(consume_latest(&slot, &generation, &consumed, &mut visible, false).redraw);
        assert_eq!(consumed.get(), 2);
        assert_eq!(visible, [2]);
        assert_eq!(
            consume_latest(&slot, &generation, &consumed, &mut visible, false),
            FrameUpdate {
                new_frame: false,
                redraw: false
            }
        );
        assert_eq!(visible, [2]);
    }

    #[test]
    fn empty_publication_clears_a_visible_frame_and_first_empty_frame_can_be_forced() {
        let slot = Mutex::new(Vec::<u8>::new());
        let generation = AtomicU64::new(1);
        let consumed = Cell::new(0);
        let mut visible = vec![1];
        assert!(consume_latest(&slot, &generation, &consumed, &mut visible, false).redraw);
        assert!(visible.is_empty());
        assert!(consume_latest(&slot, &generation, &consumed, &mut visible, true).redraw);
    }
}
