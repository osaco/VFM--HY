//! Deterministic libvlc.dll / plugins/ discovery for the VFM Phase 0 spike.
//!
//! Order is fixed so a dev machine can never silently load a mismatched DLL from PATH:
//!   1. `LIBVLC_PATH` (explicit override)
//!   2. `<exe_dir>/vlc/`   (bundled portable layout)
//!   3. `<exe_dir>/`       (flat portable layout)
//!   4. `C:\Program Files\VideoLAN\VLC` (development fallback only)

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::VlcError;

#[derive(Debug, Clone)]
pub struct VlcPaths {
    /// Directory that contains libvlc.dll, libvlccore.dll and plugins/.
    pub dir: PathBuf,
    pub core_dll: PathBuf,
    pub lib_dll: PathBuf,
    pub plugins: PathBuf,
}

impl VlcPaths {
    /// Resolve the runtime directory and set VLC_PLUGIN_PATH before libvlc_new().
    pub fn discover() -> Result<Self, VlcError> {
        let exe_dir = current_exe_dir();
        let mut candidates: Vec<PathBuf> = Vec::new();

        if let Some(p) = env::var_os("LIBVLC_PATH") {
            if !p.is_empty() {
                candidates.push(PathBuf::from(p));
            }
        }
        candidates.push(exe_dir.join("vlc"));
        candidates.push(exe_dir.clone());
        // Development convenience only; a bundled copy always wins because it is earlier.
        candidates.push(PathBuf::from(r"C:\Program Files\VideoLAN\VLC"));

        let mut last_reason = String::new();
        for dir in candidates {
            let lib = dir.join("libvlc.dll");
            let core = dir.join("libvlccore.dll");
            let plugins = dir.join("plugins");
            if !lib.is_file() {
                last_reason = format!("{} missing", lib.display());
                continue;
            }
            if !core.is_file() {
                last_reason = format!("{} missing", core.display());
                continue;
            }
            if !plugins.is_dir() {
                last_reason = format!("{} missing", plugins.display());
                continue;
            }
            // SAFETY: set_var is safe here; the spike is single-threaded at startup.
            unsafe { env::set_var("VLC_PLUGIN_PATH", &plugins) };
            return Ok(Self {
                dir,
                core_dll: core,
                lib_dll: lib,
                plugins,
            });
        }
        Err(VlcError::LibraryNotFound(last_reason))
    }

    /// The path handed to libvlc_media_new_path as a UTF-8 CString by the caller.
    pub fn display_dir(&self) -> &Path {
        &self.dir
    }
}

fn current_exe_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Used by diagnostics to report the effective plugin search path.
pub fn plugin_path_var() -> Option<OsString> {
    env::var_os("VLC_PLUGIN_PATH")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_exe_dir_is_absolute_or_dot() {
        let d = current_exe_dir();
        assert!(d.is_absolute() || d == Path::new("."));
    }
}
