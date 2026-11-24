//! Crash logging.
//!
//! A panic in a UI callback would otherwise vanish into a closed window. The hook writes the
//! message, the thread name and a backtrace to `crash.log` in the data directory (once the
//! directory is known) and keeps the default behaviour of aborting.

use std::path::PathBuf;
use std::sync::OnceLock;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static INSTALLED: OnceLock<()> = OnceLock::new();

/// Point the crash log at `dir`. Safe to call more than once; the first call wins.
pub fn set_dir(dir: PathBuf) {
    let _ = LOG_DIR.set(dir);
}

/// Install the panic hook. Idempotent.
pub fn install() {
    INSTALLED.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "unknown".to_string());
            let payload = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".to_string());
            let thread = std::thread::current();
            let name = thread.name().unwrap_or("unnamed");
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let entry = format!(
                "[{stamp}] panic in thread '{name}' at {location}: {payload}\nbacktrace:\n{}\n\n",
                std::backtrace::Backtrace::force_capture()
            );
            if let Some(dir) = LOG_DIR.get() {
                let _ = std::fs::create_dir_all(dir);
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("crash.log"))
                {
                    let _ = file.write_all(entry.as_bytes());
                }
            }
            eprintln!("{entry}");
            previous(info);
        }));
    });
}

/// Whether diagnostics are enabled: the opt-in flag in settings.
pub fn diagnostics_enabled(flag: bool) -> bool {
    flag || std::env::var("VFM_DIAGNOSTICS").is_ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn diagnostics_flag_is_honoured() {
        assert!(super::diagnostics_enabled(true));
        assert!(!super::diagnostics_enabled(false) || std::env::var("VFM_DIAGNOSTICS").is_ok());
    }
}
