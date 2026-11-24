//! Watch progress and resume policy.

use std::path::PathBuf;
use std::time::SystemTime;

/// Persisted position for one media file.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchProgress {
    pub path: PathBuf,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub watched: bool,
    pub last_played: SystemTime,
}

impl WatchProgress {
    pub fn new(path: impl Into<PathBuf>, position_ms: i64, duration_ms: i64) -> Self {
        Self {
            path: path.into(),
            position_ms: position_ms.max(0),
            duration_ms: duration_ms.max(0),
            watched: false,
            last_played: SystemTime::now(),
        }
    }

    /// Human-readable resume label used by the "continue watching" prompt.
    pub fn resume_label(&self) -> String {
        let secs = (self.position_ms / 1000).max(0);
        format!(" Resume at {}:{:02} ", secs / 60, secs % 60)
    }

    /// Mark watched within the last `min(5%, 30 s)` of the duration.
    ///
    /// Using a plain 30-second tail would mark every clip shorter than 30 seconds as
    /// watched immediately, so the 30-second allowance is capped by 5% of the media length.
    pub fn update_watched(&mut self) {
        if self.duration_ms <= 0 {
            return;
        }
        let tail = (self.duration_ms / 20).min(30_000);
        if self.duration_ms - self.position_ms <= tail {
            self.watched = true;
            self.position_ms = 0;
        }
    }
}

/// Decide whether to offer resume. Short intros are not worth prompting for.
pub fn should_resume(progress: &WatchProgress, min_remaining_ms: i64) -> bool {
    if progress.watched || progress.position_ms <= 0 || progress.duration_ms <= 0 {
        return false;
    }
    progress.duration_ms - progress.position_ms >= min_remaining_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_label_formats_mm_ss() {
        let p = WatchProgress::new("a.mkv", 125_000, 600_000);
        assert_eq!(p.resume_label(), " Resume at 2:05 ");
    }

    #[test]
    fn resume_policy() {
        let mut p = WatchProgress::new("a.mkv", 40_000, 60_000);
        assert!(!should_resume(&p, 30_000));
        p.position_ms = 20_000;
        assert!(should_resume(&p, 30_000));
        p.watched = true;
        assert!(!should_resume(&p, 30_000));
    }

    #[test]
    fn near_end_marks_watched_and_clears_position() {
        let mut p = WatchProgress::new("a.mkv", 590_000, 600_000);
        p.update_watched();
        assert!(p.watched);
        assert_eq!(p.position_ms, 0);
    }

    #[test]
    fn short_clip_is_not_watched_early() {
        let mut p = WatchProgress::new("short.mp4", 2_000, 12_000);
        p.update_watched();
        assert!(!p.watched);
        assert_eq!(p.position_ms, 2_000);

        p.position_ms = 11_600;
        p.update_watched();
        assert!(p.watched);
        assert_eq!(p.position_ms, 0);
    }
}
