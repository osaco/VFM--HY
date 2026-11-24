//! Windows integration helpers that need no `unsafe`: CLI/open-with, file associations
//! via `reg.exe`, and a single-instance lock file in the user's local app-data folder.
//! SMTC / media keys and native drag & drop require Win32/WinRT interop and are tracked
//! in `docs/testing.md` as remaining Phase 1 work.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;

use vfm_core::WindowState;

const APP_ID: &str = "VFM.VedioForMe";
const PROG_ID: &str = "VFM.Media";

/// Build a file-association helper. Uses `reg.exe` so no unsafe Win32 calls are needed.
pub struct Associations {
    exe: PathBuf,
}

impl Associations {
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        Self { exe: exe.into() }
    }

    fn reg(&self, args: &[&str]) -> Result<(), String> {
        let out = Command::new("reg.exe")
            .args(args)
            .output()
            .map_err(|e| format!("could not run reg.exe: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
        }
    }

    /// Register HKCU file associations for common media extensions.
    pub fn register(&self, extensions: &[&str]) -> Result<(), String> {
        let exe = format!("\"{}\" \"%1\"", self.exe.display());
        self.reg(&[
            "add",
            &format!(r"HKCU\Software\Classes\{PROG_ID}\shell\open\command"),
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            &exe,
            "/f",
        ])?;
        self.reg(&[
            "add",
            &format!(r"HKCU\Software\Classes\{PROG_ID}"),
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            "Vedio for me media file",
            "/f",
        ])?;
        for ext in extensions {
            self.reg(&[
                "add",
                &format!(r"HKCU\Software\Classes\.{ext}"),
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                PROG_ID,
                "/f",
            ])?;
        }
        self.reg(&[
            "add",
            &format!(r"HKCU\Software\{APP_ID}"),
            "/v",
            "AppId",
            "/t",
            "REG_SZ",
            "/d",
            APP_ID,
            "/f",
        ])
    }

    pub fn unregister(&self, extensions: &[&str]) -> Result<(), String> {
        for ext in extensions {
            let _ = self.reg(&["delete", &format!(r"HKCU\Software\Classes\.{ext}"), "/f"]);
        }
        let _ = self.reg(&[
            "delete",
            &format!(r"HKCU\Software\Classes\{PROG_ID}"),
            "/f",
            "/s",
        ]);
        let _ = self.reg(&["delete", &format!(r"HKCU\Software\{APP_ID}"), "/f", "/s"]);
        Ok(())
    }
}

/// Single-instance guard backed by a lock file. Stale locks from crashed processes are
/// recovered by checking whether the recorded PID is still alive via `tasklist`.
pub struct SingleInstance {
    path: PathBuf,
}

impl SingleInstance {
    pub fn acquire() -> Result<Self, String> {
        let dir = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(APP_ID);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("instance.lock");
        if path.exists() {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if pid_alive(text.trim()) {
                return Err("another VFM instance is already running".into());
            }
            let _ = std::fs::remove_file(&path);
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        let _ = writeln!(file, "{}", std::process::id());
        Ok(Self { path })
    }

    pub fn lock_path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn pid_alive(pid: &str) -> bool {
    match pid.trim().parse::<u32>() {
        Ok(value) => vfm_platform::process_alive(value),
        Err(_) => false,
    }
}

/// Restore window geometry with basic sanity clamping for multi-monitor changes.
pub fn sanitize_window(state: Option<&WindowState>) -> WindowState {
    match state {
        Some(s)
            if s.width >= 640
                && s.height >= 360
                && s.width <= 8192
                && s.height <= 8192
                && (-8192..=16384).contains(&s.x)
                && (-8192..=16384).contains(&s.y) =>
        {
            s.clone()
        }
        _ => WindowState {
            x: 80,
            y: 80,
            width: 1280,
            height: 720,
            maximized: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_bad_geometry() {
        let bad = WindowState {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            maximized: false,
        };
        let out = sanitize_window(Some(&bad));
        assert_eq!(out.width, 1280);
        assert_eq!(out.height, 720);
    }

    #[test]
    fn sanitize_rejects_offscreen_geometry() {
        let offscreen = WindowState {
            x: 99_999,
            y: 20,
            width: 1600,
            height: 900,
            maximized: false,
        };
        let out = sanitize_window(Some(&offscreen));
        assert_eq!(out.x, 80);
        assert_eq!(out.width, 1280);
    }

    #[test]
    fn sanitize_accepts_good_geometry() {
        let good = WindowState {
            x: 10,
            y: 20,
            width: 1600,
            height: 900,
            maximized: true,
        };
        assert_eq!(sanitize_window(Some(&good)), good);
    }

    #[test]
    fn pid_alive_rejects_garbage() {
        assert!(!pid_alive(""));
        assert!(!pid_alive("not-a-pid"));
    }
}
