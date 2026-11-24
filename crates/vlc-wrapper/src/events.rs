//! Typed libVLC events delivered through a bounded async channel.
//!
//! Thread-safety contract:
//! * The C callback runs on libVLC's event thread. It reads only the leading `i32 type`
//!   field of `libvlc_event_t` and calls `mpsc::Sender::try_send` — non-blocking, no locks,
//!   no libVLC calls, no allocation. Overflow is reported through `EventOverflow`.
//! * The receiver lives on the UI/controller thread; state queries happen there, never in
//!   the callback. This avoids both the vout-mutex deadlock and union layout assumptions.
//! * `EventHub::drop` detaches every callback before releasing the user-data box.

use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, TrySendError};
use std::sync::Arc;

use crate::sys::VlcApi;
use crate::VlcError;

// Values from libvlc_events.h (libvlc_event_e).
const MEDIA_CHANGED: i32 = 0x100;
const OPENING: i32 = 0x102;
const BUFFERING: i32 = 0x103;
const PLAYING: i32 = 0x104;
const PAUSED: i32 = 0x105;
const STOPPED: i32 = 0x106;
const END_REACHED: i32 = 0x109;
const ENCOUNTERED_ERROR: i32 = 0x10A;
const TIME_CHANGED: i32 = 0x10B;
const POSITION_CHANGED: i32 = 0x10C;
const SEEKABLE_CHANGED: i32 = 0x10D;
const TITLE_CHANGED: i32 = 0x10F;
const LENGTH_CHANGED: i32 = 0x111;
const ES_ADDED: i32 = 0x114;
const MUTED: i32 = 0x119;
const UNMUTED: i32 = 0x11A;
const AUDIO_VOLUME: i32 = 0x11B;
const CHAPTER_CHANGED: i32 = 0x11D;

const WATCHED: &[i32] = &[
    MEDIA_CHANGED,
    OPENING,
    BUFFERING,
    PLAYING,
    PAUSED,
    STOPPED,
    END_REACHED,
    ENCOUNTERED_ERROR,
    TIME_CHANGED,
    POSITION_CHANGED,
    SEEKABLE_CHANGED,
    TITLE_CHANGED,
    LENGTH_CHANGED,
    ES_ADDED,
    MUTED,
    UNMUTED,
    AUDIO_VOLUME,
    CHAPTER_CHANGED,
];

/// A device-independent event. Payloads are deliberately omitted: the receiver queries
/// the player for authoritative state, which keeps the callback allocation-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerEvent {
    MediaChanged,
    Opening,
    Buffering,
    Playing,
    Paused,
    Stopped,
    EndReached,
    EncounteredError,
    TimeChanged,
    PositionChanged,
    SeekableChanged,
    TitleChanged,
    LengthChanged,
    TrackListChanged,
    Muted,
    Unmuted,
    VolumeChanged,
    ChapterChanged,
    /// Sentinel used only if the channel was full; the next state query resynchronises.
    Overflow,
}

impl PlayerEvent {
    fn from_type(t: i32) -> Option<Self> {
        Some(match t {
            MEDIA_CHANGED => Self::MediaChanged,
            OPENING => Self::Opening,
            BUFFERING => Self::Buffering,
            PLAYING => Self::Playing,
            PAUSED => Self::Paused,
            STOPPED => Self::Stopped,
            END_REACHED => Self::EndReached,
            ENCOUNTERED_ERROR => Self::EncounteredError,
            TIME_CHANGED => Self::TimeChanged,
            POSITION_CHANGED => Self::PositionChanged,
            SEEKABLE_CHANGED => Self::SeekableChanged,
            TITLE_CHANGED => Self::TitleChanged,
            LENGTH_CHANGED => Self::LengthChanged,
            ES_ADDED => Self::TrackListChanged,
            MUTED => Self::Muted,
            UNMUTED => Self::Unmuted,
            AUDIO_VOLUME => Self::VolumeChanged,
            CHAPTER_CHANGED => Self::ChapterChanged,
            _ => return None,
        })
    }
}

/// Owns the event-manager registrations and the stable user-data pointer.
pub struct EventHub {
    api: Arc<VlcApi>,
    manager: *mut c_void,
    user: Box<mpsc::SyncSender<PlayerEvent>>,
    attached: Vec<i32>,
}

impl std::fmt::Debug for EventHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventHub")
            .field("attached", &self.attached.len())
            .finish()
    }
}

impl EventHub {
    /// Attach to a live media player. Returns the receiver for the controller thread.
    ///
    /// # Safety
    ///
    /// `player` must be a live, non-null `libvlc_media_player_t` that outlives the returned
    /// `EventHub`. The caller must drop the hub before releasing the player.
    pub(crate) unsafe fn attach(
        api: Arc<VlcApi>,
        player: *mut c_void,
    ) -> Result<(Self, Receiver<PlayerEvent>), VlcError> {
        // SAFETY: caller guarantees `player` is a live libvlc_media_player_t owned by
        // MediaPlayer and outlives the hub.
        let manager = unsafe { (api.event_manager)(player) };
        if manager.is_null() {
            return Err(VlcError::InstanceFailed("event manager is null".into()));
        }
        let (tx, rx) = mpsc::sync_channel(256);
        let mut hub = Self {
            api,
            manager,
            user: Box::new(tx),
            attached: Vec::new(),
        };
        // SAFETY: core event-manager APIs are thread-safe; `user` is a Box-owned Sender whose
        // allocation never moves, and `event_cb` only calls try_send with no libVLC calls.
        unsafe {
            let opaque = (&mut *hub.user) as *mut mpsc::SyncSender<PlayerEvent> as *mut c_void;
            for &ty in WATCHED {
                if (hub.api.event_attach)(hub.manager, ty, event_cb, opaque) == 0 {
                    hub.attached.push(ty);
                }
            }
        }
        if hub.attached.is_empty() {
            return Err(VlcError::InstanceFailed(
                "could not attach any libVLC events".into(),
            ));
        }
        Ok((hub, rx))
    }
}

impl Drop for EventHub {
    fn drop(&mut self) {
        // SAFETY: detach all callbacks while both the manager and user-data box are alive.
        unsafe {
            let opaque = (&mut *self.user) as *mut mpsc::SyncSender<PlayerEvent> as *mut c_void;
            for &ty in &self.attached {
                (self.api.event_detach)(self.manager, ty, event_cb, opaque);
            }
            // libVLC may still have one callback in flight; `event_detach` waits for it.
        }
    }
}

unsafe extern "C" fn event_cb(event: *const c_void, opaque: *mut c_void) {
    // SAFETY: `event` points at a libvlc_event_t living for the duration of this callback.
    // The C ABI guarantees the first member is `int type`; we read nothing else.
    let raw_type = *(event as *const i32);
    // SAFETY: `opaque` is the stable Box<mpsc::Sender<_>> pointer set in EventHub::attach,
    // alive until every callback has been detached in EventHub::drop.
    let tx = &*(opaque as *const mpsc::SyncSender<PlayerEvent>);
    let event = PlayerEvent::from_type(raw_type).unwrap_or(PlayerEvent::Overflow);
    match tx.try_send(event) {
        Ok(()) | Err(TrySendError::Full(PlayerEvent::Overflow)) => {}
        Err(_) => {} // receiver gone or channel full: next state query resynchronises
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_all_watched_event_codes() {
        for &code in WATCHED {
            assert_ne!(
                PlayerEvent::from_type(code),
                None,
                "unmapped event 0x{code:x}"
            );
        }
    }

    #[test]
    fn ignores_unknown_events() {
        assert_eq!(PlayerEvent::from_type(0xDEAD), None);
    }
}
