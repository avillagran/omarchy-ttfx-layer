//! Pause producers while their background cannot be seen.
//!
//! A background is hidden when the session is locked, its monitor is off, or
//! the monitor's active workspace shows a fullscreen window. Hyprland reports
//! fullscreen and workspace changes on its event socket; locking and DPMS send
//! no event, so the state is also rechecked every few seconds.
use std::cell::RefCell;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const RECHECK: Duration = Duration::from_secs(3);
const PAUSED_POLL: Duration = Duration::from_millis(200);

fn hidden() -> &'static Mutex<HashSet<String>> {
    static HIDDEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    HIDDEN.get_or_init(|| Mutex::new(HashSet::new()))
}

thread_local! {
    static MONITOR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Bind the calling producer thread to a monitor connector (e.g. "eDP-1").
pub fn set_current(connector: Option<String>) {
    MONITOR.with(|m| *m.borrow_mut() = connector);
}

fn current_hidden() -> bool {
    MONITOR.with(|m| {
        m.borrow()
            .as_ref()
            .is_some_and(|name| hidden().lock().map(|h| h.contains(name)).unwrap_or(false))
    })
}

/// Block the producer while its monitor is hidden. Returns at once when
/// visible or when `stop` is set.
pub fn wait_while_hidden(stop: &AtomicBool) {
    let mut logged = false;
    while !stop.load(Ordering::Relaxed) && current_hidden() {
        if !logged {
            logged = true;
            MONITOR.with(|m| eprintln!("paused on {}", m.borrow().as_deref().unwrap_or("?")));
        }
        std::thread::sleep(PAUSED_POLL);
    }
    if logged {
        MONITOR.with(|m| eprintln!("resumed on {}", m.borrow().as_deref().unwrap_or("?")));
    }
}

fn hypr_dir() -> Option<PathBuf> {
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
    let uid = unsafe { libc::getuid() };
    let bases = [
        std::env::var("XDG_RUNTIME_DIR").ok().map(|d| PathBuf::from(d).join("hypr")),
        Some(PathBuf::from(format!("/run/user/{uid}/hypr"))),
    ];
    for base in bases.into_iter().flatten() {
        if let Some(sig) = &signature {
            let dir = base.join(sig);
            if dir.join(".socket2.sock").exists() {
                return Some(dir);
            }
        }
        // No signature in the environment: newest running instance.
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        let newest = entries.flatten()
            .filter(|e| e.path().join(".socket2.sock").exists())
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
        if let Some(entry) = newest {
            return Some(entry.path());
        }
    }
    None
}

fn request(dir: &std::path::Path, command: &str) -> Option<serde_json::Value> {
    let mut stream = UnixStream::connect(dir.join(".socket.sock")).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream.write_all(command.as_bytes()).ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    serde_json::from_str(&reply).ok()
}

fn hidden_monitors(dir: &std::path::Path) -> Option<HashSet<String>> {
    let monitors = request(dir, "j/monitors")?;
    let workspaces = request(dir, "j/workspaces")?;
    let monitors = monitors.as_array()?;
    let fullscreen: HashSet<i64> = workspaces.as_array()?.iter()
        .filter(|w| w["hasfullscreen"].as_bool() == Some(true))
        .filter_map(|w| w["id"].as_i64())
        .collect();
    let locked = monitors.iter().any(|m| {
        m["solitaryBlockedBy"].as_array()
            .is_some_and(|reasons| reasons.iter().any(|r| r.as_str() == Some("LOCK")))
    });
    Some(monitors.iter()
        .filter(|m| {
            locked
                || m["dpmsStatus"].as_bool() == Some(false)
                || m["activeWorkspace"]["id"].as_i64().is_some_and(|id| fullscreen.contains(&id))
        })
        .filter_map(|m| m["name"].as_str().map(str::to_string))
        .collect())
}

fn refresh(dir: &std::path::Path) {
    if let Some(next) = hidden_monitors(dir) {
        if let Ok(mut current) = hidden().lock() {
            *current = next;
        }
    }
}

fn relevant(event: &str) -> bool {
    let name = event.split(">>").next().unwrap_or("");
    matches!(name, "fullscreen" | "workspace" | "workspacev2" | "focusedmon" | "focusedmonv2"
        | "activespecial" | "activespecialv2" | "monitoradded" | "monitoraddedv2"
        | "monitorremoved" | "monitorremovedv2" | "closewindow" | "movewindow" | "movewindowv2")
}

/// Start the watcher once per process. Without Hyprland nothing is ever hidden.
pub fn start() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let Some(dir) = hypr_dir() else {
            eprintln!("occlusion: Hyprland socket not found; never pausing");
            return;
        };
        let spawned = std::thread::Builder::new().name("ttfx-occlusion".into()).spawn(move || loop {
            refresh(&dir);
            let Ok(stream) = UnixStream::connect(dir.join(".socket2.sock")) else {
                std::thread::sleep(RECHECK);
                continue;
            };
            let _ = stream.set_read_timeout(Some(RECHECK));
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if relevant(line.trim_end()) {
                            refresh(&dir);
                        }
                    }
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                        refresh(&dir);
                    }
                    Err(_) => break,
                }
            }
        });
        if let Err(error) = spawned {
            eprintln!("occlusion: watcher unavailable: {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_visibility_events_trigger_a_refresh() {
        assert!(relevant("fullscreen>>1"));
        assert!(relevant("workspacev2>>2,2"));
        assert!(!relevant("activewindow>>foot,title"));
        assert!(!relevant("openlayer>>ttfx-bg-direct"));
    }

    #[test]
    fn unbound_threads_never_wait() {
        set_current(None);
        let stop = AtomicBool::new(false);
        wait_while_hidden(&stop);
    }

    #[test]
    fn hidden_monitor_blocks_until_stop() {
        hidden().lock().unwrap().insert("TEST-1".into());
        set_current(Some("TEST-1".into()));
        let stop = AtomicBool::new(true);
        wait_while_hidden(&stop);
        set_current(None);
        hidden().lock().unwrap().remove("TEST-1");
    }
}
