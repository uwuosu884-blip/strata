//! Crash recovery. While the project has unsaved changes a copy is kept in
//! `<config>/recovery` (every 30 s, and when something goes wrong). If the GPU
//! device is lost (driver reset: egui-wgpu would panic on the next frame) or
//! the GUI panics, the app saves what it can and restarts itself on that copy
//! instead of freezing or losing work. A copy left behind (e.g. the computer
//! was restarted) is reopened on the next start.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Set by wgpu's device-lost callback.
pub static DEVICE_LOST: AtomicBool = AtomicBool::new(false);
static RELAUNCHED: AtomicBool = AtomicBool::new(false);
/// A recovery copy of the current state exists.
static HAVE_COPY: AtomicBool = AtomicBool::new(false);
/// The open project's own file (restart target when nothing is unsaved).
static ORIGIN: Mutex<Option<PathBuf>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();

/// Autosave period while there are unsaved changes.
pub const INTERVAL: Duration = Duration::from_secs(30);
/// Restarts within this time of starting count towards the crash-loop limit.
const QUICK: Duration = Duration::from_secs(60);
const MAX_QUICK_RESTARTS: u32 = 3;

pub fn start() {
    START.get_or_init(Instant::now);
}

pub fn dir() -> Option<PathBuf> {
    crate::settings::Settings::path().map(|p| p.with_file_name("recovery"))
}

pub fn file() -> Option<PathBuf> {
    dir().map(|d| d.join("Recovery.dawproj"))
}

fn origin_file() -> Option<PathBuf> {
    dir().map(|d| d.join("origin.txt"))
}

/// Is there a copy left by an earlier session?
pub fn exists() -> bool {
    file().is_some_and(|f| f.exists())
}

/// The project file the copy belongs to (None = never saved).
pub fn origin() -> Option<PathBuf> {
    let s = std::fs::read_to_string(origin_file()?).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

pub fn set_origin(path: Option<&Path>) {
    if let Ok(mut o) = ORIGIN.lock() {
        *o = path.map(Path::to_path_buf);
    }
}

/// Write the recovery copy of `project` (opened from / saved to `path`).
pub fn save(project: &daw_project::Project, path: Option<&Path>, cache: &daw_core::SampleCache) -> bool {
    let (Some(f), Some(o)) = (file(), origin_file()) else { return false };
    match daw_core::save_recovery(project, &f, cache) {
        Ok(()) => {
            let _ = std::fs::write(o, path.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
            HAVE_COPY.store(true, Ordering::Relaxed);
            true
        }
        Err(e) => {
            log::warn!("recovery copy not written: {e}");
            false
        }
    }
}

/// The project was saved, replaced or closed cleanly: the copy is obsolete.
pub fn clear(path: Option<&Path>) {
    set_origin(path);
    if HAVE_COPY.swap(false, Ordering::Relaxed) || exists() {
        if let Some(d) = dir() {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// Start a new instance on the recovery copy (or on the project file when
/// nothing was unsaved). At most once per process, and not in a crash loop
/// (3 restarts each within a minute of starting).
pub fn relaunch(reason: &str) -> bool {
    if RELAUNCHED.swap(true, Ordering::SeqCst) {
        return false;
    }
    let quick = START.get().is_none_or(|s| s.elapsed() < QUICK);
    let n: u32 = std::env::var("DAW_QUICK_RESTARTS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let n = if quick { n + 1 } else { 0 };
    if n >= MAX_QUICK_RESTARTS {
        eprintln!("daw: not restarting after {reason} (crashed {n} times right after starting)");
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let mut c = std::process::Command::new(exe);
    if HAVE_COPY.load(Ordering::Relaxed) {
        c.arg("--recover");
    } else if let Some(p) = ORIGIN.lock().ok().and_then(|o| o.clone()) {
        c.arg(p);
    }
    c.env("DAW_QUICK_RESTARTS", n.to_string()).env("DAW_RESTART_REASON", reason).env_remove("DAW_TEST_CRASH");
    match c.spawn() {
        Ok(_) => {
            eprintln!("daw: restarted after {reason}");
            true
        }
        Err(e) => {
            eprintln!("daw: restart failed: {e}");
            false
        }
    }
}

/// Watch the GUI's wgpu device: a lost device is recorded (the next frame
/// saves and restarts) and other GPU errors are logged instead of panicking.
pub fn watch_device(rs: &eframe::egui_wgpu::RenderState) {
    #[cfg(debug_assertions)]
    let _ = TEST_DEVICE.set(rs.device.clone());
    rs.device.set_device_lost_callback(|reason, msg| {
        log::error!("GPU device lost ({reason:?}): {msg}");
        DEVICE_LOST.store(true, Ordering::SeqCst);
    });
    rs.device.on_uncaptured_error(std::sync::Arc::new(|e| log::error!("wgpu error: {e}")));
}

/// Panic hook part: a panic on the GUI thread restarts the app (after the
/// unwinding `DawApp` wrote the recovery copy), and a watchdog makes sure the
/// process ends even if unwinding hangs on a dead GPU (the window froze).
pub fn on_panic() {
    if std::thread::current().name() != Some("main") {
        return;
    }
    let _ = std::thread::Builder::new().name("crash-watchdog".into()).spawn(|| {
        std::thread::sleep(Duration::from_secs(5));
        relaunch("a crash");
        std::process::exit(101);
    });
}

#[cfg(debug_assertions)]
static TEST_DEVICE: OnceLock<eframe::wgpu::Device> = OnceLock::new();

/// Debug builds: `DAW_TEST_CRASH=gpu|panic` destroys the GPU device / panics
/// 10 s after start (exercises the recovery paths in GUI tests).
#[cfg(debug_assertions)]
pub fn test_crash() {
    static DONE: AtomicBool = AtomicBool::new(false);
    let Ok(kind) = std::env::var("DAW_TEST_CRASH") else { return };
    if START.get().is_none_or(|s| s.elapsed() < Duration::from_secs(10)) || DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    match kind.as_str() {
        "gpu" => {
            if let Some(d) = TEST_DEVICE.get() {
                d.destroy();
            }
        }
        _ => panic!("DAW_TEST_CRASH"),
    }
}
