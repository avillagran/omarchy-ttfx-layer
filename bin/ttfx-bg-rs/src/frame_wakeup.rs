//! Opt-in publication wakeups. eventfd itself coalesces notifications: one read
//! resets the counter atomically, and a concurrent later write stays readable.
//! No separate pending bit can lose a publication during acknowledgement.
use std::cell::RefCell;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

thread_local! {
    static CURRENT: RefCell<Option<Arc<FrameWakeup>>> = const { RefCell::new(None) };
}

pub fn set_current(wakeup: Option<Arc<FrameWakeup>>) {
    CURRENT.with(|current| *current.borrow_mut() = wakeup);
}

/// Call after committing cells and generation, under the publication lock.
pub fn notify_current() {
    CURRENT.with(|current| {
        if let Some(wakeup) = current.borrow().as_ref() {
            let _ = wakeup.notify(); // Fatal errors stop the producer; never spin.
        }
    });
}

pub fn opt_in(value: Option<&str>) -> bool {
    value == Some("1")
}

pub struct FrameWakeup {
    fd: OwnedFd,
    active: AtomicBool,
    failed: AtomicBool,
    stop: Arc<AtomicBool>,
}

impl FrameWakeup {
    pub fn new(stop: Arc<AtomicBool>) -> io::Result<Self> {
        // SAFETY: eventfd has no borrowed pointers and returns a new owned fd.
        let fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            // SAFETY: this successful eventfd is uniquely owned here. Arc keeps
            // it open through callbacks/publication; final drop closes it once.
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            active: AtomicBool::new(true),
            failed: AtomicBool::new(false),
            stop,
        })
    }

    pub fn fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire) && !self.stop.load(Ordering::Acquire)
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    /// UI shutdown calls this while holding the same lock as publication.
    pub fn disable(&self) {
        self.active.store(false, Ordering::Release);
    }

    fn fail(&self, error: io::Error) -> io::Error {
        self.failed.store(true, Ordering::Release);
        self.active.store(false, Ordering::Release);
        self.stop.store(true, Ordering::Release);
        error
    }

    pub fn notify(&self) -> io::Result<()> {
        if !self.is_active() {
            return Ok(());
        }
        let value = 1u64;
        // SAFETY: the owned fd remains live, and value supplies exactly eight
        // initialized bytes. Nonblocking eventfd writes cannot wait for GTK.
        let written = unsafe { libc::write(self.fd(), (&value as *const u64).cast(), 8) };
        if written == 8 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if written == -1 && error.kind() == io::ErrorKind::WouldBlock {
            // Counter saturation already makes the fd readable: wake retained.
            return Ok(());
        }
        Err(self.fail(error))
    }

    /// A single read bounds work per UI dispatch. Writes after this atomic
    /// reset generate another readable event; writes before it are coalesced.
    pub fn drain(&self) -> io::Result<Option<u64>> {
        let mut count = 0u64;
        // SAFETY: count is an eight-byte writable buffer and the fd is owned.
        let read = unsafe { libc::read(self.fd(), (&mut count as *mut u64).cast(), 8) };
        if read == 8 {
            return Ok(Some(count));
        }
        let error = io::Error::last_os_error();
        if read == -1 && error.kind() == io::ErrorKind::WouldBlock {
            return Ok(None);
        }
        Err(self.fail(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    fn wakeup() -> Arc<FrameWakeup> {
        Arc::new(FrameWakeup::new(Arc::new(AtomicBool::new(false))).unwrap())
    }

    #[test]
    fn opt_in_is_explicit_and_descriptor_is_nonblocking_and_close_on_exec() {
        assert!(opt_in(Some("1")));
        for value in [None, Some("0"), Some("true"), Some(" 1")] {
            assert!(!opt_in(value));
        }
        let wake = wakeup();
        assert_ne!(unsafe { libc::fcntl(wake.fd(), libc::F_GETFL) } & libc::O_NONBLOCK, 0);
        assert_ne!(unsafe { libc::fcntl(wake.fd(), libc::F_GETFD) } & libc::FD_CLOEXEC, 0);
        assert_eq!(wake.drain().unwrap(), None);
    }

    #[test]
    fn publications_coalesce_and_a_post_drain_publication_is_not_lost() {
        let wake = wakeup();
        for _ in 0..100 {
            wake.notify().unwrap();
        }
        assert_eq!(wake.drain().unwrap(), Some(100));
        assert_eq!(wake.drain().unwrap(), None);
        let barrier = Arc::new(Barrier::new(2));
        let worker_wake = wake.clone();
        let worker_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            for _ in 0..100 {
                worker_barrier.wait();
                worker_wake.notify().unwrap();
                worker_barrier.wait();
            }
        });
        for _ in 0..100 {
            assert_eq!(wake.drain().unwrap(), None);
            barrier.wait();
            barrier.wait();
            assert_eq!(wake.drain().unwrap(), Some(1));
        }
        worker.join().unwrap();
    }

    #[test]
    fn saturated_counter_preserves_the_wakeup_without_retrying() {
        let wake = wakeup();
        let maximum = u64::MAX - 1;
        assert_eq!(unsafe { libc::write(wake.fd(), (&maximum as *const u64).cast(), 8) }, 8);
        wake.notify().unwrap();
        assert_eq!(wake.drain().unwrap(), Some(maximum));
        assert!(wake.is_active());
        assert!(!wake.failed());
    }

    #[test]
    fn write_racing_the_atomic_read_is_accounted_for_exactly_once() {
        let wake = wakeup();
        let barrier = Arc::new(Barrier::new(2));
        let worker_wake = wake.clone();
        let worker_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            for _ in 0..200 {
                worker_barrier.wait();
                worker_wake.notify().unwrap();
                worker_barrier.wait();
            }
        });
        for _ in 0..200 {
            barrier.wait();
            let before = wake.drain().unwrap().unwrap_or(0);
            barrier.wait();
            let after = wake.drain().unwrap().unwrap_or(0);
            assert_eq!(before + after, 1);
        }
        worker.join().unwrap();
    }

    #[test]
    fn disabled_notifier_does_not_publish_and_last_owner_closes_descriptor() {
        let wake = wakeup();
        let fd = wake.fd();
        let producer = wake.clone();
        let weak = Arc::downgrade(&wake);
        wake.disable();
        producer.notify().unwrap();
        assert_eq!(wake.drain().unwrap(), None);
        drop(wake);
        assert!(weak.upgrade().is_some());
        assert_ne!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
        drop(producer);
        assert!(weak.upgrade().is_none());
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    }
}
