//! Producer-thread policy, not a persisted preference or a process-global mode.
//! Controller threads retain read_config's original live/plugin behavior.
use crate::Config;
use std::{cell::RefCell, marker::PhantomData, rc::Rc, sync::{Arc, atomic::{AtomicBool, Ordering}}};

struct Fixed {
    config: Config,
    stop: Arc<AtomicBool>,
}
thread_local! {
    static FIXED: RefCell<Option<Fixed>> = const { RefCell::new(None) };
    #[cfg(test)]
    static PLUGIN_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// A guard must be dropped on its owner thread, including on error/unwind.
pub struct Guard {
    previous: Option<Fixed>,
    _owner: PhantomData<Rc<()>>,
}
pub fn enter(config: Config, stop: Arc<AtomicBool>) -> Guard {
    let previous = FIXED.with(|slot| slot.replace(Some(Fixed { config, stop })));
    Guard { previous, _owner: PhantomData }
}
impl Drop for Guard {
    fn drop(&mut self) {
        FIXED.with(|slot| { slot.replace(self.previous.take()); });
    }
}
pub fn current() -> Option<Config> {
    FIXED.with(|slot| slot.borrow().as_ref().map(|fixed| {
        let mut config = fixed.config.clone();
        config.running = !fixed.stop.load(Ordering::Acquire);
        config
    }))
}

// Counts entry into the actual plugin file-read branch, not calls satisfied by
// Fixed. Compiled out completely in the product binary.
#[cfg(test)]
pub fn record_plugin_read() { PLUGIN_READS.with(|count| count.set(count.get() + 1)); }
#[cfg(test)]
pub fn plugin_reads() -> usize { PLUGIN_READS.with(|count| count.get()) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_policy_is_scoped_thread_local_and_stop_driven() {
        assert!(current().is_none());
        let stop = Arc::new(AtomicBool::new(false));
        let config = Config { intensity: 4, speed: 7, reactivity: 2, running: false, ..Config::default() };
        let outer = enter(config, stop.clone());
        let first = current().unwrap();
        assert_eq!((first.intensity, first.speed, first.reactivity, first.running), (4, 7, 2, true));
        assert!(std::thread::spawn(|| current().is_none()).join().unwrap());
        {
            let _inner = enter(Config { speed: 9, ..Config::default() }, Arc::new(AtomicBool::new(false)));
            assert_eq!(current().unwrap().speed, 9);
        }
        assert_eq!(current().unwrap().speed, 7);
        stop.store(true, Ordering::Release);
        assert!(!current().unwrap().running);
        drop(outer);
        assert!(current().is_none());
    }
}
