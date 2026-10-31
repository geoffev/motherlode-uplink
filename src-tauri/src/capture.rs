// Screen capture through OS APIs only: Star Citizen runs Easy Anti-Cheat, so this
// never touches the game process (no injection, hooks, memory reads or process
// handles). Only the StarCitizen.exe window is captured, found by PID from the
// system process list. The Wayland ScreenCast portal is still to add behind `Backend`.

use std::time::{Duration, Instant};

use image::RgbaImage;

pub const GAME_EXE: &str = "StarCitizen.exe";
const RESOLVE_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureError {
    GameNotRunning,
    GameMinimised,
    Failed(String),
}

/// Not `Send`: OS capture handles stay on the capture thread that made them.
pub trait Backend {
    fn grab(&mut self) -> Result<RgbaImage, CaptureError>;
}

#[derive(Default)]
pub struct GameWindow {
    window: Option<xcap::Window>,
    resolved_at: Option<Instant>,
    #[cfg(windows)]
    session: Option<crate::wgc::WindowCapture>,
}

impl GameWindow {
    fn resolve(&mut self) {
        let fresh = self.resolved_at.is_some_and(|t| t.elapsed() < RESOLVE_EVERY);
        if fresh && self.window.is_some() {
            return;
        }
        self.resolved_at = Some(Instant::now());
        self.window = find_game_window();
    }

    /// Windows Graphics Capture of the window, through our own session (xcap's
    /// opens the game process on every frame).
    #[cfg(windows)]
    fn capture(&mut self, w: &xcap::Window) -> Result<RgbaImage, String> {
        // Window handles fit in 32 bits, which is what xcap's id() carries.
        let hwnd = w.id().map_err(|e| e.to_string())? as isize;
        if self.session.as_ref().is_none_or(|s| s.hwnd() != hwnd) {
            self.session = Some(crate::wgc::WindowCapture::new(hwnd)?);
        }
        self.session.as_mut().unwrap().grab()
    }

    #[cfg(not(windows))]
    fn capture(&mut self, w: &xcap::Window) -> Result<RgbaImage, String> {
        w.capture_image().map_err(|e| e.to_string())
    }
}

impl Backend for GameWindow {
    fn grab(&mut self) -> Result<RgbaImage, CaptureError> {
        self.resolve();
        let Some(w) = self.window.clone() else {
            #[cfg(windows)]
            {
                self.session = None;
            }
            return Err(CaptureError::GameNotRunning);
        };
        if w.is_minimized().unwrap_or(false) {
            return Err(CaptureError::GameMinimised);
        }
        self.capture(&w).map_err(|e| {
            self.window = None;
            #[cfg(windows)]
            {
                self.session = None;
            }
            CaptureError::Failed(e)
        })
    }
}

fn find_game_window() -> Option<xcap::Window> {
    let pids = pids_named(GAME_EXE);
    if pids.is_empty() {
        return None;
    }
    xcap::Window::all()
        .ok()?
        .into_iter()
        .filter(|w| w.pid().is_ok_and(|pid| pids.contains(&pid)))
        .max_by_key(|w| w.width().unwrap_or(0) as u64 * w.height().unwrap_or(0) as u64)
}

/// Processes whose executable is `exe`. Read from the system's process list,
/// without opening any process.
#[cfg(windows)]
fn pids_named(exe: &str) -> Vec<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    // SAFETY: plain Win32 calls; the snapshot handle is closed before returning
    // and the entry outlives each call that fills it.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut entry) != 0;
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(exe) {
                out.push(entry.th32ProcessID);
            }
            more = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    out
}

/// Under Wine/Proton the process's comm is the Windows executable name, and
/// its command line holds the .exe path.
#[cfg(target_os = "linux")]
fn pids_named(exe: &str) -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else { return vec![] };
    let named = |pid: u32| {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if comm.trim().eq_ignore_ascii_case(exe) {
            return true;
        }
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        cmdline
            .split(|b| *b == 0)
            .filter_map(|arg| std::str::from_utf8(arg).ok())
            .filter_map(|arg| arg.rsplit(['\\', '/']).next())
            .any(|name| name.eq_ignore_ascii_case(exe))
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| named(pid))
        .collect()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn pids_named(_exe: &str) -> Vec<u32> {
    vec![]
}

pub fn create() -> Box<dyn Backend> {
    Box::<GameWindow>::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(windows, target_os = "linux"))]
    fn finds_a_process_by_its_executable() {
        let me = std::env::current_exe().unwrap();
        let exe = me.file_name().unwrap().to_string_lossy().into_owned();
        // Linux truncates comm to 15 characters, but the command line has the full name.
        assert!(pids_named(&exe).contains(&std::process::id()), "{exe} should be listed");
        assert!(!pids_named(GAME_EXE).contains(&std::process::id()));
    }

    #[test]
    #[ignore] // Needs a desktop session; Star Citizen not running.
    fn no_game_no_capture() {
        assert_eq!(GameWindow::default().grab().err(), Some(CaptureError::GameNotRunning));
    }
}
