//! Hand-audited subset of the libVLC 3.0.x C ABI, resolved at runtime with `libloading`.
//!
//! Invariants (checked against the bundled SDK headers):
//! - `libvlc_time_t` is `i64` milliseconds; volume is `int` 0..=200.
//! - `libvlc_video_lock_cb(opaque, void **planes)` returns an opaque picture pointer and
//!   writes the plane-0 data pointer into `planes[0]`.
//! - Event callbacks receive `const libvlc_event_t*`; we read ONLY the leading `int type`
//!   field and never dereference the union, so no union layout assumption is needed.
//! - Every call is `extern "C"`. Callbacks never re-enter libVLC.

use libloading::{Library, Symbol};
use std::ffi::{c_char, c_int, c_void};

use crate::loader::VlcPaths;
use crate::video::{VIDEO_DISPLAY_CB, VIDEO_LOCK_CB, VIDEO_UNLOCK_CB};
use crate::VlcError;

/// Signature of `libvlc_callback_t`.
pub type EventCb = unsafe extern "C" fn(*const c_void, *mut c_void);

/// `libvlc_chapter_description_t` — chapter offsets and durations are milliseconds.
#[repr(C)]
pub struct ChapterDescription {
    pub time_offset: i64,
    pub duration: i64,
    pub name: *mut c_char,
}

/// `libvlc_audio_track_t`.
#[repr(C)]
pub struct AudioTrack {
    pub channels: u32,
    pub rate: u32,
}

/// `libvlc_video_viewpoint_t` — three floats, no padding on any supported ABI.
#[repr(C)]
pub struct VideoViewpoint {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

/// `libvlc_video_track_t`.
#[repr(C)]
pub struct VideoTrack {
    pub height: u32,
    pub width: u32,
    pub sar_num: u32,
    pub sar_den: u32,
    pub frame_rate_num: u32,
    pub frame_rate_den: u32,
    pub orientation: c_int,
    pub projection: c_int,
    pub pose: VideoViewpoint,
}

/// `libvlc_subtitle_track_t`.
#[repr(C)]
pub struct SubtitleTrack {
    pub encoding: *mut c_char,
}

/// `libvlc_media_track_t`. The union is a single pointer; `repr(C)` inserts the
/// same 4 bytes of padding before it that the C compiler does on x86-64.
#[repr(C)]
pub struct MediaTrack {
    pub codec: u32,
    pub original_fourcc: u32,
    pub id: c_int,
    pub track_type: c_int,
    pub profile: c_int,
    pub level: c_int,
    pub specific: *mut c_void,
    pub bitrate: u32,
    pub language: *mut c_char,
    pub description: *mut c_char,
}

/// `libvlc_audio_output_device_t` — a linked list of audio output devices.
#[repr(C)]
pub struct AudioOutputDevice {
    pub next: *mut AudioOutputDevice,
    pub device: *mut c_char,
    pub description: *mut c_char,
}

/// `libvlc_track_description_t` — a linked list of live track ids and display names.
///
/// Returned by `libvlc_audio_get_track_description` and
/// `libvlc_video_get_spu_description`; must be freed with
/// `libvlc_track_description_list_release`.
#[repr(C)]
pub struct TrackDescription {
    pub i_id: c_int,
    pub psz_name: *mut c_char,
    pub p_next: *mut TrackDescription,
}

pub struct VlcApi {
    // Keep both DLLs resident for the lifetime of every handle that borrows these pointers.
    _core: Library,
    _lib: Library,
    pub new: unsafe extern "C" fn(c_int, *mut *mut c_char) -> *mut c_void,
    pub release: unsafe extern "C" fn(*mut c_void),
    pub errmsg: unsafe extern "C" fn() -> *const c_char,
    pub free: unsafe extern "C" fn(*mut c_void),
    pub media_new_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    pub media_new_location: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    pub media_add_option: unsafe extern "C" fn(*mut c_void, *const c_char),
    pub media_release: unsafe extern "C" fn(*mut c_void),
    pub media_add_slave: unsafe extern "C" fn(*mut c_void, c_int, c_int, *const c_char) -> c_int,
    pub player_new: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub player_new_from_media: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub player_release: unsafe extern "C" fn(*mut c_void),
    pub player_set_media: unsafe extern "C" fn(*mut c_void, *mut c_void),
    /// `libvlc_media_t *libvlc_media_player_get_media(mp)` — borrowed, do NOT release.
    pub player_get_media: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub player_play: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub player_pause: unsafe extern "C" fn(*mut c_void),
    pub player_stop: unsafe extern "C" fn(*mut c_void),
    pub player_is_playing: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub player_get_state: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub player_get_time: unsafe extern "C" fn(*mut c_void) -> i64,
    pub player_set_time: unsafe extern "C" fn(*mut c_void, i64) -> c_int,
    pub player_get_length: unsafe extern "C" fn(*mut c_void) -> i64,
    pub player_get_position: unsafe extern "C" fn(*mut c_void) -> f32,
    pub player_set_position: unsafe extern "C" fn(*mut c_void, f32) -> c_int,
    pub player_set_rate: unsafe extern "C" fn(*mut c_void, f32) -> c_int,
    pub player_get_rate: unsafe extern "C" fn(*mut c_void) -> f32,
    pub player_next_frame: unsafe extern "C" fn(*mut c_void),
    pub player_get_chapter_count: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub player_get_chapter: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub player_set_chapter: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    /// `int libvlc_media_player_get_full_chapter_descriptions(mp, title, ***chapters)`
    pub player_full_chapter_descriptions:
        unsafe extern "C" fn(*mut c_void, c_int, *mut *mut *mut ChapterDescription) -> c_int,
    pub chapter_descriptions_release: unsafe extern "C" fn(*mut *mut ChapterDescription, u32),
    /// `char *libvlc_media_get_meta(md, libvlc_meta_t)` — caller frees with `libvlc_free`.
    pub media_get_meta: unsafe extern "C" fn(*mut c_void, c_int) -> *mut c_char,
    /// `void libvlc_media_parse(md)` — synchronous parse; required before tags and tracks
    /// are populated on a media handle that has never been played.
    pub media_parse: unsafe extern "C" fn(*mut c_void),
    pub media_tracks_get: unsafe extern "C" fn(*mut c_void, *mut *mut *mut MediaTrack) -> u32,
    pub media_tracks_release: unsafe extern "C" fn(*mut *mut MediaTrack, u32),
    pub audio_get_volume: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub audio_set_volume: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    pub audio_toggle_mute: unsafe extern "C" fn(*mut c_void),
    pub audio_get_delay: unsafe extern "C" fn(*mut c_void) -> i64,
    pub audio_set_delay: unsafe extern "C" fn(*mut c_void, i64) -> c_int,
    pub audio_output_device_enum: unsafe extern "C" fn(*mut c_void) -> *mut AudioOutputDevice,
    pub audio_output_device_list_release: unsafe extern "C" fn(*mut AudioOutputDevice),
    pub audio_output_device_set: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char),
    pub audio_output_device_get: unsafe extern "C" fn(*mut c_void) -> *mut c_char,
    pub audio_get_track_count: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub audio_get_track: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub audio_set_track: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    /// `libvlc_track_description_t *libvlc_audio_get_track_description(mp)` — caller frees
    /// with `libvlc_track_description_list_release`.
    pub audio_get_track_description: unsafe extern "C" fn(*mut c_void) -> *mut TrackDescription,
    /// `libvlc_track_description_t *libvlc_video_get_spu_description(mp)`.
    pub video_get_spu_description: unsafe extern "C" fn(*mut c_void) -> *mut TrackDescription,
    pub track_description_list_release: unsafe extern "C" fn(*mut TrackDescription),
    pub video_get_spu_count: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub video_get_spu: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub video_set_spu: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    pub video_get_spu_delay: unsafe extern "C" fn(*mut c_void) -> i64,
    pub video_set_spu_delay: unsafe extern "C" fn(*mut c_void, i64) -> c_int,
    pub video_get_track_count: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub video_get_track: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub video_set_track: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    pub video_set_aspect_ratio: unsafe extern "C" fn(*mut c_void, *const c_char),
    pub video_set_adjust_int: unsafe extern "C" fn(*mut c_void, u32, c_int),
    pub video_set_adjust_float: unsafe extern "C" fn(*mut c_void, u32, f32),
    pub video_set_scale: unsafe extern "C" fn(*mut c_void, f32),
    pub equalizer_new_from_preset: unsafe extern "C" fn(u32) -> *mut c_void,
    pub equalizer_release: unsafe extern "C" fn(*mut c_void),
    pub player_set_equalizer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub video_take_snapshot:
        unsafe extern "C" fn(*mut c_void, c_int, *const c_char, c_int, c_int) -> c_int,
    pub event_manager: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub event_attach: unsafe extern "C" fn(*mut c_void, c_int, EventCb, *mut c_void) -> c_int,
    pub event_detach: unsafe extern "C" fn(*mut c_void, c_int, EventCb, *mut c_void),
    pub video_set_callbacks: unsafe extern "C" fn(
        *mut c_void,
        unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void,
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *mut c_void),
        unsafe extern "C" fn(*mut c_void, *mut c_void),
        *mut c_void,
    ),
    pub video_set_format: unsafe extern "C" fn(*mut c_void, *const c_char, u32, u32, u32),
    pub video_get_size: unsafe extern "C" fn(*mut c_void, u32, *mut u32, *mut u32) -> c_int,
}

impl VlcApi {
    /// SAFETY: the DLL paths must point at a matching libVLC 3.0.x pair.
    pub unsafe fn load(paths: &VlcPaths) -> Result<Self, VlcError> {
        let core = Library::new(&paths.core_dll)
            .map_err(|e| VlcError::LoadFailed(format!("{}: {e}", paths.core_dll.display())))?;
        let lib = Library::new(&paths.lib_dll)
            .map_err(|e| VlcError::LoadFailed(format!("{}: {e}", paths.lib_dll.display())))?;

        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let s: Symbol<$ty> = lib
                    .get::<$ty>($name)
                    .map_err(|e| VlcError::MissingSymbol(format!("{:?}: {e}", $name)))?;
                *s
            }};
        }

        Ok(Self {
            _core: core,
            new: sym!(
                b"libvlc_new\0",
                unsafe extern "C" fn(c_int, *mut *mut c_char) -> *mut c_void
            ),
            release: sym!(b"libvlc_release\0", unsafe extern "C" fn(*mut c_void)),
            errmsg: sym!(b"libvlc_errmsg\0", unsafe extern "C" fn() -> *const c_char),
            free: sym!(b"libvlc_free\0", unsafe extern "C" fn(*mut c_void)),
            media_new_path: sym!(
                b"libvlc_media_new_path\0",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void
            ),
            media_new_location: sym!(
                b"libvlc_media_new_location\0",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void
            ),
            media_add_option: sym!(
                b"libvlc_media_add_option\0",
                unsafe extern "C" fn(*mut c_void, *const c_char)
            ),
            media_release: sym!(b"libvlc_media_release\0", unsafe extern "C" fn(*mut c_void)),
            media_add_slave: sym!(
                b"libvlc_media_slaves_add\0",
                unsafe extern "C" fn(*mut c_void, c_int, c_int, *const c_char) -> c_int
            ),
            player_new: sym!(
                b"libvlc_media_player_new\0",
                unsafe extern "C" fn(*mut c_void) -> *mut c_void
            ),
            player_new_from_media: sym!(
                b"libvlc_media_player_new_from_media\0",
                unsafe extern "C" fn(*mut c_void) -> *mut c_void
            ),
            player_release: sym!(
                b"libvlc_media_player_release\0",
                unsafe extern "C" fn(*mut c_void)
            ),
            player_set_media: sym!(
                b"libvlc_media_player_set_media\0",
                unsafe extern "C" fn(*mut c_void, *mut c_void)
            ),
            player_get_media: sym!(
                b"libvlc_media_player_get_media\0",
                unsafe extern "C" fn(*mut c_void) -> *mut c_void
            ),
            player_play: sym!(
                b"libvlc_media_player_play\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            player_pause: sym!(
                b"libvlc_media_player_pause\0",
                unsafe extern "C" fn(*mut c_void)
            ),
            player_stop: sym!(
                b"libvlc_media_player_stop\0",
                unsafe extern "C" fn(*mut c_void)
            ),
            player_is_playing: sym!(
                b"libvlc_media_player_is_playing\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            player_get_state: sym!(
                b"libvlc_media_player_get_state\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            player_get_time: sym!(
                b"libvlc_media_player_get_time\0",
                unsafe extern "C" fn(*mut c_void) -> i64
            ),
            player_set_time: sym!(
                b"libvlc_media_player_set_time\0",
                unsafe extern "C" fn(*mut c_void, i64) -> c_int
            ),
            player_get_length: sym!(
                b"libvlc_media_player_get_length\0",
                unsafe extern "C" fn(*mut c_void) -> i64
            ),
            player_get_position: sym!(
                b"libvlc_media_player_get_position\0",
                unsafe extern "C" fn(*mut c_void) -> f32
            ),
            player_set_position: sym!(
                b"libvlc_media_player_set_position\0",
                unsafe extern "C" fn(*mut c_void, f32) -> c_int
            ),
            player_set_rate: sym!(
                b"libvlc_media_player_set_rate\0",
                unsafe extern "C" fn(*mut c_void, f32) -> c_int
            ),
            player_get_rate: sym!(
                b"libvlc_media_player_get_rate\0",
                unsafe extern "C" fn(*mut c_void) -> f32
            ),
            player_next_frame: sym!(
                b"libvlc_media_player_next_frame\0",
                unsafe extern "C" fn(*mut c_void)
            ),
            player_get_chapter_count: sym!(
                b"libvlc_media_player_get_chapter_count\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            player_get_chapter: sym!(
                b"libvlc_media_player_get_chapter\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            player_set_chapter: sym!(
                b"libvlc_media_player_set_chapter\0",
                unsafe extern "C" fn(*mut c_void, c_int) -> c_int
            ),
            player_full_chapter_descriptions: sym!(
                b"libvlc_media_player_get_full_chapter_descriptions\0",
                unsafe extern "C" fn(
                    *mut c_void,
                    c_int,
                    *mut *mut *mut ChapterDescription,
                ) -> c_int
            ),
            chapter_descriptions_release: sym!(
                b"libvlc_chapter_descriptions_release\0",
                unsafe extern "C" fn(*mut *mut ChapterDescription, u32)
            ),
            media_get_meta: sym!(
                b"libvlc_media_get_meta\0",
                unsafe extern "C" fn(*mut c_void, c_int) -> *mut c_char
            ),
            media_parse: sym!(b"libvlc_media_parse\0", unsafe extern "C" fn(*mut c_void)),
            media_tracks_get: sym!(
                b"libvlc_media_tracks_get\0",
                unsafe extern "C" fn(*mut c_void, *mut *mut *mut MediaTrack) -> u32
            ),
            media_tracks_release: sym!(
                b"libvlc_media_tracks_release\0",
                unsafe extern "C" fn(*mut *mut MediaTrack, u32)
            ),
            audio_get_volume: sym!(
                b"libvlc_audio_get_volume\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            audio_set_volume: sym!(
                b"libvlc_audio_set_volume\0",
                unsafe extern "C" fn(*mut c_void, c_int) -> c_int
            ),
            audio_toggle_mute: sym!(
                b"libvlc_audio_toggle_mute\0",
                unsafe extern "C" fn(*mut c_void)
            ),
            audio_get_delay: sym!(
                b"libvlc_audio_get_delay\0",
                unsafe extern "C" fn(*mut c_void) -> i64
            ),
            audio_set_delay: sym!(
                b"libvlc_audio_set_delay\0",
                unsafe extern "C" fn(*mut c_void, i64) -> c_int
            ),
            audio_output_device_enum: sym!(
                b"libvlc_audio_output_device_enum\0",
                unsafe extern "C" fn(*mut c_void) -> *mut AudioOutputDevice
            ),
            audio_output_device_list_release: sym!(
                b"libvlc_audio_output_device_list_release\0",
                unsafe extern "C" fn(*mut AudioOutputDevice)
            ),
            audio_output_device_set: sym!(
                b"libvlc_audio_output_device_set\0",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)
            ),
            audio_output_device_get: sym!(
                b"libvlc_audio_output_device_get\0",
                unsafe extern "C" fn(*mut c_void) -> *mut c_char
            ),
            audio_get_track_count: sym!(
                b"libvlc_audio_get_track_count\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            audio_get_track: sym!(
                b"libvlc_audio_get_track\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            audio_set_track: sym!(
                b"libvlc_audio_set_track\0",
                unsafe extern "C" fn(*mut c_void, c_int) -> c_int
            ),
            audio_get_track_description: sym!(
                b"libvlc_audio_get_track_description\0",
                unsafe extern "C" fn(*mut c_void) -> *mut TrackDescription
            ),
            video_get_spu_description: sym!(
                b"libvlc_video_get_spu_description\0",
                unsafe extern "C" fn(*mut c_void) -> *mut TrackDescription
            ),
            track_description_list_release: sym!(
                b"libvlc_track_description_list_release\0",
                unsafe extern "C" fn(*mut TrackDescription)
            ),
            video_get_spu_count: sym!(
                b"libvlc_video_get_spu_count\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            video_get_spu: sym!(
                b"libvlc_video_get_spu\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            video_set_spu: sym!(
                b"libvlc_video_set_spu\0",
                unsafe extern "C" fn(*mut c_void, c_int) -> c_int
            ),
            video_get_spu_delay: sym!(
                b"libvlc_video_get_spu_delay\0",
                unsafe extern "C" fn(*mut c_void) -> i64
            ),
            video_set_spu_delay: sym!(
                b"libvlc_video_set_spu_delay\0",
                unsafe extern "C" fn(*mut c_void, i64) -> c_int
            ),
            video_get_track_count: sym!(
                b"libvlc_video_get_track_count\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            video_get_track: sym!(
                b"libvlc_video_get_track\0",
                unsafe extern "C" fn(*mut c_void) -> c_int
            ),
            video_set_track: sym!(
                b"libvlc_video_set_track\0",
                unsafe extern "C" fn(*mut c_void, c_int) -> c_int
            ),
            video_set_aspect_ratio: sym!(
                b"libvlc_video_set_aspect_ratio\0",
                unsafe extern "C" fn(*mut c_void, *const c_char)
            ),
            video_set_adjust_int: sym!(
                b"libvlc_video_set_adjust_int\0",
                unsafe extern "C" fn(*mut c_void, u32, c_int)
            ),
            video_set_adjust_float: sym!(
                b"libvlc_video_set_adjust_float\0",
                unsafe extern "C" fn(*mut c_void, u32, f32)
            ),
            video_set_scale: sym!(
                b"libvlc_video_set_scale\0",
                unsafe extern "C" fn(*mut c_void, f32)
            ),
            equalizer_new_from_preset: sym!(
                b"libvlc_audio_equalizer_new_from_preset\0",
                unsafe extern "C" fn(u32) -> *mut c_void
            ),
            equalizer_release: sym!(
                b"libvlc_audio_equalizer_release\0",
                unsafe extern "C" fn(*mut c_void)
            ),
            player_set_equalizer: sym!(
                b"libvlc_media_player_set_equalizer\0",
                unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int
            ),
            video_take_snapshot: sym!(
                b"libvlc_video_take_snapshot\0",
                unsafe extern "C" fn(*mut c_void, c_int, *const c_char, c_int, c_int) -> c_int
            ),
            event_manager: sym!(
                b"libvlc_media_player_event_manager\0",
                unsafe extern "C" fn(*mut c_void) -> *mut c_void
            ),
            event_attach: sym!(
                b"libvlc_event_attach\0",
                unsafe extern "C" fn(*mut c_void, c_int, EventCb, *mut c_void) -> c_int
            ),
            event_detach: sym!(
                b"libvlc_event_detach\0",
                unsafe extern "C" fn(*mut c_void, c_int, EventCb, *mut c_void)
            ),
            video_set_callbacks: sym!(
                b"libvlc_video_set_callbacks\0",
                unsafe extern "C" fn(
                    *mut c_void,
                    unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void,
                    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *mut c_void),
                    unsafe extern "C" fn(*mut c_void, *mut c_void),
                    *mut c_void,
                )
            ),
            video_set_format: sym!(
                b"libvlc_video_set_format\0",
                unsafe extern "C" fn(*mut c_void, *const c_char, u32, u32, u32)
            ),
            video_get_size: sym!(
                b"libvlc_video_get_size\0",
                unsafe extern "C" fn(*mut c_void, u32, *mut u32, *mut u32) -> c_int
            ),
            _lib: lib,
        })
    }

    /// Last libVLC error string, if any. libVLC owns the returned memory.
    pub fn last_error(&self) -> Option<String> {
        unsafe {
            let p = (self.errmsg)();
            if p.is_null() {
                None
            } else {
                Some(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned())
            }
        }
    }
}

const _: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void = VIDEO_LOCK_CB;
const _: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *mut c_void) = VIDEO_UNLOCK_CB;
const _: unsafe extern "C" fn(*mut c_void, *mut c_void) = VIDEO_DISPLAY_CB;
