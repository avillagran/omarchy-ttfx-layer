//! Publicación nativa con alternativa A/B exacta; el llamador posee el mutex.
pub fn publish<T: Clone>(current: &mut Vec<T>, cells: &mut Vec<T>, swap: bool) {
    if swap { std::mem::swap(current, cells); } else { current.clone_from(cells); }
}

#[cfg(test)]
use crate::asm_direct_bridge::DirectCell;
#[cfg(test)]
thread_local! {
    static CAPTURE: std::cell::RefCell<Option<(usize, Vec<Vec<DirectCell>>)>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub fn capture_begin(limit: usize) {
    CAPTURE.with(|c| *c.borrow_mut()=Some((limit,Vec::new())));
}
#[cfg(test)]
pub fn capture_frame(cells: &[DirectCell], stop: Option<&std::sync::atomic::AtomicBool>) {
    CAPTURE.with(|c| {
        if let Some((limit,frames))=c.borrow_mut().as_mut() {
            frames.push(cells.to_vec());
            if frames.len()>=*limit {
                if let Some(stop)=stop {stop.store(true,std::sync::atomic::Ordering::Release);}
            }
        }
    });
}
#[cfg(test)]
pub fn capture_finish() -> Vec<Vec<DirectCell>> {
    CAPTURE.with(|c|c.borrow_mut().take().unwrap().1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
    #[derive(Debug)]
    struct Counted(u32,Arc<AtomicUsize>);
    impl Clone for Counted {
        fn clone(&self)->Self {self.1.fetch_add(1,Ordering::Relaxed);Self(self.0,self.1.clone())}
    }
    #[test]
    fn swap_transfers_allocation_without_cloning_elements() {
        let counter=Arc::new(AtomicUsize::new(0));
        let mut current=vec![Counted(9,counter.clone())];
        let mut cells=vec![Counted(1,counter.clone()),Counted(2,counter.clone())];
        let ptr=cells.as_ptr();
        publish(&mut current,&mut cells,true);
        assert_eq!(counter.load(Ordering::Relaxed),0);
        assert_eq!(current.as_ptr(),ptr);
        assert_eq!(current.iter().map(|c|c.0).collect::<Vec<_>>(),vec![1,2]);
        assert_eq!(cells[0].0,9);
        publish(&mut current,&mut cells,false);
        assert!(counter.load(Ordering::Relaxed)>0);
    }
    #[test]
    fn recycled_slot_and_consumer_swaps_preserve_visible_sequence() {
        use crate::frame_handoff::consume_latest;
        use std::{cell::Cell,sync::{Mutex,atomic::AtomicU64}};
        fn run(swap:bool)->Vec<(u64,Vec<u32>,bool)> {
            let slot=Mutex::new(vec![999]);
            let generation=AtomicU64::new(0);
            let consumed=Cell::new(0);
            let mut visible=vec![];
            let mut cells=vec![];
            let mut result=vec![];
            let frames=[vec![1,2],vec![1,2],vec![],vec![3],vec![1,2],vec![1,9,2],vec![]];
            for (i,frame) in frames.iter().enumerate() {
                cells.clear(); cells.extend_from_slice(frame);
                {let mut current=slot.lock().unwrap();publish(&mut current,&mut cells,swap);generation.fetch_add(1,Ordering::Release);}
                // También cubrir una publicación intermedia sustituida antes de consumir.
                if i==3 {continue;}
                let update=consume_latest(&slot,&generation,&consumed,&mut visible,false);
                result.push((consumed.get(),visible.clone(),update.redraw));
            }
            result
        }
        assert_eq!(run(false),run(true));
    }
}
