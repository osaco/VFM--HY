//! Player state machine driven by typed events. All decisions are pure and unit-testable;
//! side effects (save progress, advance queue, seek) are returned as `PlayerAction`s.

use std::path::PathBuf;

use crate::state::{PlaybackState, PlayerSnapshot};

#[derive(Debug, Clone, PartialEq)]
pub enum CoreEvent {
    Playing,
    Paused,
    Stopped,
    EndReached,
    Error(String),
    TimeChanged(i64),
    LengthChanged(i64),
    PositionChanged(f32),
    VolumeChanged(i32),
    Muted,
    Unmuted,
    RateChanged(f32),
    MediaChanged(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerAction {
    None,
    SeekTo(i64),
    SaveProgress {
        path: PathBuf,
        position_ms: i64,
        duration_ms: i64,
    },
    AdvanceQueue,
    ShowError(String),
}

#[derive(Debug)]
pub struct PlayerStateMachine {
    pub snapshot: PlayerSnapshot,
    a_point: Option<i64>,
    b_point: Option<i64>,
}

impl Default for PlayerStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerStateMachine {
    pub fn new() -> Self {
        Self {
            snapshot: PlayerSnapshot::default(),
            a_point: None,
            b_point: None,
        }
    }

    pub fn state(&self) -> &PlaybackState {
        &self.snapshot.state
    }

    pub fn set_ab(&mut self, a: Option<i64>, b: Option<i64>) {
        self.a_point = a;
        self.b_point = b;
    }

    pub fn a_point(&self) -> Option<i64> {
        self.a_point
    }
    pub fn b_point(&self) -> Option<i64> {
        self.b_point
    }

    /// Reset A–B markers when the media changes.
    pub fn on_new_media(&mut self, path: PathBuf) {
        self.snapshot.path = Some(path.to_string_lossy().into_owned());
        self.snapshot.time_ms = 0;
        self.snapshot.length_ms = 0;
        self.snapshot.position = 0.0;
        self.a_point = None;
        self.b_point = None;
    }

    /// Apply one event and return the actions the controller must perform.
    pub fn apply(&mut self, event: CoreEvent) -> Vec<PlayerAction> {
        match event {
            CoreEvent::Playing => {
                self.snapshot.state = PlaybackState::Playing;
                vec![PlayerAction::None]
            }
            CoreEvent::Paused => {
                self.snapshot.state = PlaybackState::Paused;
                vec![self.save_progress_action()]
            }
            CoreEvent::Stopped => {
                self.snapshot.state = PlaybackState::Stopped;
                vec![self.save_progress_action()]
            }
            CoreEvent::EndReached => {
                self.snapshot.state = PlaybackState::Ended;
                let save = self.save_progress_action();
                vec![save, PlayerAction::AdvanceQueue]
            }
            CoreEvent::Error(msg) => {
                self.snapshot.state = PlaybackState::Error(msg.clone());
                vec![PlayerAction::ShowError(msg)]
            }
            CoreEvent::TimeChanged(ms) => {
                self.snapshot.time_ms = ms.max(0);
                match (self.a_point, self.b_point, ms) {
                    (Some(a), Some(b), now) if b > a && now >= b => {
                        vec![PlayerAction::SeekTo(a)]
                    }
                    _ => vec![PlayerAction::None],
                }
            }
            CoreEvent::LengthChanged(ms) => {
                self.snapshot.length_ms = ms.max(0);
                vec![PlayerAction::None]
            }
            CoreEvent::PositionChanged(p) => {
                self.snapshot.position = p.clamp(0.0, 1.0);
                vec![PlayerAction::None]
            }
            CoreEvent::VolumeChanged(v) => {
                self.snapshot.volume = v.clamp(0, 200);
                vec![PlayerAction::None]
            }
            CoreEvent::Muted => {
                self.snapshot.muted = true;
                vec![PlayerAction::None]
            }
            CoreEvent::Unmuted => {
                self.snapshot.muted = false;
                vec![PlayerAction::None]
            }
            CoreEvent::RateChanged(r) => {
                self.snapshot.rate = r;
                vec![PlayerAction::None]
            }
            CoreEvent::MediaChanged(path) => {
                self.on_new_media(path);
                vec![PlayerAction::None]
            }
        }
    }

    fn save_progress_action(&self) -> PlayerAction {
        match &self.snapshot.path {
            Some(p) => PlayerAction::SaveProgress {
                path: PathBuf::from(p),
                position_ms: self.snapshot.time_ms,
                duration_ms: self.snapshot.length_ms,
            },
            None => PlayerAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_pause_stop_transitions() {
        let mut m = PlayerStateMachine::new();
        assert_eq!(m.apply(CoreEvent::Playing), vec![PlayerAction::None]);
        assert_eq!(m.state(), &PlaybackState::Playing);
        m.apply(CoreEvent::Paused);
        assert_eq!(m.state(), &PlaybackState::Paused);
        m.apply(CoreEvent::Stopped);
        assert_eq!(m.state(), &PlaybackState::Stopped);
    }

    #[test]
    fn end_reached_saves_then_advances() {
        let mut m = PlayerStateMachine::new();
        m.on_new_media(PathBuf::from("ep.mkv"));
        m.apply(CoreEvent::TimeChanged(42_000));
        m.apply(CoreEvent::LengthChanged(120_000));
        let actions = m.apply(CoreEvent::EndReached);
        assert_eq!(
            actions,
            vec![
                PlayerAction::SaveProgress {
                    path: PathBuf::from("ep.mkv"),
                    position_ms: 42_000,
                    duration_ms: 120_000,
                },
                PlayerAction::AdvanceQueue,
            ]
        );
    }

    #[test]
    fn ab_repeat_seeks_to_a() {
        let mut m = PlayerStateMachine::new();
        m.set_ab(Some(10_000), Some(20_000));
        assert_eq!(
            m.apply(CoreEvent::TimeChanged(15_000)),
            vec![PlayerAction::None]
        );
        assert_eq!(
            m.apply(CoreEvent::TimeChanged(21_000)),
            vec![PlayerAction::SeekTo(10_000)]
        );
    }

    #[test]
    fn new_media_clears_ab_and_progress() {
        let mut m = PlayerStateMachine::new();
        m.set_ab(Some(1), Some(2));
        m.apply(CoreEvent::TimeChanged(99));
        m.on_new_media(PathBuf::from("next.mkv"));
        assert_eq!(m.a_point(), None);
        assert_eq!(m.b_point(), None);
        assert_eq!(m.snapshot.time_ms, 0);
        assert_eq!(m.snapshot.path.as_deref(), Some("next.mkv"));
    }

    #[test]
    fn error_event_reports_message() {
        let mut m = PlayerStateMachine::new();
        let actions = m.apply(CoreEvent::Error("codec missing".into()));
        assert_eq!(
            actions,
            vec![PlayerAction::ShowError("codec missing".into())]
        );
        assert!(matches!(m.state(), PlaybackState::Error(_)));
    }

    #[test]
    fn volume_and_mute_updates() {
        let mut m = PlayerStateMachine::new();
        m.apply(CoreEvent::VolumeChanged(250));
        assert_eq!(m.snapshot.volume, 200);
        m.apply(CoreEvent::Muted);
        assert!(m.snapshot.muted);
        m.apply(CoreEvent::Unmuted);
        assert!(!m.snapshot.muted);
    }
}
