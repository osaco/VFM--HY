//! Safe-ish façade over audited libVLC 3.0.x FFI. All `unsafe` stays in this crate;
//! `core` and `app` contain no unsafe code of their own.

pub mod events;
pub mod loader;
#[allow(clippy::missing_safety_doc)]
pub mod sys;
pub mod video;

use std::ffi::{c_void, CStr, CString};
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use events::{EventHub, PlayerEvent};
use loader::VlcPaths;
use sys::VlcApi;
pub use video::{swizzle_rgba, FrameSwap, PixelFormat};

/// One audio output device as libVLC sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    /// Identifier handed back to `set_audio_output_device`.
    pub id: String,
    /// Human-readable name for the UI dropdown.
    pub name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum VlcError {
    #[error("libVLC runtime not found ({0}); set LIBVLC_PATH to a win64 3.0.x directory")]
    LibraryNotFound(String),
    #[error("failed to load libVLC DLL: {0}")]
    LoadFailed(String),
    #[error("libVLC DLL is missing symbol: {0}")]
    MissingSymbol(String),
    #[error("libVLC could not create an instance: {0}")]
    InstanceFailed(String),
    #[error("invalid media path")]
    InvalidMediaPath,
    #[error("invalid frame dimensions {0}x{1}")]
    InvalidDimensions(u32, u32),
    #[error("unsupported chroma {0:?}; use rgba or rv32")]
    InvalidChroma(String),
}

/// Convert a path into the string form libVLC expects on this platform.
///
/// libVLC on Windows only accepts native `\` separators for **absolute** paths:
/// `libvlc_media_new_path("D:/media/ep.mkv")` returns a NULL handle, while
/// `"D:\\media\\ep.mkv"` succeeds. Rust's `Path` keeps whatever separator it was
/// built from, so a path that arrived over the command line, a drag-and-drop or
/// a config file can carry forward slashes. Normalise them here — once, at the
/// FFI boundary — instead of at every call site.
///
/// Strings that look like URLs (`scheme://…`) are passed through untouched, and
/// non-Windows platforms keep their native separator.
pub fn vlc_path_string(path: &Path) -> String {
    let raw = path.to_string_lossy().into_owned();
    if raw.contains("://") {
        return raw;
    }
    if cfg!(windows) {
        raw.replace('/', "\\")
    } else {
        raw
    }
}

/// Tag metadata read from `libvlc_media_get_meta`. Empty strings mean "not tagged".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaMetadata {
    pub title: String,
    pub artist: String,
    pub genre: String,
    pub album: String,
    pub track_number: String,
    pub description: String,
    pub date: String,
    pub url: String,
    pub language: String,
    pub publisher: String,
    pub encoded_by: String,
    pub season: String,
    pub episode: String,
    pub show_name: String,
    pub actors: String,
    pub album_artist: String,
    pub disc_number: String,
}

impl MediaMetadata {
    /// `true` when the file carries no usable tag at all.
    pub fn is_empty(&self) -> bool {
        self.title.is_empty()
            && self.artist.is_empty()
            && self.album.is_empty()
            && self.show_name.is_empty()
            && self.genre.is_empty()
    }
}

/// One entry of the chapter list. Times are milliseconds from the start of the media.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    pub start_ms: i64,
    pub duration_ms: i64,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoTrackInfo {
    pub codec: u32,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f32,
    pub bitrate: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioTrackInfo {
    pub codec: u32,
    pub channels: u32,
    pub rate: u32,
    pub bitrate: u32,
}

/// Stream layout of a parsed media file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackSummary {
    pub video: Option<VideoTrackInfo>,
    pub audio: Vec<AudioTrackInfo>,
}

/// A loaded libVLC instance. Keeps both DLLs resident via `Arc<VlcApi>`.
pub struct Instance {
    handle: *mut c_void,
    api: Arc<VlcApi>,
}

impl std::fmt::Debug for Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Instance")
    }
}

// SAFETY: libVLC's instance/media/player APIs are documented as callable from any thread
// (excluding callback re-entry, which this wrapper never does).
unsafe impl Send for Instance {}
unsafe impl Sync for Instance {}
unsafe impl Send for Media {}
unsafe impl Sync for Media {}
unsafe impl Send for MediaPlayer {}
unsafe impl Sync for MediaPlayer {}

impl Instance {
    pub fn new() -> Result<Self, VlcError> {
        let paths = VlcPaths::discover()?;
        let api = Arc::new(unsafe { sys::VlcApi::load(&paths)? });
        let args: Vec<CString> = ["--no-video-title-show", "--no-osd", "--quiet"]
            .iter()
            .map(|s| CString::new(*s).unwrap())
            .collect();
        let mut argv: Vec<*mut i8> = args.iter().map(|s| s.as_ptr() as *mut i8).collect();
        // SAFETY: argv points at CStrings alive for the duration of this call; libVLC copies args.
        let handle = unsafe { (api.new)(argv.len() as i32, argv.as_mut_ptr()) };
        if handle.is_null() {
            return Err(VlcError::InstanceFailed(
                api.last_error().unwrap_or_default(),
            ));
        }
        Ok(Self { handle, api })
    }

    /// Create a player with no media attached (library-first startup). Call `set_media`
    /// before `play()`. Video callbacks still need enabling before playback.
    pub fn empty_player(&self) -> Result<MediaPlayer, VlcError> {
        let h = unsafe { (self.api.player_new)(self.handle) };
        if h.is_null() {
            return Err(VlcError::InstanceFailed(
                self.api.last_error().unwrap_or_default(),
            ));
        }
        Ok(MediaPlayer {
            handle: h,
            api: Arc::clone(&self.api),
            frames: Mutex::new(None),
            events: None,
        })
    }

    pub fn runtime_dir(&self) -> String {
        loader::plugin_path_var()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn media_from_path(&self, path: &Path) -> Result<Media, VlcError> {
        let c = CString::new(vlc_path_string(path).as_bytes())
            .map_err(|_| VlcError::InvalidMediaPath)?;
        // SAFETY: `self.handle` is a live libvlc_instance_t; CString outlives the call.
        let h = unsafe { (self.api.media_new_path)(self.handle, c.as_ptr()) };
        if h.is_null() {
            return Err(VlcError::InvalidMediaPath);
        }
        Ok(Media {
            handle: h,
            api: Arc::clone(&self.api),
        })
    }

    /// Current UI language as a libVLC option, used for libVLC's own OSD/messages.
    pub fn set_language(&self, lang: &str) -> Result<(), VlcError> {
        // Placeholder for Phase 2 i18n integration; language is chosen at instance creation.
        let _ = lang;
        Ok(())
    }

    /// Open a stream by URL (`http://`, `rtsp://`, `dshow://`, `screen://`, …).
    ///
    /// `libvlc_media_new_path` would treat the same string as a local file name, so network
    /// and capture sources have to go through the location constructor.
    pub fn media_from_location(&self, location: &str) -> Result<Media, VlcError> {
        let c = CString::new(location).map_err(|_| VlcError::InvalidMediaPath)?;
        // SAFETY: instance handle is live and the CString outlives the call.
        let h = unsafe { (self.api.media_new_location)(self.handle, c.as_ptr()) };
        if h.is_null() {
            return Err(VlcError::InvalidMediaPath);
        }
        Ok(Media {
            handle: h,
            api: Arc::clone(&self.api),
        })
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe { (self.api.release)(self.handle) };
    }
}

pub struct Media {
    handle: *mut c_void,
    api: Arc<VlcApi>,
}

impl Media {
    pub fn player(&self) -> Result<MediaPlayer, VlcError> {
        // SAFETY: media handle is alive; libvlc_media_player_new_from_media retains it.
        let h = unsafe { (self.api.player_new_from_media)(self.handle) };
        if h.is_null() {
            return Err(VlcError::InstanceFailed(
                self.api.last_error().unwrap_or_default(),
            ));
        }
        Ok(MediaPlayer {
            handle: h,
            api: Arc::clone(&self.api),
            frames: Mutex::new(None),
            events: None,
        })
    }

    /// Add a media option (`:sout=…`, `:dshow-vdev=…`, `:network-caching=…`).
    ///
    /// The leading colon is optional — libVLC accepts both `:opt=value` and `opt=value`.
    pub fn add_option(&self, option: &str) -> Result<(), VlcError> {
        let trimmed = option.trim_start_matches(':');
        if trimmed.is_empty() {
            return Ok(());
        }
        let c = CString::new(trimmed).map_err(|_| VlcError::InvalidMediaPath)?;
        // SAFETY: live media handle; libVLC copies the string before returning.
        unsafe { (self.api.media_add_option)(self.handle, c.as_ptr()) };
        Ok(())
    }

    /// Attach an external subtitle file to this media (libVLC media slave, type 0 = text).
    pub fn add_subtitle_file(&self, path: &Path) -> Result<(), VlcError> {
        let c = CString::new(vlc_path_string(path).as_bytes())
            .map_err(|_| VlcError::InvalidMediaPath)?;
        // SAFETY: live media handle; CString outlives the call. libVLC copies the URI.
        let rc = unsafe { (self.api.media_add_slave)(self.handle, 0, 0, c.as_ptr()) };
        if rc != 0 {
            return Err(VlcError::InstanceFailed(format!(
                "add subtitle returned {rc}"
            )));
        }
        Ok(())
    }

    /// Read one `libvlc_meta_t` field. libVLC hands back an allocated UTF-8 string
    /// that must be released with `libvlc_free`.
    fn meta(&self, field: i32) -> String {
        // SAFETY: `self.handle` is a live media handle. The returned pointer is either
        // NULL or a NUL-terminated string owned by libVLC; it is freed exactly once here.
        unsafe {
            let raw = (self.api.media_get_meta)(self.handle, field);
            if raw.is_null() {
                return String::new();
            }
            let text = CStr::from_ptr(raw).to_string_lossy().into_owned();
            (self.api.free)(raw.cast());
            text
        }
    }

    /// Tag metadata (title, artist, album, show/season/episode …) for the metadata panel.
    ///
    /// Field order follows `libvlc_meta_t`; see `docs/phase-7.md`.
    pub fn metadata(&self) -> MediaMetadata {
        // Tags are only available once the demuxer has read the header.
        self.parse();
        MediaMetadata {
            title: self.meta(0),
            artist: self.meta(1),
            genre: self.meta(2),
            album: self.meta(4),
            track_number: self.meta(5),
            description: self.meta(6),
            date: self.meta(8),
            url: self.meta(10),
            language: self.meta(11),
            publisher: self.meta(13),
            encoded_by: self.meta(14),
            season: self.meta(19),
            episode: self.meta(20),
            show_name: self.meta(21),
            actors: self.meta(22),
            album_artist: self.meta(23),
            disc_number: self.meta(24),
        }
    }

    /// Parse the media synchronously so tags and track descriptions are populated.
    ///
    /// `libvlc_media_tracks_get` returns an empty list on an unparsed handle, and
    /// `libvlc_media_get_meta` returns empty strings, so both readers call this first.
    /// Parsing is cached inside the media object, making repeat calls cheap.
    pub fn parse(&self) {
        // SAFETY: live media handle; libVLC blocks until the demuxer has read the header.
        unsafe { (self.api.media_parse)(self.handle) };
    }

    /// Video/audio stream details from `libvlc_media_tracks_get`.
    ///
    /// Must be called after the media has been parsed; libVLC returns an empty list
    /// before that, which surfaces as `TrackSummary::default()`.
    pub fn tracks(&self) -> TrackSummary {
        read_tracks(&self.api, self.handle, true)
    }

    /// Track list, waiting for the asynchronous preparse to land.
    ///
    /// `libvlc_media_parse` only *requests* a preparse in libVLC 3.0 — it returns long before
    /// the demuxer has filled the track array, so reading straight after it always reports
    /// "no tracks". Polling the same handle is the cheapest way to get a synchronous answer.
    pub fn tracks_wait(&self, timeout: std::time::Duration) -> TrackSummary {
        let deadline = std::time::Instant::now() + timeout;
        self.parse();
        loop {
            let mut list: *mut *mut sys::MediaTrack = std::ptr::null_mut();
            // SAFETY: live media handle; libVLC allocates the array and each entry.
            let count = unsafe { (self.api.media_tracks_get)(self.handle, &mut list) };
            if count > 0 && !list.is_null() {
                return read_tracks(&self.api, self.handle, false);
            }
            if std::time::Instant::now() >= deadline {
                return TrackSummary::default();
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

/// Read the track list of a media handle.
///
/// `parse_if_empty` triggers a parse when the list comes back empty. Note that
/// `libvlc_media_parse` is *asynchronous* in libVLC 3.0 — it posts a preparse request and
/// returns immediately — so a handle that has never been played still reports nothing on the
/// next line. A handle the player owns is already demuxed and needs no parse at all, which is
/// why `MediaPlayer::media_tracks` passes `false`.
fn read_tracks(api: &VlcApi, handle: *mut c_void, parse_if_empty: bool) -> TrackSummary {
    let mut list: *mut *mut sys::MediaTrack = std::ptr::null_mut();
    // SAFETY: live media handle; libVLC allocates the array and each entry.
    let mut count = unsafe { (api.media_tracks_get)(handle, &mut list) };
    if count == 0 && parse_if_empty {
        // A handle that was never played has no track list yet; parse once and retry.
        unsafe { (api.media_parse)(handle) };
        count = unsafe { (api.media_tracks_get)(handle, &mut list) };
    }
    if count == 0 || list.is_null() {
        return TrackSummary::default();
    }
    let mut summary = TrackSummary::default();
    for index in 0..count as usize {
        // SAFETY: `count` entries were written by libVLC; each pointer may be NULL.
        let track = unsafe { *list.add(index) };
        if track.is_null() {
            continue;
        }
        let track = unsafe { &*track };
        match track.track_type {
            // `libvlc_track_type_t` is zero-based: 0 = audio, 1 = video, 2 = text.
            // (Off-by-one here reads a video track as audio, so `tracks.video` is always None
            // and every file is mistaken for audio-only — it also hides the real frame size.)
            0 if !track.specific.is_null() => {
                let audio = unsafe { &*(track.specific as *const sys::AudioTrack) };
                summary.audio.push(AudioTrackInfo {
                    codec: track.codec,
                    channels: audio.channels,
                    rate: audio.rate,
                    bitrate: track.bitrate,
                });
            }
            1 if !track.specific.is_null() => {
                let video = unsafe { &*(track.specific as *const sys::VideoTrack) };
                if summary.video.is_none() {
                    let fps = if video.frame_rate_den > 0 {
                        video.frame_rate_num as f32 / video.frame_rate_den as f32
                    } else {
                        0.0
                    };
                    summary.video = Some(VideoTrackInfo {
                        codec: track.codec,
                        width: video.width,
                        height: video.height,
                        frame_rate: fps,
                        bitrate: track.bitrate,
                    });
                }
            }
            _ => {}
        }
    }
    // SAFETY: frees the array and every entry libVLC allocated for us.
    unsafe { (api.media_tracks_release)(list, count) };
    summary
}

impl Drop for Media {
    fn drop(&mut self) {
        unsafe { (self.api.media_release)(self.handle) };
    }
}

/// Playback handle. Stops libVLC before dropping the callback-owned frame buffers.
pub struct MediaPlayer {
    handle: *mut c_void,
    api: Arc<VlcApi>,
    /// The live frame bridge. Behind a `Mutex` so a new file can re-arm the pipeline with its
    /// own dimensions through `&self` — the shell shares the player as an `Arc`.
    frames: Mutex<Option<Arc<FrameSwap>>>,
    events: Option<EventHub>,
}

impl MediaPlayer {
    /// Install the RV32/RGBA callback pipeline. Must be called before `play()`.
    pub fn enable_video_callbacks(
        &mut self,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> Result<(), VlcError> {
        let frames = FrameSwap::new(width, height, format)?;
        let opaque = Arc::as_ptr(&frames) as *mut c_void;
        // SAFETY: Arc<FrameSwap> is cloned into self.frames below, so `opaque` stays valid
        // until drop (player stopped + released first). Trampolines never re-enter libVLC.
        unsafe {
            (self.api.video_set_callbacks)(
                self.handle,
                video::VIDEO_LOCK_CB,
                video::VIDEO_UNLOCK_CB,
                video::VIDEO_DISPLAY_CB,
                opaque,
            );
            (self.api.video_set_format)(
                self.handle,
                format.fourcc().as_ptr() as *const i8,
                width,
                height,
                width * 4,
            );
        }
        *self.frames.lock().unwrap() = Some(frames);
        Ok(())
    }

    /// Point the pipeline at a different frame size, e.g. when the next file is 4K instead of
    /// 1080p. `libvlc_video_set_format` may only be called while the vout does not exist, so
    /// this stops playback first; libVLC's stop is synchronous, which also guarantees the old
    /// buffer is no longer written to when its `Arc` is dropped below.
    ///
    /// Returns `Ok(false)` when the pipeline already matches, so callers can skip the stop.
    pub fn rearm_video_callbacks(
        &self,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> Result<bool, VlcError> {
        if let Some(current) = self.frames.lock().unwrap().as_ref() {
            if current.width() == width && current.height() == height && current.format() == format
            {
                return Ok(false);
            }
        }
        let frames = FrameSwap::new(width, height, format)?;
        // SAFETY: stop first so no vout callback is in flight; the new Arc is stored in
        // `self.frames` below, keeping `opaque` valid for as long as libVLC can call back.
        unsafe {
            (self.api.player_stop)(self.handle);
            let opaque = Arc::as_ptr(&frames) as *mut c_void;
            (self.api.video_set_callbacks)(
                self.handle,
                video::VIDEO_LOCK_CB,
                video::VIDEO_UNLOCK_CB,
                video::VIDEO_DISPLAY_CB,
                opaque,
            );
            (self.api.video_set_format)(
                self.handle,
                format.fourcc().as_ptr() as *const i8,
                width,
                height,
                width * 4,
            );
        }
        *self.frames.lock().unwrap() = Some(frames);
        Ok(true)
    }

    /// Attach typed libVLC events. The receiver is intended for the controller/UI thread.
    pub fn subscribe_events(&mut self) -> Result<Receiver<PlayerEvent>, VlcError> {
        // SAFETY: `self.handle` is live until Drop, and `self.events` outlives no player use;
        // MediaPlayer::drop clears the hub before releasing the handle.
        let (hub, rx) = unsafe { EventHub::attach(Arc::clone(&self.api), self.handle)? };
        self.events = Some(hub);
        Ok(rx)
    }

    /// Swap the media without recreating the player (used by the queue).
    pub fn set_media(&self, media: &Media) {
        unsafe { (self.api.player_set_media)(self.handle, media.handle) };
    }

    pub fn frame_bridge(&self) -> Option<Arc<FrameSwap>> {
        self.frames.lock().unwrap().clone()
    }

    /// Tracks of the media the player currently owns.
    ///
    /// Unlike `Media::tracks`, this needs no parse: the player's demuxer has already read the
    /// header, so the list is populated as soon as playback starts. It is the only reliable way
    /// to learn the real frame size — `libvlc_video_get_size` reports the format we asked for,
    /// not the source's.
    pub fn media_tracks(&self) -> TrackSummary {
        // SAFETY: `libvlc_media_player_get_media` returns a borrowed handle owned by the
        // player; it must not be released, and it stays valid while the player is alive.
        let media = unsafe { (self.api.player_get_media)(self.handle) };
        if media.is_null() {
            return TrackSummary::default();
        }
        read_tracks(&self.api, media, false)
    }

    /// Size of the video libVLC is decoding, once a vout exists (i.e. after `play()`).
    /// Returns `None` before playback starts, for audio-only media, or when the demuxer has
    /// not resolved the video track yet.
    pub fn video_size(&self) -> Option<(u32, u32)> {
        let mut width = 0u32;
        let mut height = 0u32;
        // SAFETY: live player handle; both out-parameters are stack locals.
        let status = unsafe { (self.api.video_get_size)(self.handle, 0, &mut width, &mut height) };
        (status == 0 && width > 0 && height > 0).then_some((width, height))
    }

    pub fn play(&self) -> Result<(), VlcError> {
        let rc = unsafe { (self.api.player_play)(self.handle) };
        if rc != 0 {
            return Err(VlcError::InstanceFailed(
                self.api
                    .last_error()
                    .unwrap_or_else(|| format!("play returned {rc}")),
            ));
        }
        Ok(())
    }

    pub fn pause(&self) {
        unsafe { (self.api.player_pause)(self.handle) };
    }

    pub fn toggle_pause(&self) {
        if self.is_playing() {
            self.pause()
        } else {
            let _ = self.play();
        }
    }

    pub fn stop(&self) {
        unsafe { (self.api.player_stop)(self.handle) };
    }

    pub fn is_playing(&self) -> bool {
        unsafe { (self.api.player_is_playing)(self.handle) != 0 }
    }

    /// Raw libVLC state: 1=Opening 2=Buffering 3=Playing 4=Paused 5=Stopped 6=Ended 7=Error.
    pub fn state_code(&self) -> i32 {
        unsafe { (self.api.player_get_state)(self.handle) }
    }

    pub fn time_ms(&self) -> i64 {
        unsafe { (self.api.player_get_time)(self.handle) }
    }
    pub fn set_time_ms(&self, ms: i64) {
        unsafe { (self.api.player_set_time)(self.handle, ms) };
    }
    pub fn length_ms(&self) -> i64 {
        unsafe { (self.api.player_get_length)(self.handle) }
    }
    pub fn position(&self) -> f32 {
        unsafe { (self.api.player_get_position)(self.handle) }.clamp(0.0, 1.0)
    }
    pub fn set_position(&self, pos: f32) {
        unsafe { (self.api.player_set_position)(self.handle, pos.clamp(0.0, 1.0)) };
    }
    pub fn set_rate(&self, rate: f32) -> Result<(), VlcError> {
        let rc = unsafe { (self.api.player_set_rate)(self.handle, rate.clamp(0.25, 8.0)) };
        if rc != 0 {
            Err(VlcError::InstanceFailed("set_rate failed".into()))
        } else {
            Ok(())
        }
    }
    pub fn rate(&self) -> f32 {
        unsafe { (self.api.player_get_rate)(self.handle) }
    }
    pub fn next_frame(&self) {
        unsafe { (self.api.player_next_frame)(self.handle) };
    }

    pub fn volume(&self) -> i32 {
        unsafe { (self.api.audio_get_volume)(self.handle) }
    }
    pub fn set_volume(&self, vol: i32) {
        unsafe { (self.api.audio_set_volume)(self.handle, vol.clamp(0, 200)) };
    }
    pub fn toggle_mute(&self) {
        unsafe { (self.api.audio_toggle_mute)(self.handle) };
    }

    /// Audio output devices libVLC can currently enumerate.
    ///
    /// The list comes from the active audio output module (WASAPI/MMDevice on Windows);
    /// an empty list means the module does not enumerate devices, which is not an error.
    pub fn audio_output_devices(&self) -> Vec<AudioDeviceInfo> {
        // SAFETY: the returned list is owned by us and released before returning; strings
        // are copied out before release.
        unsafe {
            let head = (self.api.audio_output_device_enum)(self.handle);
            let mut out = Vec::new();
            let mut node = head;
            while !node.is_null() {
                let entry = &*node;
                let id = if entry.device.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(entry.device).to_string_lossy().into_owned()
                };
                let name = if entry.description.is_null() {
                    id.clone()
                } else {
                    CStr::from_ptr(entry.description)
                        .to_string_lossy()
                        .into_owned()
                };
                if !id.is_empty() {
                    out.push(AudioDeviceInfo { id, name });
                }
                node = entry.next;
            }
            (self.api.audio_output_device_list_release)(head);
            out
        }
    }

    /// Route playback to a specific audio output device (`device_id` from
    /// [`audio_output_devices`]), or back to the system default when `None`.
    pub fn set_audio_output_device(&self, device_id: Option<&str>) {
        // SAFETY: both C strings are valid NUL-terminated data for the duration of the
        // call; libVLC copies what it needs.
        unsafe {
            match device_id {
                Some(id) => {
                    let id = CString::new(id).unwrap_or_default();
                    (self.api.audio_output_device_set)(self.handle, std::ptr::null(), id.as_ptr());
                }
                None => {
                    (self.api.audio_output_device_set)(
                        self.handle,
                        std::ptr::null(),
                        std::ptr::null(),
                    );
                }
            }
        }
    }

    /// The device identifier currently selected, if libVLC reports one.
    pub fn audio_output_device(&self) -> Option<String> {
        // SAFETY: the returned string is owned by us and freed before returning.
        unsafe {
            let raw = (self.api.audio_output_device_get)(self.handle);
            if raw.is_null() {
                return None;
            }
            let out = CStr::from_ptr(raw).to_string_lossy().into_owned();
            (self.api.free)(raw as *mut c_void);
            (!out.is_empty()).then_some(out)
        }
    }
    pub fn audio_delay_ms(&self) -> i64 {
        unsafe { (self.api.audio_get_delay)(self.handle) }
    }
    pub fn set_audio_delay_ms(&self, ms: i64) {
        unsafe { (self.api.audio_set_delay)(self.handle, ms) };
    }
    pub fn subtitle_delay_ms(&self) -> i64 {
        unsafe { (self.api.video_get_spu_delay)(self.handle) }
    }
    pub fn set_subtitle_delay_ms(&self, ms: i64) {
        unsafe { (self.api.video_set_spu_delay)(self.handle, ms) };
    }

    pub fn audio_track_count(&self) -> i32 {
        unsafe { (self.api.audio_get_track_count)(self.handle) }
    }
    pub fn audio_track(&self) -> i32 {
        unsafe { (self.api.audio_get_track)(self.handle) }
    }
    pub fn set_audio_track(&self, id: i32) {
        unsafe { (self.api.audio_set_track)(self.handle, id) };
    }
    pub fn subtitle_track_count(&self) -> i32 {
        unsafe { (self.api.video_get_spu_count)(self.handle) }
    }
    pub fn subtitle_track(&self) -> i32 {
        unsafe { (self.api.video_get_spu)(self.handle) }
    }
    pub fn set_subtitle_track(&self, id: i32) {
        unsafe { (self.api.video_set_spu)(self.handle, id) };
    }
    pub fn video_track_count(&self) -> i32 {
        unsafe { (self.api.video_get_track_count)(self.handle) }
    }
    pub fn video_track(&self) -> i32 {
        unsafe { (self.api.video_get_track)(self.handle) }
    }
    pub fn set_video_track(&self, id: i32) {
        unsafe { (self.api.video_set_track)(self.handle, id) };
    }

    pub fn set_aspect_ratio(&self, ratio: Option<&str>) -> Result<(), VlcError> {
        let c = ratio
            .map(|s| CString::new(s).map_err(|_| VlcError::InvalidChroma(s.into())))
            .transpose()?;
        unsafe {
            (self.api.video_set_aspect_ratio)(
                self.handle,
                c.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            )
        };
        Ok(())
    }

    /// Apply the five libVLC adjust-filter values. The filter is enabled on first use.
    /// Ranges: contrast/brightness/gamma 0.0–2.0, hue -180..180, saturation 0.0–3.0.
    pub fn set_video_adjustments(
        &self,
        contrast: f32,
        brightness: f32,
        hue: f32,
        saturation: f32,
        gamma: f32,
    ) {
        unsafe {
            (self.api.video_set_adjust_int)(self.handle, 0, 1); // libvlc_adjust_Enable
            (self.api.video_set_adjust_float)(self.handle, 1, contrast.clamp(0.0, 2.0));
            (self.api.video_set_adjust_float)(self.handle, 2, brightness.clamp(0.0, 2.0));
            (self.api.video_set_adjust_float)(self.handle, 3, hue.clamp(-180.0, 180.0));
            (self.api.video_set_adjust_float)(self.handle, 4, saturation.clamp(0.0, 3.0));
            (self.api.video_set_adjust_float)(self.handle, 5, gamma.clamp(0.0, 2.0));
        }
    }

    pub fn set_zoom(&self, factor: f32) {
        unsafe { (self.api.video_set_scale)(self.handle, factor.clamp(0.1, 8.0)) };
    }

    /// Apply a built-in libVLC equalizer preset. `None` clears the equalizer.
    pub fn set_equalizer_preset(&self, index: Option<u32>) -> Result<(), VlcError> {
        let eq = index.map(|i| unsafe { (self.api.equalizer_new_from_preset)(i) });
        let rc = unsafe {
            (self.api.player_set_equalizer)(self.handle, eq.unwrap_or(std::ptr::null_mut()))
        };
        if let Some(eq) = eq {
            // libVLC does not retain the reference; release after the call returns.
            unsafe { (self.api.equalizer_release)(eq) };
        }
        if rc != 0 {
            Err(VlcError::InstanceFailed(format!(
                "set_equalizer returned {rc}"
            )))
        } else {
            Ok(())
        }
    }

    pub fn snapshot(&self, path: &Path) -> Result<(), VlcError> {
        let c = CString::new(vlc_path_string(path).as_bytes())
            .map_err(|_| VlcError::InvalidMediaPath)?;
        let rc = unsafe { (self.api.video_take_snapshot)(self.handle, 0, c.as_ptr(), 0, 0) };
        if rc != 0 {
            Err(VlcError::InstanceFailed(format!("snapshot returned {rc}")))
        } else {
            Ok(())
        }
    }

    pub fn chapter_count(&self) -> i32 {
        unsafe { (self.api.player_get_chapter_count)(self.handle) }
    }

    /// Full chapter list for the current title (`libvlc_media_player_get_full_chapter_descriptions`).
    ///
    /// Returns an empty vector for files without chapters, before playback starts, or when
    /// the title has none — the caller shows "no chapters" rather than an error.
    pub fn chapters(&self) -> Vec<Chapter> {
        let mut list: *mut *mut sys::ChapterDescription = std::ptr::null_mut();
        // SAFETY: live player handle; -1 asks for the current title. libVLC allocates
        // the array and every entry; both are released exactly once below.
        let count =
            unsafe { (self.api.player_full_chapter_descriptions)(self.handle, -1, &mut list) };
        if count <= 0 || list.is_null() {
            return Vec::new();
        }
        let count = count as usize;
        let mut chapters = Vec::with_capacity(count);
        for index in 0..count {
            // SAFETY: `count` entries were written by libVLC.
            let entry = unsafe { *list.add(index) };
            if entry.is_null() {
                continue;
            }
            let entry = unsafe { &*entry };
            let name = if entry.name.is_null() {
                String::new()
            } else {
                // SAFETY: libVLC guarantees a NUL-terminated string for a non-NULL name.
                unsafe { CStr::from_ptr(entry.name) }
                    .to_string_lossy()
                    .into_owned()
            };
            chapters.push(Chapter {
                start_ms: entry.time_offset,
                duration_ms: entry.duration,
                name,
            });
        }
        // SAFETY: pairs with the array returned above.
        unsafe { (self.api.chapter_descriptions_release)(list, count as u32) };
        chapters
    }

    pub fn chapter(&self) -> i32 {
        unsafe { (self.api.player_get_chapter)(self.handle) }
    }
    pub fn set_chapter(&self, n: i32) {
        unsafe { (self.api.player_set_chapter)(self.handle, n) };
    }
}

impl Drop for MediaPlayer {
    fn drop(&mut self) {
        // CRITICAL ORDER: stop the vout, detach events, release the player, then drop
        // Box<FrameSwap>. Reversing this is use-after-free.
        unsafe {
            (self.api.player_stop)(self.handle);
        }
        self.events = None;
        unsafe { (self.api.player_release)(self.handle) };
        *self.frames.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// libVLC rejects absolute Windows paths written with forward slashes, so the
    /// wrapper must normalise them before the FFI call.
    #[test]
    fn absolute_forward_slash_paths_are_normalised() {
        let got = vlc_path_string(Path::new("D:/media/ep.mkv"));
        if cfg!(windows) {
            assert_eq!(got, "D:\\media\\ep.mkv");
        } else {
            assert_eq!(got, "D:/media/ep.mkv");
        }
    }

    #[test]
    fn backslash_paths_are_left_alone() {
        let got = vlc_path_string(Path::new("D:\\media\\ep.mkv"));
        assert_eq!(got, "D:\\media\\ep.mkv");
    }

    #[test]
    fn urls_are_passed_through_untouched() {
        for url in [
            "https://example.com/a/b.mkv",
            "file:///D:/media/ep.mkv",
            "rtsp://10.0.0.1/live",
        ] {
            assert_eq!(vlc_path_string(Path::new(url)), url);
        }
    }

    #[test]
    fn relative_paths_keep_a_single_separator_style() {
        let got = vlc_path_string(Path::new("Show/Ep01.mkv"));
        if cfg!(windows) {
            assert_eq!(got, "Show\\Ep01.mkv");
        } else {
            assert_eq!(got, "Show/Ep01.mkv");
        }
    }
}
