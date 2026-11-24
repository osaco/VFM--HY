//! System Media Transport Controls (SMTC) bridge.
//!
//! Uses the official Windows crate safe bindings. The WinRT callback only forwards a
//! device-independent `MediaKey` over an `mpsc` channel; all player actions happen on the
//! Slint UI thread via an event-loop timer. No libVLC calls occur from this module.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use windows::Foundation::TypedEventHandler;
use windows::Media::Playback::MediaPlayer;
use windows::Media::{
    MediaPlaybackStatus, SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKey {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
}

/// Keeps the WinRT `MediaPlayer` alive and owns the SMTC event registration.
pub struct Smtc {
    _player: MediaPlayer,
    controls: SystemMediaTransportControls,
    token: i64,
}

impl Smtc {
    /// Attach SMTC and forward button presses to `sender`.
    ///
    /// SMTC may be unavailable in some Windows sessions; callers should log and continue
    /// rather than failing playback startup.
    pub fn attach(sender: Sender<MediaKey>) -> windows::core::Result<Self> {
        let player = MediaPlayer::new()?;
        let controls = player.SystemMediaTransportControls()?;
        controls.SetIsEnabled(true)?;
        controls.SetIsPlayEnabled(true)?;
        controls.SetIsPauseEnabled(true)?;
        controls.SetIsStopEnabled(true)?;
        controls.SetIsNextEnabled(true)?;
        controls.SetIsPreviousEnabled(true)?;
        controls.SetIsFastForwardEnabled(false)?;
        controls.SetIsRewindEnabled(false)?;
        controls.SetPlaybackStatus(MediaPlaybackStatus::Stopped)?;

        // `mpsc::Sender` is Send but not Sync; the WinRT handler must be both.
        let sender = Arc::new(Mutex::new(sender));
        let handler = TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(move |_source, args| {
            if let Some(button) = args.as_ref().and_then(|args| args.Button().ok()) {
                let key = match button {
                    SystemMediaTransportControlsButton::Play => MediaKey::Play,
                    SystemMediaTransportControlsButton::Pause => MediaKey::Pause,
                    SystemMediaTransportControlsButton::Stop => MediaKey::Stop,
                    SystemMediaTransportControlsButton::Next => MediaKey::Next,
                    SystemMediaTransportControlsButton::Previous => MediaKey::Previous,
                    _ => return Ok(()),
                };
                if let Ok(tx) = sender.lock() {
                    let _ = tx.send(key);
                }
            }
            Ok(())
        });
        let token = controls.ButtonPressed(&handler)?;

        Ok(Self {
            _player: player,
            controls,
            token,
        })
    }

    /// Reflect local playback state in the system media control.
    pub fn set_playing(&self, playing: bool) {
        let status = if playing {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        };
        let _ = self.controls.SetPlaybackStatus(status);
    }
}

impl Drop for Smtc {
    fn drop(&mut self) {
        let _ = self.controls.RemoveButtonPressed(self.token);
        let _ = self.controls.SetIsEnabled(false);
    }
}
