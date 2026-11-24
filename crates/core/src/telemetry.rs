//! Frame telemetry math. Counters live in the wrapper (atomics); this module turns
//! raw counts + copy-duration samples into the numbers shown by the HUD and bench harness.

use std::collections::VecDeque;
use std::time::Instant;

/// A point-in-time view of pipeline health.
#[derive(Debug, Clone, PartialEq)]
pub struct TelemetrySnapshot {
    pub decoded_frames: u64,
    pub presented_frames: u64,
    pub dropped_frames: u64,
    pub decoded_fps: f32,
    pub presented_fps: f32,
    /// 0.0 ..= 100.0
    pub drop_rate_pct: f32,
    /// Average front-buffer copy in milliseconds.
    pub avg_copy_ms: f32,
    pub p95_copy_ms: f32,
    pub elapsed_secs: f32,
}

impl Default for TelemetrySnapshot {
    fn default() -> Self {
        Self {
            decoded_frames: 0,
            presented_frames: 0,
            dropped_frames: 0,
            decoded_fps: 0.0,
            presented_fps: 0.0,
            drop_rate_pct: 0.0,
            avg_copy_ms: 0.0,
            p95_copy_ms: 0.0,
            elapsed_secs: 0.0,
        }
    }
}

/// Accumulates frame counts and copy timings. Cheap enough to call from a timer thread.
#[derive(Debug)]
pub struct TelemetryAccumulator {
    started: Instant,
    decoded: u64,
    presented: u64,
    dropped: u64,
    copy_samples: VecDeque<f32>,
    window_decoded: u64,
    window_presented: u64,
    window_dropped: u64,
    window_started: Instant,
}

impl Default for TelemetryAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryAccumulator {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            decoded: 0,
            presented: 0,
            dropped: 0,
            copy_samples: VecDeque::with_capacity(512),
            window_decoded: 0,
            window_presented: 0,
            window_dropped: 0,
            window_started: now,
        }
    }

    pub fn record_decoded(&mut self) {
        self.decoded += 1;
        self.window_decoded += 1;
    }

    pub fn record_dropped(&mut self) {
        self.dropped += 1;
        self.window_dropped += 1;
    }

    /// A frame reached the Slint image property. `copy_ms` is the front-buffer upload time.
    pub fn record_presented(&mut self, copy_ms: f32) {
        self.presented += 1;
        self.window_presented += 1;
        self.copy_samples.push_back(copy_ms);
        if self.copy_samples.len() > 512 {
            self.copy_samples.pop_front();
        }
    }

    /// Snapshot over the whole run.
    pub fn snapshot(&self) -> TelemetrySnapshot {
        let secs = self.started.elapsed().as_secs_f32().max(1e-3);
        let decoded = self.decoded;
        let presented = self.presented;
        let dropped = self.dropped;
        let (avg, p95) = copy_stats(&self.copy_samples);
        TelemetrySnapshot {
            decoded_frames: decoded,
            presented_frames: presented,
            dropped_frames: dropped,
            decoded_fps: decoded as f32 / secs,
            presented_fps: presented as f32 / secs,
            drop_rate_pct: percent(dropped, decoded + dropped),
            avg_copy_ms: avg,
            p95_copy_ms: p95,
            elapsed_secs: secs,
        }
    }

    /// Snapshot of the last window (call once per second from the bench harness),
    /// then reset window counters. Copy timings remain cumulative for stable p95.
    pub fn take_window(&mut self) -> TelemetrySnapshot {
        let secs = self.window_started.elapsed().as_secs_f32().max(1e-3);
        let d = self.window_decoded;
        let p = self.window_presented;
        let x = self.window_dropped;
        self.window_decoded = 0;
        self.window_presented = 0;
        self.window_dropped = 0;
        self.window_started = Instant::now();
        let (avg, p95) = copy_stats(&self.copy_samples);
        TelemetrySnapshot {
            decoded_frames: d,
            presented_frames: p,
            dropped_frames: x,
            decoded_fps: d as f32 / secs,
            presented_fps: p as f32 / secs,
            drop_rate_pct: percent(x, d + x),
            avg_copy_ms: avg,
            p95_copy_ms: p95,
            elapsed_secs: secs,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

fn percent(part: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (part as f32 * 100.0) / total as f32
    }
}

fn copy_stats(samples: &VecDeque<f32>) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let mut sorted: Vec<f32> = samples.iter().copied().collect();
    let sum: f32 = sorted.iter().sum();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let idx = ((sorted.len() as f32 * 0.95) as usize).saturating_sub(1);
    (sum / sorted.len() as f32, sorted[idx])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_snapshot_is_zeroed() {
        let s = TelemetrySnapshot::default();
        assert_eq!(s.decoded_fps, 0.0);
        assert_eq!(s.drop_rate_pct, 0.0);
    }

    #[test]
    fn drop_rate_math() {
        let mut t = TelemetryAccumulator::new();
        for _ in 0..98 {
            t.record_decoded();
            t.record_presented(1.0);
        }
        for _ in 0..2 {
            t.record_dropped();
        }
        let s = t.take_window();
        assert_eq!(s.decoded_frames, 98);
        assert_eq!(s.presented_frames, 98);
        assert_eq!(s.dropped_frames, 2);
        assert!((s.drop_rate_pct - 2.0).abs() < 1e-4);
        assert!((s.avg_copy_ms - 1.0).abs() < 1e-4);
    }

    #[test]
    fn window_resets_but_cumulative_totals_do_not() {
        let mut t = TelemetryAccumulator::new();
        t.record_decoded();
        let first = t.take_window();
        assert_eq!(first.decoded_frames, 1);
        let second = t.take_window();
        assert_eq!(second.decoded_frames, 0);
        assert_eq!(t.snapshot().decoded_frames, 1);
    }

    #[test]
    fn p95_ignores_nothing_and_is_monotonic() {
        let mut t = TelemetryAccumulator::new();
        for i in 1..=100 {
            t.record_presented(i as f32);
        }
        let s = t.snapshot();
        assert!(s.p95_copy_ms >= 90.0);
        assert!(s.p95_copy_ms <= 100.0);
        assert!(s.avg_copy_ms <= s.p95_copy_ms);
    }
}
