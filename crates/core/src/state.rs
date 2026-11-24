//! Playback state machine and an immutable snapshot handed to the UI.

/// Coarse playback state reported to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
    Ended,
    Error(String),
}

/// Immutable view of the player, refreshed on libVLC events and UI ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSnapshot {
    pub state: PlaybackState,
    pub path: Option<String>,
    pub time_ms: i64,
    pub length_ms: i64,
    /// 0.0 ..= 1.0, clamped.
    pub position: f32,
    pub volume: i32,
    pub muted: bool,
    pub rate: f32,
}

impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self {
            state: PlaybackState::default(),
            path: None,
            time_ms: 0,
            length_ms: 0,
            position: 0.0,
            volume: 100,
            muted: false,
            rate: 1.0,
        }
    }
}

impl PlayerSnapshot {
    /// Human readable status line used by the Phase 0 HUD.
    pub fn status_line(&self) -> String {
        let label = match &self.state {
            PlaybackState::Stopped => "Stopped",
            PlaybackState::Playing => "Playing",
            PlaybackState::Paused => "Paused",
            PlaybackState::Ended => "Ended",
            PlaybackState::Error(e) => return format!("Error: {e}"),
        };
        format!(
            "{label}  {}/{}  vol {}%{}",
            crate::time::format_timecode(self.time_ms),
            crate::time::format_timecode(self.length_ms),
            self.volume,
            if self.muted { " (muted)" } else { "" }
        )
    }

    /// Clamp a raw libVLC position into the 0..=1 range libVLC sometimes overshoots.
    pub fn clamped_position(raw: f32) -> f32 {
        raw.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_snapshot_is_stopped() {
        let s = PlayerSnapshot::default();
        assert_eq!(s.state, PlaybackState::Stopped);
        assert_eq!(s.volume, 100);
        assert!((s.rate - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn status_line_reports_error() {
        let s = PlayerSnapshot {
            state: PlaybackState::Error("missing codec".into()),
            ..Default::default()
        };
        assert_eq!(s.status_line(), "Error: missing codec");
    }

    #[test]
    fn position_is_clamped() {
        assert_eq!(PlayerSnapshot::clamped_position(-0.2), 0.0);
        assert_eq!(PlayerSnapshot::clamped_position(1.7), 1.0);
        assert_eq!(PlayerSnapshot::clamped_position(0.42), 0.42);
    }
}
