// deny (not forbid): Slint's generated vtable code carries #[allow(unsafe_code)] internally.
#![deny(unsafe_code)]

//! Vedio for me (VFM) — a Windows media player.
//! libVLC 3.0.x runtime loading -> RGBA/RV32 callback double-buffer -> Slint Image with a
//! composited HUD on top (no HWND airspace), plus the `--bench` telemetry harness.

use clap::Parser;
use slint::{
    ComponentHandle, DataTransfer, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode,
    VecModel,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use vfm_core::open_media;
use vfm_core::{
    format_timecode, format_timecode_long, should_resume, CoreEvent, Language, LibraryStore,
    MediaItem, PlayerAction, PlayerStateMachine, Queue, Settings, SettingsStore, SortMode,
    TelemetryAccumulator, ViewMode, WatchProgress,
};
use vlc_wrapper::events::PlayerEvent;
use vlc_wrapper::{swizzle_rgba, Instance, MediaPlayer, PixelFormat};

mod crash;
mod fonts;
mod hotkeys_ui;
mod i18n;
mod layout_ui;
mod library_ui;
mod open_dialog;
mod presets_ui;
mod settings_ui;
mod smtc;
mod strings_ui;
mod subs_search;
mod subtitles_ui;
mod timeline_ui;
mod windows;

slint::include_modules!();

#[derive(Debug, Parser)]
#[command(
    name = "VFM",
    about = "VFM — a Windows media player built on libVLC and Slint"
)]
struct Args {
    /// Local media files or URLs to enqueue (open-with passes several).
    #[arg()]
    media: Vec<String>,
    /// Register HKCU file associations for common media extensions and exit.
    #[arg(long)]
    associate: bool,
    /// Remove VFM file associations and exit.
    #[arg(long)]
    unassociate: bool,
    /// Headless engine test: play each file for N seconds, save progress, then exit.
    #[arg(long)]
    engine_test: Option<u64>,
    /// Directory for library.sqlite and settings.toml.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Allow a second instance (useful for debugging).
    #[arg(long)]
    no_single_instance: bool,
    /// Run for N seconds with 1 Hz telemetry rows, then print a summary and exit.
    #[arg(long)]
    bench: Option<u64>,
    /// Open the hidden design-system component gallery and exit when closed.
    #[arg(long)]
    gallery: bool,
    /// Initial gallery theme: dark or light.
    #[arg(long, default_value = "dark")]
    gallery_theme: String,
    /// Initial gallery language: en or ar.
    #[arg(long, default_value = "en")]
    gallery_language: String,
    /// Pixel format requested from libVLC: rgba (no UI-thread work) or rv32 (BGRA, swizzled).
    #[arg(long, default_value = "rgba")]
    chroma: String,
    /// Open the full-screen library view at startup instead of the player.
    #[arg(long)]
    library: bool,
    /// External subtitle file(s) to load with the first media item.
    #[arg(long = "subtitle")]
    subtitles: Vec<String>,
    /// Open the right adjustment sidebar at startup: 0 subtitles, 1 audio, 2 video,
    /// 3 playback, 4 presets.
    #[arg(long = "sidebar")]
    sidebar: Option<i32>,
    /// Open the OpenSubtitles search dialog at startup (used by the screenshot tool).
    #[arg(long = "subs-search")]
    subs_search: bool,
    /// Open the settings window at startup.
    #[arg(long)]
    settings: bool,
    /// Open the settings window with the shortcut dialog showing.
    #[arg(long)]
    hotkeys: bool,
    /// Show the first-run onboarding screen even if it was already dismissed.
    #[arg(long)]
    onboarding: bool,
    /// Open the open-media dialog at startup, optionally on a panel (0 file … 7 stream).
    #[arg(long, num_args = 0..=1, default_missing_value = "0")]
    open: Option<i32>,
    /// Open the control-bar customisation window at startup.
    #[arg(long)]
    customize: bool,
    /// Source width in pixels (must match the media for the spike).
    #[arg(long, default_value_t = 1920)]
    width: u32,
    /// Source height in pixels.
    #[arg(long, default_value_t = 1080)]
    height: u32,
}

/// Whether the command line names no destination at all, i.e. the user just launched the
/// executable (desktop shortcut, Start menu, `VFM.exe`).
fn opens_library_by_default(args: &Args) -> bool {
    args.media.is_empty()
        && !args.library
        && !args.settings
        && !args.hotkeys
        && !args.onboarding
        && !args.customize
        && args.open.is_none()
        && !args.associate
        && !args.unassociate
        && !args.gallery
}

/// Rebuild the audio/subtitle popovers from libVLC's live track descriptions.
///
/// This is the only place track names are produced: the list length and ids come from the
/// engine, so a file with no subtitle track shows an empty list instead of an invented one.
fn refresh_track_models(
    ui: &PlayerWindow,
    player: &MediaPlayer,
    audio: &Mutex<subtitles_ui::TrackList>,
    subtitles: &Mutex<subtitles_ui::TrackList>,
) {
    let a = subtitles_ui::live_tracks(
        &player.audio_track_descriptions(),
        "Audio",
        player.audio_track(),
    );
    let s = subtitles_ui::live_tracks(
        &player.subtitle_track_descriptions(),
        "Sub",
        player.subtitle_track(),
    );
    *audio.lock().unwrap() = a.clone();
    *subtitles.lock().unwrap() = s.clone();
    subtitles_ui::push_track_names(ui, &a, &s);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Choose the renderer before any Slint type is touched. On machines without a working GL
    // driver the GPU backends pay a context-creation timeout for *every* window, which put the
    // first window 20-27 s out; the software renderer opens in under 2 s. An explicit
    // SLINT_BACKEND from the environment still wins.
    if std::env::var_os("SLINT_BACKEND").is_none() {
        std::env::set_var("SLINT_BACKEND", "winit-software");
    }
    let mut args = Args::parse();
    crash::install();
    if let Some(dir) = &args.data_dir {
        crash::set_dir(dir.clone());
    }
    fonts::register()?;

    if args.gallery {
        return run_gallery(&args);
    }

    let fmt = PixelFormat::parse(&args.chroma)?;

    let exe = std::env::current_exe()?;
    if args.associate || args.unassociate {
        let assoc = windows::Associations::new(&exe);
        let exts = [
            "mp4", "mkv", "avi", "mov", "webm", "m4v", "mp3", "flac", "wav", "ogg", "srt",
        ];
        if args.associate {
            assoc.register(&exts)?;
            println!(
                "VFM file associations registered for {} extensions.",
                exts.len()
            );
        } else {
            assoc.unregister(&exts)?;
            println!("VFM file associations removed.");
        }
        return Ok(());
    }

    // A bare launch (double-clicking the desktop shortcut) opens the library. There is no
    // media to play yet, and refusing to start would leave the user staring at an error
    // instead of the app they just launched.
    if opens_library_by_default(&args) {
        args.library = true;
    }

    let _instance_guard = if args.no_single_instance {
        None
    } else {
        match windows::SingleInstance::acquire() {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("VFM is already running ({e}). Use --no-single-instance to override.");
                return Err(e.into());
            }
        }
    };

    if args.engine_test.is_some() {
        if let Some(guard) = &_instance_guard {
            println!("single-instance lock: {}", guard.lock_path().display());
        }
    }

    let data_dir = args.data_dir.clone().unwrap_or_else(|| {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("VFM")
    });
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!(
            "VFM cannot create its data folder:\n  {}\n  {e}\n\n\
             Pick a writable location with --data-dir.",
            data_dir.display()
        );
        return Err(e.into());
    }
    crash::set_dir(data_dir.clone());
    let settings_path = data_dir.join("settings.toml");
    let settings = SettingsStore::with_file(settings_path.clone())?;
    if crash::diagnostics_enabled(settings.get().diagnostics) {
        eprintln!(
            "VFM diagnostics on — crash log: {}",
            data_dir.join("crash.log").display()
        );
    }
    if let Err(e) = settings.flush() {
        eprintln!(
            "VFM cannot write its settings file:\n  {}\n  {e}\n\n\
             The folder is read-only, on a protected drive, or owned by another user.\n\
             Pick a writable location with --data-dir.",
            settings_path.display()
        );
        return Err(e.into());
    }
    let db_path = data_dir.join("library.sqlite");
    let db = LibraryStore::open(&db_path)?;

    if let Some(seconds) = args.engine_test {
        return engine_test(&args, &settings, &db, seconds, &data_dir);
    }

    // libVLC init: friendly typed error instead of a Windows loader crash popup.
    let instance = Arc::new(match Instance::new() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("VFM cannot start playback:\n  {e}\n\nSet LIBVLC_PATH to a VideoLAN win64 3.0.x directory containing libvlc.dll, libvlccore.dll and plugins/.");
            return Err(Box::new(e));
        }
    });
    println!("VFM plugin path: {}", instance.runtime_dir());

    // Library-first startup has no initial file: create an empty player and attach the
    // video callback pipeline so the first item opened from the library renders correctly.
    let initial: Option<PathBuf> = args.media.first().map(|p| absolutize(Path::new(p)));
    let seed = initial
        .clone()
        .unwrap_or_else(|| PathBuf::from("(library)"));
    // Size the callback buffer to the file's real resolution. The 1920x1080 default only
    // applies to a library-first start, where opening a file re-arms the pipeline anyway.
    let mut startup_size = (args.width, args.height);
    let mut raw_player = if let Some(ref path) = initial {
        let media = instance.media_from_path(path)?;
        // A short wait beats a wrong buffer: the preparse is asynchronous, and if it has not
        // landed yet the vout is created at the default 16:9 size and the file is stretched.
        if let Some(video) = media.tracks_wait(Duration::from_millis(400)).video {
            if video.width > 0 && video.height > 0 {
                startup_size = (video.width, video.height);
            }
        }
        media.player()?
    } else {
        instance.empty_player()?
    };
    raw_player.enable_video_callbacks(startup_size.0, startup_size.1, fmt)?;
    if raw_player.frame_bridge().is_none() {
        return Err("the video pipeline did not start".into());
    }
    let player = Arc::new(raw_player);

    // ---- A-B loop watcher ----
    // While both points are set, bounce the playhead back to A whenever it passes B. The
    // state lives in a static so `swap_media` can clear it on every new file (libVLC's own
    // A-B loop dies with the media, and ours should behave the same).
    {
        let player = Arc::clone(&player);
        std::thread::Builder::new()
            .name("vfm-abloop".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(80));
                let (a, b) = { *AB_LOOP.lock().unwrap() };
                if let (Some(a), Some(b)) = (a, b) {
                    let now = player.time_ms();
                    if now >= b || now < a {
                        player.set_time_ms(a);
                    }
                }
            })
            .ok();
    }

    // The buffer size is not the picture's shape: libVLC scales whatever it decodes into the
    // format we request, so a 4:3 file in a 1920x1080 buffer is already stretched. Until the
    // source size is known the stage must therefore use `contain` (aspect 0), never the
    // buffer's own ratio — otherwise the UI would "correct" a stretch it invented.
    VIDEO_ASPECT.store(0.0f32.to_bits(), Ordering::Relaxed);

    let queue = Arc::new(Mutex::new(Queue::new(0x5eed_1234)));
    {
        let mut q = queue.lock().unwrap();
        for raw in &args.media {
            q.enqueue(MediaItem::new(absolutize(Path::new(raw))));
        }
        if !args.media.is_empty() {
            q.play(0).expect("media list is non-empty");
        }
    }
    let current_path = Arc::new(Mutex::new(seed.clone()));
    let (media_key_tx, media_key_rx) = mpsc::channel();
    let smtc = Arc::new(match smtc::Smtc::attach(media_key_tx) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("SMTC unavailable: {e}");
            None
        }
    });

    let ui = PlayerWindow::new()?;
    // Honour the persisted theme and language instead of always booting dark/English.
    let startup_language = settings.get().language;
    let startup_mode = theme_mode(settings.get().theme);
    strings_ui::apply_player(&ui, startup_language);
    ui.global::<Theme>().set_mode(startup_mode);
    ui.global::<Theme>().set_rtl(i18n::is_rtl(startup_language));
    apply_accent(&ui.global::<Theme>(), parse_colour(&settings.get().accent));
    i18n::set_ui_language(startup_language);
    let ui_language = Arc::new(Mutex::new(startup_language));

    let pip = PipWindow::new()?;
    strings_ui::apply_pip(&pip, startup_language);
    pip.set_filename(filename_label(&seed).into());
    pip.global::<Theme>().set_mode(startup_mode);
    pip.global::<Theme>()
        .set_rtl(i18n::is_rtl(startup_language));
    apply_accent(&pip.global::<Theme>(), parse_colour(&settings.get().accent));
    let pip_open = Arc::new(AtomicBool::new(false));
    let pip_weak = pip.as_weak();

    // ---- Phase 5: subtitle controller + external track discovery ----
    let subtitles = Arc::new(Mutex::new(subtitles_ui::SubtitleController::new()));
    let seek_cache = Arc::new(vfm_core::SeekThumbnailCache::new(
        data_dir.join("seek-thumbs"),
        64,
    ));
    {
        let mut sc = subtitles.lock().unwrap();
        let mut loaded_any = false;
        for sub in &args.subtitles {
            match sc.load(Path::new(sub)) {
                Ok(n) => {
                    println!("VFM loaded {n} subtitle cue(s) from {sub}");
                    loaded_any = true;
                }
                Err(e) => eprintln!("subtitle load failed: {e}"),
            }
        }
        if !loaded_any {
            // Auto-discover a matching external subtitle next to the first media file.
            if let Some(ref path) = initial {
                if let Some(found) = subtitles_ui::discover_subtitles(path).first() {
                    match sc.load(found) {
                        Ok(n) => println!("VFM auto-loaded {n} cue(s) from {}", found.display()),
                        Err(e) => eprintln!("auto subtitle load failed: {e}"),
                    }
                }
            }
        }
        if !sc.session.is_empty() {
            // Disable libVLC's own SPU track so the Slint overlay is the only renderer.
            player.set_subtitle_track(-1);
        }
    }

    // ---- Phase 6: sidebar state and preset resolution for the first item ----
    ui.set_sidebar_state(presets_ui::default_state());
    presets_ui::push_options(&ui, startup_language);

    // Live track lists shared by the popover callbacks and the ticker. Keeping the libVLC ids
    // beside the display names means a popover row selects the real track even when ids are
    // sparse (a container may number tracks 1, 3, 7 rather than 0, 1, 2).
    let audio_track_list: Arc<Mutex<subtitles_ui::TrackList>> =
        Arc::new(Mutex::new(subtitles_ui::TrackList::default()));
    let subtitle_track_list: Arc<Mutex<subtitles_ui::TrackList>> =
        Arc::new(Mutex::new(subtitles_ui::TrackList::default()));

    // Audio output devices: enumerate once in the background (the WASAPI enum can block a
    // moment) and feed the sidebar dropdown. Index 0 is always "System default".
    let audio_devices: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let player = Arc::clone(&player);
        let ui_w = ui.as_weak();
        let devices = Arc::clone(&audio_devices);
        std::thread::Builder::new()
            .name("vfm-audiodev".into())
            .spawn(move || {
                let list = player.audio_output_devices();
                let ids: Vec<String> = list.iter().map(|d| d.id.clone()).collect();
                let names: Vec<slint::SharedString> =
                    list.iter().map(|d| d.name.clone().into()).collect();
                let _ = slint::invoke_from_event_loop(move || {
                    *devices.lock().unwrap() = ids;
                    if let Some(u) = ui_w.upgrade() {
                        let mut options = vec![slint::SharedString::from(i18n::translate(
                            i18n::ui_language(),
                            "audio.device.default",
                        ))];
                        options.extend(names);
                        u.set_audio_device_options(ModelRc::new(VecModel::from(options)));
                    }
                });
            })
            .ok();
    }
    {
        // The search dialog stays inert until a key exists, so the app never calls the
        // API without an explicit opt-in.
        let key = settings.get().opensubtitles_api_key;
        ui.set_subs_key_needed(key.trim().is_empty());
        ui.set_subs_key(key.clone().into());
        if key.trim().is_empty() {
            ui.set_subs_status(i18n::translate(startup_language, "opensubtitles.need-key").into());
        }
    }
    if let Some(tab) = args.sidebar {
        let mut state = ui.get_sidebar_state();
        state.show = true;
        state.tab = tab.clamp(0, 4);
        ui.set_sidebar_state(state);
    }
    if args.subs_search {
        ui.set_subs_dialog_show(true);
    }
    {
        let presets = vfm_core::load_presets(&db).unwrap_or_default();
        presets_ui::push_preset_rows(&ui, &presets, "");
    }
    if let Some(ref path) = initial {
        match presets_ui::apply_for_media(&db, &ui, &player, &subtitles, path) {
            Some(name) => println!("VFM auto-applied preset '{name}' to {}", path.display()),
            None => {
                let mut state = ui.get_sidebar_state();
                state.preset_scope_label = presets_ui::scope_label(
                    &presets_ui::scope_for_media(path),
                    i18n::ui_language(),
                )
                .into();
                ui.set_sidebar_state(state);
            }
        }
    }

    // ---- Phase 4: library window and controller ----
    let lib = LibraryWindow::new()?;
    strings_ui::apply_library(&lib, startup_language);
    lib.global::<Theme>().set_mode(startup_mode);
    lib.global::<Theme>()
        .set_rtl(i18n::is_rtl(startup_language));
    apply_accent(&lib.global::<Theme>(), parse_colour(&settings.get().accent));
    let thumbs = library_ui::ThumbnailCache::new(data_dir.join("thumbs"));
    let _ = thumbs.ensure_dir();
    let lib_controller = Arc::new(Mutex::new(library_ui::LibraryController::new(thumbs)));
    {
        let mut c = lib_controller.lock().unwrap();
        let _ = c.reload(&db);
        library_ui::push_rows(&lib, &c);
    }

    // Helper: rebuild library rows on the UI thread after a data change.
    fn refresh_lib(
        win: &LibraryWindow,
        db: &LibraryStore,
        controller: &Arc<Mutex<library_ui::LibraryController>>,
    ) {
        let mut c = controller.lock().unwrap();
        if let Err(e) = c.reload(db) {
            eprintln!("library reload failed: {e}");
        }
        library_ui::push_rows(win, &c);
    }

    // Section navigation.
    {
        let lib_w = lib.as_weak();
        let controller = Arc::clone(&lib_controller);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        lib.on_sidebar_clicked(move |index| {
            let Some(w) = lib_w.upgrade() else { return };
            controller.lock().unwrap().section = library_ui::Section::from_index(index);
            refresh_lib(&w, &db, &controller);
            if index == 3 {
                w.set_show_network_form(true);
                w.set_input_mode(3);
            } else {
                w.set_show_network_form(false);
            }
        });
    }
    // Search.
    {
        let lib_w = lib.as_weak();
        let controller = Arc::clone(&lib_controller);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        lib.on_search_changed(move |text| {
            let Some(w) = lib_w.upgrade() else { return };
            controller.lock().unwrap().query = text.into();
            refresh_lib(&w, &db, &controller);
        });
    }
    // Sort / view.
    {
        let lib_w = lib.as_weak();
        let controller = Arc::clone(&lib_controller);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        lib.on_sort_changed(move |index| {
            let Some(w) = lib_w.upgrade() else { return };
            let modes = [
                SortMode::NameAsc,
                SortMode::DateNewest,
                SortMode::DateOldest,
            ];
            controller.lock().unwrap().sort = modes
                .get(index as usize)
                .copied()
                .unwrap_or(SortMode::NameAsc);
            refresh_lib(&w, &db, &controller);
        });
        let controller = Arc::clone(&lib_controller);
        lib.on_view_changed(move |mode| {
            controller.lock().unwrap().view = match mode {
                0 => ViewMode::Grid,
                2 => ViewMode::Detail,
                _ => ViewMode::List,
            };
        });
    }
    // One path for "the user chose these files": queue them, play the first, bring the player
    // forward. The native pickers, both drop areas and the open dialog all end up here.
    let play_paths: Rc<dyn Fn(Vec<PathBuf>)> = {
        let instance = Arc::clone(&instance);
        let player = Arc::clone(&player);
        let queue = Arc::clone(&queue);
        let current = Arc::clone(&current_path);
        let smtc = Arc::clone(&smtc);
        let ui_w = ui.as_weak();
        let pip_w = pip_weak.clone();
        let lib_w = lib.as_weak();
        let subtitles = Arc::clone(&subtitles);
        let preset_db = data_dir.join("library.sqlite");
        Rc::new(move |paths: Vec<PathBuf>| {
            let Some(first) = paths.first().cloned() else {
                return;
            };
            {
                let mut q = queue.lock().unwrap();
                q.clear();
                for path in &paths {
                    q.enqueue(MediaItem::new(path));
                }
                let _ = q.play(0);
            }
            if let Err(e) = swap_media(&instance, &player, &first) {
                eprintln!("could not open {}: {e}", first.display());
                return;
            }
            *current.lock().unwrap() = first.clone();
            apply_preset_for_path(&preset_db, &ui_w, &player, &subtitles, &first);
            if let Some(s) = smtc.as_ref() {
                s.set_playing(true);
            }
            let label: slint::SharedString = filename_label(&first).into();
            if let Some(u) = ui_w.upgrade() {
                u.set_filename(label.clone());
                u.set_playing(true);
                u.set_seek_position(0.0);
                library_ui::push_queue_rows(&u, &queue.lock().unwrap());
                u.window().show().ok();
            }
            if let Some(l) = lib_w.upgrade() {
                l.window().hide().ok();
            }
            if let Some(pp) = pip_w.upgrade() {
                pp.set_filename(label);
                pp.set_playing(true);
                pp.set_seek_position(0.0);
            }
        })
    };

    // Extensions offered by the native pickers, taken from the one filter table so the dialog
    // and the drop validation can never disagree.
    let media_extensions: Vec<&'static str> = vfm_core::open_media::OPEN_FILTERS
        .iter()
        .take(4)
        .flat_map(|(_, list)| list.iter().copied())
        .collect();

    // Add folder / file — native pickers. A text box that expects a typed path is not an
    // "open media" feature; this is what the toolbar buttons and Ctrl+O now do.
    {
        let lib_w = lib.as_weak();
        let controller = Arc::clone(&lib_controller);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        lib.on_add_folder(move || {
            let Some(w) = lib_w.upgrade() else { return };
            let Some(dir) = vfm_platform::pick_folder("Add a folder to the library") else {
                return;
            };
            let count = {
                let mut c = controller.lock().unwrap();
                c.scan_folder(&db, &dir, false).unwrap_or_else(|e| {
                    eprintln!("folder scan failed: {e}");
                    0
                })
            };
            println!("VFM added {count} file(s) from {}", dir.display());
            refresh_lib(&w, &db, &controller);
        });
    }
    {
        let play = Rc::clone(&play_paths);
        let extensions = media_extensions.clone();
        lib.on_add_file(move || {
            let picked = vfm_platform::pick_files("Open media", "Media files", &extensions);
            play(picked);
        });
    }
    {
        let lib_w = lib.as_weak();
        lib.on_open_stream(move |text| {
            if let Some(w) = lib_w.upgrade() {
                w.set_show_network_form(true);
                w.set_input_mode(3);
                w.set_network_url(text);
            }
        });
    }
    // Drop onto the library: folders are imported, files are played. Without this the default
    // screen (the library) silently ignores drops, which reads as "drag and drop is missing".
    {
        let play = Rc::clone(&play_paths);
        let controller = Arc::clone(&lib_controller);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        let lib_w = lib.as_weak();
        lib.on_accepts_drop(|data: DataTransfer| data.has_file_paths());
        lib.on_files_dropped(move |data: DataTransfer| {
            let Ok(paths) = data.file_paths() else { return };
            let mut files = Vec::new();
            for path in paths.map(|p| p.to_path_buf()).collect::<Vec<_>>() {
                if !path.is_dir() {
                    files.push(path);
                    continue;
                }
                {
                    let mut c = controller.lock().unwrap();
                    if let Err(e) = c.scan_folder(&db, &path, false) {
                        eprintln!("folder scan failed: {e}");
                    }
                }
                if let Some(w) = lib_w.upgrade() {
                    refresh_lib(&w, &db, &controller);
                }
            }
            play(vfm_core::open_media::collect_media(files));
        });
    }
    // Submit a folder/file/stream path.
    {
        let lib_w = lib.as_weak();
        let controller = Arc::clone(&lib_controller);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        lib.on_submit_path(move |text| {
            let Some(w) = lib_w.upgrade() else { return };
            let text = text.trim();
            if text.is_empty() {
                return;
            }
            let mode = w.get_input_mode();
            let path = PathBuf::from(text);
            if mode == 1 && path.is_dir() {
                {
                    let mut c = controller.lock().unwrap();
                    match c.scan_folder(&db, &path, false) {
                        Ok(n) => println!("VFM scanned {} file(s) from {}", n, path.display()),
                        Err(e) => eprintln!("folder scan failed: {e}"),
                    }
                }
                w.set_show_network_form(false);
                refresh_lib(&w, &db, &controller);
            } else if mode == 2 && path.is_file() {
                if let Some(mut e) = vfm_core::LibraryEntry::new(&path) {
                    e.added = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    if let Err(err) = db.upsert_entry(&e) {
                        eprintln!("add file failed: {err}");
                    }
                }
                w.set_show_network_form(false);
                refresh_lib(&w, &db, &controller);
            } else if mode == 3 {
                for stream in library_ui::stream_entries_from_text(text) {
                    if let Err(e) = db.add_stream(&stream) {
                        eprintln!("add stream failed: {e}");
                    }
                }
                w.set_show_network_form(false);
                controller.lock().unwrap().section = library_ui::Section::Network;
                refresh_lib(&w, &db, &controller);
            } else {
                eprintln!("VFM: path not found or unsupported: {text}");
            }
        });
    }
    // Pin / watched toggles.
    {
        let lib_w = lib.as_weak();
        let controller = Arc::clone(&lib_controller);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        lib.on_pin_row(move |index| {
            let Some(w) = lib_w.upgrade() else { return };
            if let Err(e) = controller.lock().unwrap().toggle_pin(&db, index as usize) {
                eprintln!("pin failed: {e}");
            }
            refresh_lib(&w, &db, &controller);
        });
    }
    // Open a row: play in the shared player, hide library, show player. "Play without
    // leaving the video" — the mini-player keeps playing while the library stays open.
    {
        let lib_w = lib.as_weak();
        let ui_w = ui.as_weak();
        let controller = Arc::clone(&lib_controller);
        let p = Arc::clone(&player);
        let instance_arc = Arc::clone(&instance);
        let queue = Arc::clone(&queue);
        let current = Arc::clone(&current_path);
        let pip_w = pip_weak.clone();
        let sc = Arc::clone(&subtitles);
        let preset_db = db_path.clone();
        lib.on_open_row(move |index| {
            let Some(w) = lib_w.upgrade() else { return };
            let entry = controller.lock().unwrap().entry(index as usize).cloned();
            let Some(entry) = entry else { return };
            if entry.is_stream() {
                let url = entry
                    .path
                    .to_string_lossy()
                    .trim_start_matches("stream:")
                    .to_string();
                let media = match instance_arc.media_from_path(Path::new(&url)) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("open stream failed: {e}");
                        return;
                    }
                };
                p.set_media(&media);
                if let Err(e) = p.play() {
                    eprintln!("open stream play failed: {e}");
                    return;
                }
            } else {
                if let Err(e) = swap_media(&instance_arc, &p, &entry.path) {
                    eprintln!("open media failed: {e}");
                    return;
                }
            }
            {
                let mut q = queue.lock().unwrap();
                if !q.items().iter().any(|m| m.path == entry.path) {
                    q.enqueue(library_ui::item_from_entry(&entry));
                }
                if let Some(i) = q.items().iter().position(|m| m.path == entry.path) {
                    let _ = q.play(i);
                }
            }
            *current.lock().unwrap() = entry.path.clone();
            apply_preset_for_path(&preset_db, &ui_w, &p, &sc, &entry.path);
            let label: slint::SharedString = entry.title.clone().into();
            w.set_now_playing(label.clone());
            w.set_playing(true);
            if let Some(u) = ui_w.upgrade() {
                u.set_filename(label.clone());
                u.set_playing(true);
                u.set_seek_position(0.0);
                library_ui::push_queue_rows(&u, &queue.lock().unwrap());
            }
            if let Some(pp) = pip_w.upgrade() {
                pp.set_filename(label);
                pp.set_playing(true);
            }
        });
    }
    // Add a library row to the transient queue without interrupting playback.
    {
        let ui_w = ui.as_weak();
        let controller = Arc::clone(&lib_controller);
        let queue = Arc::clone(&queue);
        lib.on_add_to_queue(move |index| {
            let entry = controller.lock().unwrap().entry(index as usize).cloned();
            if let Some(e) = entry {
                queue
                    .lock()
                    .unwrap()
                    .enqueue(library_ui::item_from_entry(&e));
                if let Some(u) = ui_w.upgrade() {
                    library_ui::push_queue_rows(&u, &queue.lock().unwrap());
                }
            }
        });
    }
    // Play all rows in the current view.
    {
        let ui_w = ui.as_weak();
        let controller = Arc::clone(&lib_controller);
        let queue = Arc::clone(&queue);
        lib.on_play_all(move || {
            let items: Vec<MediaItem> = controller
                .lock()
                .unwrap()
                .rows
                .iter()
                .filter(|e| !e.is_stream())
                .map(library_ui::item_from_entry)
                .collect();
            let mut q = queue.lock().unwrap();
            q.clear();
            q.enqueue_many(items);
            if let Some(u) = ui_w.upgrade() {
                library_ui::push_queue_rows(&u, &q);
            }
        });
    }
    // Mini-player controls.
    {
        let p = Arc::clone(&player);
        let lib_w = lib.as_weak();
        lib.on_toggle_play(move || {
            p.toggle_pause();
            if let Some(w) = lib_w.upgrade() {
                w.set_playing(p.is_playing());
            }
        });
        let p = Arc::clone(&player);
        lib.on_seek_changed(move |pos| p.set_position(pos));
    }
    // Back / forward use the library controller's section history via a simple stack kept
    // in the widget's section property; delegate to sidebar by cycling for now.
    {
        let lib_w = lib.as_weak();
        lib.on_back(move || {
            if let Some(w) = lib_w.upgrade() {
                let i = (w.get_sidebar_index() - 1).max(0);
                w.set_sidebar_index(i);
            }
        });
        let lib_w = lib.as_weak();
        lib.on_forward(move || {
            if let Some(w) = lib_w.upgrade() {
                w.set_sidebar_index((w.get_sidebar_index() + 1).min(7));
            }
        });
    }
    // Open player / close library.
    {
        let lib_w = lib.as_weak();
        let ui_w = ui.as_weak();
        lib.on_open_player(move || {
            if let (Some(l), Some(u)) = (lib_w.upgrade(), ui_w.upgrade()) {
                l.window().hide().ok();
                u.window().show().ok();
            }
        });
        let lib_w2 = lib.as_weak();
        lib.on_close_library(move || {
            if let Some(l) = lib_w2.upgrade() {
                l.window().hide().ok();
            }
        });
    }
    // Player -> library (Ctrl+L or context menu).
    {
        let lib_w = lib.as_weak();
        let ui_w = ui.as_weak();
        let controller = Arc::clone(&lib_controller);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        ui.on_open_library(move || {
            let Some(l) = lib_w.upgrade() else { return };
            refresh_lib(&l, &db, &controller);
            l.window().show().ok();
            if let Some(u) = ui_w.upgrade() {
                u.window().hide().ok();
            }
        });
    }
    {
        let geometry = windows::sanitize_window(settings.get().last_window.as_ref());
        let window = ui.window();
        window.set_size(slint::PhysicalSize::new(geometry.width, geometry.height));
        window.set_position(slint::PhysicalPosition::new(geometry.x, geometry.y));
        window.set_maximized(geometry.maximized);
    }
    ui.set_filename(filename_label(&seed).into());
    ui.set_resolution(
        format!(
            "{}×{}  {}",
            args.width,
            args.height,
            args.chroma.to_uppercase()
        )
        .into(),
    );
    ui.set_volume_level(player.volume().clamp(0, 100));

    let telemetry = Arc::new(Mutex::new(TelemetryAccumulator::new()));

    // ---- UI callbacks (all actions go through Arc<MediaPlayer>, never from libVLC threads) ----
    {
        let ui_weak = ui.as_weak();
        let p = Arc::clone(&player);
        let smtc = Arc::clone(&smtc);
        let pip_weak = pip_weak.clone();
        ui.on_toggle_play(move || {
            p.toggle_pause();
            let playing = p.is_playing();
            if let Some(s) = smtc.as_ref() {
                s.set_playing(playing);
            }
            if let Some(u) = ui_weak.upgrade() {
                u.set_playing(playing);
            }
            if let Some(pp) = pip_weak.upgrade() {
                pp.set_playing(playing);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let p = Arc::clone(&player);
        let smtc = Arc::clone(&smtc);
        let pip_weak = pip_weak.clone();
        ui.on_stop_playback(move || {
            p.stop();
            if let Some(s) = smtc.as_ref() {
                s.set_playing(false);
            }
            if let Some(u) = ui_weak.upgrade() {
                u.set_playing(false);
                u.set_seek_position(0.0);
            }
            if let Some(pp) = pip_weak.upgrade() {
                pp.set_playing(false);
                pp.set_seek_position(0.0);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_toggle_fullscreen(move || {
            if let Some(u) = ui_weak.upgrade() {
                let window = u.window();
                window.set_fullscreen(!window.is_fullscreen());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let language = Arc::clone(&ui_language);
        let pip_weak = pip_weak.clone();
        ui.on_toggle_rtl(move || {
            let Some(u) = ui_weak.upgrade() else { return };
            let next = {
                let mut current = language.lock().unwrap();
                *current = if *current == Language::En {
                    Language::Ar
                } else {
                    Language::En
                };
                *current
            };
            strings_ui::apply_player(&u, next);
            i18n::set_ui_language(next);
            // The sidebar read-outs carry translated fragments (scope label, filter
            // list), so they have to be rebuilt in the new language.
            let mut state = u.get_sidebar_state();
            presets_ui::refresh_labels(&mut state);
            u.set_sidebar_state(state);
            if let Some(pp) = pip_weak.upgrade() {
                strings_ui::apply_pip(&pp, next);
            }
            let rtl = i18n::is_rtl(next);
            u.global::<Theme>().set_rtl(rtl);
            if let Some(pp) = pip_weak.upgrade() {
                pp.global::<Theme>().set_rtl(rtl);
            }
        });
    }
    {
        let p = Arc::clone(&player);
        ui.on_seek_changed(move |pos| p.set_position(pos));
    }
    {
        let p = Arc::clone(&player);
        let ui_weak = ui.as_weak();
        ui.on_volume_changed(move |v| {
            p.set_volume(v);
            if let Some(u) = ui_weak.upgrade() {
                u.set_volume_level(v);
            }
        });
    }

    {
        let p = Arc::clone(&player);
        let ui_weak = ui.as_weak();
        let cache = Arc::clone(&seek_cache);
        let current = Arc::clone(&current_path);
        ui.on_seek_hover(move |pos| {
            let len = p.length_ms().max(0);
            let time_ms = (pos * len as f32) as i64;
            let tooltip = format_timecode(time_ms);
            if let Some(u) = ui_weak.upgrade() {
                u.set_seek_tooltip(tooltip.into());
                let bucket = vfm_core::bucket_position(pos, len, 32);
                let media = current.lock().unwrap().clone();
                if let Some(thumb) = cache.get(&media, bucket) {
                    if let Ok(img) = slint::Image::load_from_path(&thumb) {
                        u.set_seek_thumbnail(img);
                        u.set_seek_thumbnail_show(true);
                    }
                } else {
                    u.set_seek_thumbnail_show(false);
                }
            }
        });
    }
    {
        let instance = Arc::clone(&instance);
        let p = Arc::clone(&player);
        let queue = Arc::clone(&queue);
        let current = Arc::clone(&current_path);
        let smtc = Arc::clone(&smtc);
        let ui_weak = ui.as_weak();
        let pip_weak = pip_weak.clone();
        let sc = Arc::clone(&subtitles);
        let preset_db = db_path.clone();
        ui.on_next(move || {
            skip_media(
                true, &instance, &p, &queue, &current, &smtc, &ui_weak, &pip_weak, &preset_db, &sc,
            );
        });
    }
    {
        let instance = Arc::clone(&instance);
        let p = Arc::clone(&player);
        let queue = Arc::clone(&queue);
        let current = Arc::clone(&current_path);
        let smtc = Arc::clone(&smtc);
        let ui_weak = ui.as_weak();
        let pip_weak = pip_weak.clone();
        let sc = Arc::clone(&subtitles);
        let preset_db = db_path.clone();
        ui.on_previous(move || {
            skip_media(
                false, &instance, &p, &queue, &current, &smtc, &ui_weak, &pip_weak, &preset_db, &sc,
            );
        });
    }
    {
        let p = Arc::clone(&player);
        let snapshot = data_dir.join("snapshot.png");
        ui.on_record(move || {
            if let Err(e) = p.snapshot(&snapshot) {
                eprintln!("snapshot failed: {e}");
            }
        });
    }
    {
        let p = Arc::clone(&player);
        let ui_weak = ui.as_weak();
        let pip_weak = pip_weak.clone();
        ui.on_toggle_mute(move || {
            let next = if p.volume() == 0 { 80 } else { 0 };
            p.set_volume(next);
            if let Some(u) = ui_weak.upgrade() {
                u.set_volume_level(next);
                u.set_muted(next == 0);
            }
            if let Some(pp) = pip_weak.upgrade() {
                pp.set_playing(p.is_playing());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let pip_weak = pip.as_weak();
        let pip_open = Arc::clone(&pip_open);
        ui.on_toggle_pip(move || {
            let Some(pp) = pip_weak.upgrade() else { return };
            let open = !pip_open.swap(true, Ordering::AcqRel);
            if open {
                if let Some(u) = ui_weak.upgrade() {
                    pp.set_video_frame(u.get_video_frame());
                    pp.set_filename(u.get_filename());
                    pp.set_seek_position(u.get_seek_position());
                    pp.set_playing(u.get_playing());
                }
                if let Err(e) = pp.window().show() {
                    eprintln!("PiP show failed: {e}");
                    pip_open.store(false, Ordering::Release);
                }
            } else {
                let _ = pp.window().hide();
            }
        });
    }
    {
        let pip_weak = pip.as_weak();
        let pip_open = Arc::clone(&pip_open);
        pip.on_close_pip(move || {
            if let Some(pp) = pip_weak.upgrade() {
                let _ = pp.window().hide();
            }
            pip_open.store(false, Ordering::Release);
        });
    }
    {
        let p = Arc::clone(&player);
        let ui_weak = ui.as_weak();
        ui.on_volume_wheeled(move |delta| {
            let Some(u) = ui_weak.upgrade() else { return };
            let step = if delta > 0.0 { 5 } else { -5 };
            let next = (u.get_volume_level() + step).clamp(0, 100);
            p.set_volume(next);
            u.set_volume_level(next);
        });
    }
    {
        let ui_weak = ui.as_weak();
        let close = |u: PlayerWindow| {
            u.set_chrome_hidden(true);
            u.set_chrome_visible(false);
            u.set_context_menu_show(false);
        };
        ui.on_hide_ui(move || {
            if let Some(u) = ui_weak.upgrade() {
                close(u);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let queue = Arc::clone(&queue);
        let queue_for_toggle = Arc::clone(&queue);
        ui.on_toggle_queue(move || {
            if let Some(u) = ui_weak.upgrade() {
                let next = !u.get_queue_show();
                u.set_queue_show(next);
                if next {
                    library_ui::push_queue_rows(&u, &queue_for_toggle.lock().unwrap());
                }
            }
        });
        {
            let ui_weak = ui.as_weak();
            let p = Arc::clone(&player);
            let instance_arc = Arc::clone(&instance);
            let queue = Arc::clone(&queue);
            let current = Arc::clone(&current_path);
            let pip_weak = pip_weak.clone();
            let sc = Arc::clone(&subtitles);
            let preset_db = db_path.clone();
            ui.on_queue_play_at(move |index| {
                let path = {
                    let mut q = queue.lock().unwrap();
                    let i = index as usize;
                    if q.play(i).is_ok() {
                        q.items().get(i).map(|m| m.path.clone())
                    } else {
                        None
                    }
                };
                let Some(path) = path else { return };
                if let Err(e) = swap_media(&instance_arc, &p, &path) {
                    eprintln!("queue play failed: {e}");
                    return;
                }
                *current.lock().unwrap() = path.clone();
                apply_preset_for_path(&preset_db, &ui_weak, &p, &sc, &path);
                let label: slint::SharedString = filename_label(&path).into();
                if let Some(u) = ui_weak.upgrade() {
                    u.set_filename(label.clone());
                    u.set_playing(true);
                    u.set_seek_position(0.0);
                    library_ui::push_queue_rows(&u, &queue.lock().unwrap());
                }
                if let Some(pp) = pip_weak.upgrade() {
                    pp.set_filename(label);
                    pp.set_playing(true);
                }
            });
        }
        {
            let ui_weak = ui.as_weak();
            let queue = Arc::clone(&queue);
            ui.on_queue_remove_at(move |index| {
                let mut q = queue.lock().unwrap();
                let _ = q.remove(index as usize);
                if let Some(u) = ui_weak.upgrade() {
                    library_ui::push_queue_rows(&u, &q);
                }
            });
        }
        {
            let ui_weak = ui.as_weak();
            let queue = Arc::clone(&queue);
            ui.on_queue_clear(move || {
                queue.lock().unwrap().clear();
                if let Some(u) = ui_weak.upgrade() {
                    library_ui::push_queue_rows(&u, &queue.lock().unwrap());
                }
            });
        }
        {
            let ui_weak = ui.as_weak();
            let queue = Arc::clone(&queue);
            ui.on_queue_move(move |from, to| {
                let mut q = queue.lock().unwrap();
                let _ = q.move_item(from as usize, to as usize);
                if let Some(u) = ui_weak.upgrade() {
                    library_ui::push_queue_rows(&u, &q);
                }
            });
        }
        {
            let ui_w = ui.as_weak();
            ui.on_toggle_settings(move || {
                if let Some(u) = ui_w.upgrade() {
                    let mut state = u.get_sidebar_state();
                    state.show = !state.show;
                    if state.show {
                        state.tab = 4;
                    }
                    u.set_sidebar_state(state);
                }
            });
        }
        {
            let ui_w = ui.as_weak();
            ui.on_show_audio(move || {
                if let Some(u) = ui_w.upgrade() {
                    let mut state = u.get_sidebar_state();
                    state.show = true;
                    state.tab = 1;
                    u.set_sidebar_state(state);
                }
            });
        }
        {
            let ui_w = ui.as_weak();
            ui.on_show_video(move || {
                if let Some(u) = ui_w.upgrade() {
                    let mut state = u.get_sidebar_state();
                    state.show = true;
                    state.tab = 2;
                    u.set_sidebar_state(state);
                }
            });
        }
        {
            let ui_w = ui.as_weak();
            ui.on_show_subtitles(move || {
                if let Some(u) = ui_w.upgrade() {
                    let mut state = u.get_sidebar_state();
                    state.show = true;
                    state.tab = 0;
                    u.set_sidebar_state(state);
                }
            });
        }
        {
            let ui_w = ui.as_weak();
            let p = Arc::clone(&player);
            let sc = Arc::clone(&subtitles);
            ui.on_sidebar_setting_changed(move |key, value| {
                let Some(u) = ui_w.upgrade() else { return };
                let mut state = u.get_sidebar_state();
                let payload = presets_ui::snapshot_payload(&state);
                match key.as_str() {
                    "subtitle.delay" => {
                        state.subtitle_delay = value;
                        let updated = presets_ui::snapshot_payload(&state);
                        updated.apply_to_subtitle_session(&mut sc.lock().unwrap().session);
                    }
                    "subtitle.size" | "subtitle.position" | "subtitle.outline" => {
                        let field = match key.as_str() {
                            "subtitle.size" => &mut state.subtitle_size,
                            "subtitle.position" => &mut state.subtitle_position,
                            _ => &mut state.subtitle_outline,
                        };
                        *field = value;
                        let updated = presets_ui::snapshot_payload(&state);
                        updated.apply_to_subtitle_session(&mut sc.lock().unwrap().session);
                    }
                    "audio.delay" => {
                        state.audio_delay = value;
                        p.set_audio_delay_ms(payload.audio.delay_ms);
                    }
                    "video.brightness" | "video.contrast" | "video.saturation" | "video.hue"
                    | "video.gamma" => {
                        match key.as_str() {
                            "video.brightness" => state.brightness = value,
                            "video.contrast" => state.contrast = value,
                            "video.saturation" => state.saturation = value,
                            "video.hue" => state.hue = value,
                            _ => state.gamma = value,
                        }
                        let video = presets_ui::snapshot_payload(&state).video;
                        p.set_video_adjustments(
                            video.contrast,
                            video.brightness,
                            video.hue,
                            video.saturation,
                            video.gamma,
                        );
                    }
                    "video.zoom" => {
                        state.zoom = value;
                        p.set_zoom(presets_ui::snapshot_payload(&state).video.zoom);
                    }
                    "playback.speed" => {
                        state.speed = value;
                        let _ = p.set_rate(presets_ui::snapshot_payload(&state).playback.speed);
                    }
                    "playback.sleep" => state.sleep_minutes = value,
                    _ => {}
                }
                presets_ui::refresh_labels(&mut state);
                u.set_sidebar_state(state);
            });
        }
        {
            let ui_w = ui.as_weak();
            let sc = Arc::clone(&subtitles);
            ui.on_sidebar_toggle_changed(move |key, value| {
                let Some(u) = ui_w.upgrade() else { return };
                let mut state = u.get_sidebar_state();
                let payload = presets_ui::snapshot_payload(&state);
                match key.as_str() {
                    "subtitle.relative" => {
                        state.subtitle_relative = value;
                        state.subtitle_size = if value {
                            presets_ui::size_slider(payload.subtitle.size_scale)
                        } else {
                            presets_ui::precise_slider(payload.subtitle.precise_size_px)
                        };
                        let updated = presets_ui::snapshot_payload(&state);
                        updated.apply_to_subtitle_session(&mut sc.lock().unwrap().session);
                    }
                    "subtitle.timeline" => state.subtitle_timeline = value,
                    "audio.normalize" => state.audio_normalize = value,
                    "audio.surround" => state.audio_surround = value,
                    "video.flip-h" => state.flip_h = value,
                    "video.flip-v" => state.flip_v = value,
                    "playback.waveform" => {
                        state.waveform = value;
                        u.set_waveform_visible(value);
                    }
                    "playback.metadata" => state.metadata = value,
                    _ => {}
                }
                presets_ui::refresh_labels(&mut state);
                store_video_transform(&state);
                u.set_sidebar_state(state);
            });
        }
        {
            let ui_w = ui.as_weak();
            let p = Arc::clone(&player);
            let audio_devices = Arc::clone(&audio_devices);
            ui.on_sidebar_option_changed(move |key, index| {
                let Some(u) = ui_w.upgrade() else { return };
                let mut state = u.get_sidebar_state();
                match key.as_str() {
                    "audio.eq" => {
                        state.eq_index = index;
                        let _ = p.set_equalizer_preset(Some(index.max(0) as u32));
                    }
                    "audio.device" => {
                        state.audio_device_index = index;
                        // Index 0 is "System default"; anything else maps into the
                        // enumerated device ids (which exclude the default entry).
                        if index <= 0 {
                            p.set_audio_output_device(None);
                        } else {
                            let id = audio_devices
                                .lock()
                                .unwrap()
                                .get(index as usize - 1)
                                .cloned();
                            if let Some(id) = id {
                                p.set_audio_output_device(Some(&id));
                            }
                        }
                    }
                    "video.aspect" => {
                        state.aspect_index = index;
                        let video = presets_ui::snapshot_payload(&state).video;
                        let _ = p.set_aspect_ratio(video.aspect_cstr());
                    }
                    "video.rotation" => state.rotation_index = index,
                    _ => {}
                }
                presets_ui::refresh_labels(&mut state);
                store_video_transform(&state);
                u.set_sidebar_state(state);
            });
        }
        {
            let ui_w = ui.as_weak();
            let p = Arc::clone(&player);
            let sc = Arc::clone(&subtitles);
            let path_for_scope = Arc::clone(&current_path);
            let preset_db = db_path.clone();
            ui.on_preset_action(move |action, index| {
                let Some(u) = ui_w.upgrade() else { return };
                let Ok(db) = LibraryStore::open(&preset_db) else {
                    return;
                };
                let mut presets = vfm_core::load_presets(&db).unwrap_or_default();
                let mut state = u.get_sidebar_state();
                match action.as_str() {
                    "select" => {
                        if let Some(preset) = presets.get(index.max(0) as usize) {
                            state.active_preset_index = index;
                            state.preset_name = preset.name.clone().into();
                            state.preset_scope_label =
                                presets_ui::scope_label(&preset.scope, i18n::ui_language()).into();
                        }
                    }
                    "save" => {
                        let name = if state.preset_name.is_empty() {
                            format!("Preset {}", presets.len() + 1)
                        } else {
                            state.preset_name.to_string()
                        };
                        let media = path_for_scope.lock().unwrap().clone();
                        let scope = presets_ui::scope_for_media(&media);
                        let preset = vfm_core::Preset::new(
                            name.clone(),
                            scope.clone(),
                            presets_ui::snapshot_payload(&state),
                        );
                        if let Err(e) = vfm_core::save_preset(&db, &preset) {
                            eprintln!("save preset failed: {e}");
                            return;
                        }
                        presets = vfm_core::load_presets(&db).unwrap_or_default();
                        state.active_preset_index = presets
                            .iter()
                            .position(|item| item.name == name)
                            .map(|i| i as i32)
                            .unwrap_or(-1);
                        state.preset_name = name.clone().into();
                        state.preset_scope_label =
                            presets_ui::scope_label(&scope, i18n::ui_language()).into();
                        presets_ui::push_preset_rows(&u, &presets, &name);
                    }
                    "load" => {
                        if let Some(preset) = presets.get(index.max(0) as usize) {
                            presets_ui::apply_to_player(&p, &sc, &preset.payload);
                            presets_ui::apply_payload_to_state(&mut state, &preset.payload);
                            store_video_transform(&state);
                            state.active_preset_index = index;
                            state.preset_name = preset.name.clone().into();
                            state.preset_scope_label =
                                presets_ui::scope_label(&preset.scope, i18n::ui_language()).into();
                            presets_ui::push_preset_rows(&u, &presets, &preset.name);
                        }
                    }
                    "rename" => {
                        if let Some(old) = presets.get(index.max(0) as usize) {
                            let new = state.preset_name.to_string();
                            if !new.is_empty() && new != old.name {
                                if let Err(e) = vfm_core::rename_preset(&db, &old.name, &new) {
                                    eprintln!("rename preset failed: {e}");
                                    return;
                                }
                                presets = vfm_core::load_presets(&db).unwrap_or_default();
                                presets_ui::push_preset_rows(&u, &presets, &new);
                            }
                        }
                    }
                    "delete" => {
                        if let Some(old) = presets.get(index.max(0) as usize) {
                            if let Err(e) = vfm_core::remove_preset(&db, &old.name) {
                                eprintln!("delete preset failed: {e}");
                                return;
                            }
                            presets = vfm_core::load_presets(&db).unwrap_or_default();
                            state.active_preset_index = -1;
                            state.preset_name = "".into();
                            let media = path_for_scope.lock().unwrap().clone();
                            state.preset_scope_label = presets_ui::scope_label(
                                &presets_ui::scope_for_media(&media),
                                i18n::ui_language(),
                            )
                            .into();
                            presets_ui::push_preset_rows(&u, &presets, "");
                        }
                    }
                    "add-subtitles" => u.invoke_load_subtitles(),
                    // An empty query means "search this exact file by hash"; typing a name
                    // switches to a title search.
                    "search-subtitles" => u.set_subs_dialog_show(true),
                    _ => {}
                }
                u.set_sidebar_state(state);
            });
        }
        // Track selection from the popovers. libVLC track ids map directly to popover rows.
        // ---- Phase 7: chapter list, bookmark list and Ctrl+B shortcut ----
        {
            let ui_w = ui.as_weak();
            let p = Arc::clone(&player);
            let path_now = Arc::clone(&current_path);
            let db_file = db_path.clone();
            ui.on_timeline_action(move |action, index| {
                let Some(u) = ui_w.upgrade() else { return };
                let media = path_now.lock().unwrap().clone();
                let duration = u.get_duration_ms().max(0) as i64;
                match action.as_str() {
                    "chapter" => {
                        if let Some(chapter) = p.chapters().get(index.max(0) as usize) {
                            p.set_time_ms(chapter.start_ms);
                            if duration > 0 {
                                u.set_seek_position(
                                    (chapter.start_ms as f32 / duration as f32).clamp(0.0, 1.0),
                                );
                            }
                        }
                    }
                    "bookmark" => {
                        if let Some(item) =
                            open_bookmarks(&db_file, &media).get(index.max(0) as usize)
                        {
                            p.set_time_ms(item.time_ms);
                            if duration > 0 {
                                u.set_seek_position(
                                    (item.time_ms as f32 / duration as f32).clamp(0.0, 1.0),
                                );
                            }
                        }
                    }
                    "bookmark-add" => {
                        add_bookmark_at(&u, &db_file, &media, p.time_ms(), duration);
                    }
                    "bookmark-remove" => {
                        let items = open_bookmarks(&db_file, &media);
                        if let Some(item) = items.get(index.max(0) as usize) {
                            let Ok(db) = LibraryStore::open(&db_file) else {
                                return;
                            };
                            if let Err(e) = vfm_core::remove_bookmark(&db, item.id) {
                                eprintln!("remove bookmark failed: {e}");
                                return;
                            }
                            let left = vfm_core::list_bookmarks(&db, &media).unwrap_or_default();
                            push_bookmarks(&u, &left, duration);
                            u.set_bookmark_index(-1);
                        }
                    }
                    _ => {}
                }
            });
        }
        {
            let ui_w = ui.as_weak();
            let p = Arc::clone(&player);
            let path_now = Arc::clone(&current_path);
            let db_file = db_path.clone();
            ui.on_bookmark_add(move || {
                let Some(u) = ui_w.upgrade() else { return };
                let media = path_now.lock().unwrap().clone();
                let duration = u.get_duration_ms().max(0) as i64;
                if add_bookmark_at(&u, &db_file, &media, p.time_ms(), duration).is_some() {
                    // Reveal the list so the new tick is visible on the bar and in the panel.
                    let mut state = u.get_sidebar_state();
                    state.show = true;
                    state.tab = 3;
                    u.set_sidebar_state(state);
                }
            });
        }
        {
            let ui_w = ui.as_weak();
            let p = Arc::clone(&player);
            let path_now = Arc::clone(&current_path);
            let db_file = db_path.clone();
            ui.on_bookmark_jump(move |direction| {
                let Some(u) = ui_w.upgrade() else { return };
                let media = path_now.lock().unwrap().clone();
                let duration = u.get_duration_ms().max(0) as i64;
                let items = open_bookmarks(&db_file, &media);
                let target = if direction > 0 {
                    vfm_core::neighbour_bookmark(&items, p.time_ms(), true)
                } else {
                    vfm_core::neighbour_bookmark(&items, p.time_ms(), false)
                };
                let Some(item) = target else { return };
                p.set_time_ms(item.time_ms);
                if duration > 0 {
                    u.set_seek_position((item.time_ms as f32 / duration as f32).clamp(0.0, 1.0));
                }
            });
        }
        // ---- Phase 7: continue-watching answers ----
        {
            let ui_w = ui.as_weak();
            let p = Arc::clone(&player);
            ui.on_continue_resume(move || {
                if let Some(u) = ui_w.upgrade() {
                    u.set_continue_show(false);
                }
                let _ = p.play();
            });
        }
        {
            let ui_w = ui.as_weak();
            let p = Arc::clone(&player);
            ui.on_continue_restart(move || {
                if let Some(u) = ui_w.upgrade() {
                    u.set_continue_show(false);
                    u.set_seek_position(0.0);
                }
                p.set_time_ms(0);
                let _ = p.play();
            });
        }
        // ---- Phase 7: OpenSubtitles search dialog ----
        {
            let ui_w = ui.as_weak();
            let path_now = Arc::clone(&current_path);
            let sc = Arc::clone(&subtitles);
            let store = settings.clone();
            let subs_dir = data_dir.join("subtitles");
            let hits = Arc::new(Mutex::new(Vec::<vfm_core::SubtitleHit>::new()));
            let hits_search = Arc::clone(&hits);
            let hits_download = Arc::clone(&hits);
            ui.on_subs_action(move |action, index| {
                let Some(u) = ui_w.upgrade() else { return };
                match action.as_str() {
                    "close" => {
                        // Persist whatever is in the key field: the dialog is the only place
                        // it can be edited, and closing is the natural commit point.
                        let key = u.get_subs_key().to_string();
                        let _ = store.update(|s| s.opensubtitles_api_key = key.clone());
                        u.set_subs_key_needed(key.trim().is_empty());
                        u.set_subs_dialog_show(false);
                    }
                    "save-key" => {
                        let key = u.get_subs_key().to_string();
                        let _ = store.update(|s| s.opensubtitles_api_key = key.clone());
                        u.set_subs_key_needed(key.trim().is_empty());
                    }
                    "search" => {
                        let key = store.get().opensubtitles_api_key;
                        if key.trim().is_empty() {
                            u.set_subs_status(
                                i18n::translate(i18n::ui_language(), "opensubtitles.need-key")
                                    .into(),
                            );
                            u.set_subs_key_needed(true);
                            return;
                        }
                        let media = path_now.lock().unwrap().clone();
                        u.set_subs_busy(true);
                        u.set_subs_status("…".into());
                        subs_search::search(
                            ui_w.clone(),
                            key,
                            media,
                            u.get_subs_query().to_string(),
                            Arc::clone(&hits_search),
                        );
                    }
                    "download" => {
                        let key = store.get().opensubtitles_api_key;
                        let chosen = hits_download
                            .lock()
                            .unwrap()
                            .get(index.max(0) as usize)
                            .cloned();
                        if let Some(hit) = chosen.filter(|_| !key.trim().is_empty()) {
                            u.set_subs_busy(true);
                            subs_search::download(
                                ui_w.clone(),
                                key,
                                hit,
                                subs_dir.clone(),
                                Arc::clone(&sc),
                            );
                        }
                    }
                    _ => {}
                }
            });
        }
        {
            let ui_w = ui.as_weak();
            let p = Arc::clone(&player);
            let al = Arc::clone(&audio_track_list);
            let sl = Arc::clone(&subtitle_track_list);
            ui.on_select_audio_track(move |row| {
                if let Some(id) = subtitles_ui::track_id_at(&al.lock().unwrap(), row) {
                    p.set_audio_track(id);
                }
                if let Some(u) = ui_w.upgrade() {
                    refresh_track_models(&u, &p, &al, &sl);
                }
            });
        }
        {
            let ui_w = ui.as_weak();
            let p = Arc::clone(&player);
            let sc = Arc::clone(&subtitles);
            let al = Arc::clone(&audio_track_list);
            let sl = Arc::clone(&subtitle_track_list);
            ui.on_select_subtitle_track(move |row| {
                let Some(id) = subtitles_ui::track_id_at(&sl.lock().unwrap(), row) else {
                    return;
                };
                p.set_subtitle_track(id);
                // When libVLC renders the track itself, disable our overlay to avoid doubles.
                sc.lock().unwrap().session.enabled = id < 0;
                if let Some(u) = ui_w.upgrade() {
                    refresh_track_models(&u, &p, &al, &sl);
                }
            });
        }
        // External subtitle loader: discover matching files or clear. Called from popover.
        {
            let ui_w = ui.as_weak();
            let sc = Arc::clone(&subtitles);
            let current = Arc::clone(&current_path);
            ui.on_load_subtitles(move || {
                let path = current.lock().unwrap().clone();
                if path.exists() {
                    let found = subtitles_ui::discover_subtitles(&path);
                    if let Some(f) = found.first() {
                        let mut sc = sc.lock().unwrap();
                        if let Ok(n) = sc.load(f) {
                            sc.session.enabled = true;
                            if let Some(u) = ui_w.upgrade() {
                                u.set_subtitle_text("".into());
                            }
                            println!("VFM loaded {n} cue(s) from {}", f.display());
                        }
                    }
                }
            });
        }
        // Populate initial track names from libVLC's own track descriptions.
        refresh_track_models(&ui, &player, &audio_track_list, &subtitle_track_list);
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_minimize(move || {
            if let Some(u) = ui_weak.upgrade() {
                u.window().set_minimized(true);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_toggle_maximize(move || {
            if let Some(u) = ui_weak.upgrade() {
                let window = u.window();
                window.set_maximized(!window.is_maximized());
            }
        });
    }
    {
        ui.on_quit(move || {
            let _ = slint::quit_event_loop();
        });
    }

    {
        let ui_weak = ui.as_weak();
        let p = Arc::clone(&player);
        let instance_arc = Arc::clone(&instance);
        let queue = Arc::clone(&queue);
        let current = Arc::clone(&current_path);
        let smtc = Arc::clone(&smtc);
        let pip_weak = pip_weak.clone();
        let sc = Arc::clone(&subtitles);
        let preset_db = db_path.clone();
        ui.on_accepts_drop(|data: DataTransfer| data.has_file_paths());
        ui.on_files_dropped(move |data: DataTransfer| {
            let Ok(paths) = data.file_paths() else { return };
            // Drop only what the player can actually open: a stray .txt or a folder of
            // documents should not replace whatever is playing.
            let media: Vec<PathBuf> = vfm_core::open_media::collect_media(
                paths.map(|p| p.to_path_buf()).collect::<Vec<_>>(),
            );
            if media.is_empty() {
                if let Some(u) = ui_weak.upgrade() {
                    u.invoke_osd(
                        "".into(),
                        i18n::translate(i18n::ui_language(), "osd.not-media").into(),
                    );
                }
                return;
            }
            let dropped: Vec<PathBuf> = {
                let mut q = queue.lock().unwrap();
                media
                    .into_iter()
                    .inspect(|path| {
                        q.enqueue(MediaItem::new(path));
                    })
                    .collect()
            };
            let Some(path) = dropped.first() else { return };
            if let Err(e) = swap_media(&instance_arc, &p, path) {
                eprintln!("drop playback failed: {e}");
                return;
            }
            *current.lock().unwrap() = path.clone();
            apply_preset_for_path(&preset_db, &ui_weak, &p, &sc, path);
            if let Some(s) = smtc.as_ref() {
                s.set_playing(true);
            }
            let label: slint::SharedString = filename_label(path).into();
            if let Some(u) = ui_weak.upgrade() {
                u.set_filename(label.clone());
                u.set_playing(true);
                u.set_seek_position(0.0);
            }
            if let Some(pp) = pip_weak.upgrade() {
                pp.set_filename(label);
                pp.set_playing(true);
                pp.set_seek_position(0.0);
            }
        });
    }

    // ---- presenter thread: coalesced frame upload on the Slint UI thread ----
    {
        let ui_weak = ui.as_weak();
        let pip_weak = pip_weak.clone();
        let pip_open = Arc::clone(&pip_open);
        let player = Arc::clone(&player);
        let telemetry = Arc::clone(&telemetry);
        let swizzle = fmt.needs_swizzle();
        let in_flight = Arc::new(AtomicBool::new(false));
        std::thread::Builder::new()
            .name("vfm-presenter".into())
            .spawn(move || loop {
                // Re-read the bridge every tick: opening a file re-arms the pipeline with the
                // media's real dimensions, which swaps in a fresh FrameSwap.
                let Some(frames) = player.frame_bridge() else {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                };
                if frames.has_pending() && !in_flight.swap(true, Ordering::AcqRel) {
                    let ui_weak = ui_weak.clone();
                    let frames = Arc::clone(&frames);
                    let telemetry = Arc::clone(&telemetry);
                    let in_flight = Arc::clone(&in_flight);
                    let pip_weak = pip_weak.clone();
                    let pip_open = Arc::clone(&pip_open);
                    let result = slint::invoke_from_event_loop(move || {
                        let started = Instant::now();
                        // One copy per frame: build the image straight from the decoder's
                        // buffer instead of detaching a reused buffer (`make_mut_slice` had
                        // to memcpy 8 MB again whenever Slint still held the old image).
                        let mut frame = None;
                        // Rotation and mirroring change the pixels, so they are applied while
                        // the frame is copied. The cost is one pass over the buffer and only
                        // when the user actually set a transform.
                        let transform = video_transform();
                        let mut out_size = (0u32, 0u32);
                        let uploaded = frames.with_front(|src| {
                            if swizzle {
                                swizzle_rgba(src);
                            }
                            let (w, h) = (frames.width(), frames.height());
                            if transform.is_identity() {
                                out_size = (w, h);
                                frame = Some(Image::from_rgba8(
                                    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(src, w, h),
                                ));
                            } else {
                                let (ow, oh) = transform.output_size(w, h);
                                out_size = (ow, oh);
                                let mut pixels = vec![0u8; (ow * oh * 4) as usize];
                                transform.apply_into(src, w, h, &mut pixels);
                                frame = Some(Image::from_rgba8(
                                    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                                        &pixels, ow, oh,
                                    ),
                                ));
                            }
                        });
                        let copy_ms = started.elapsed().as_secs_f32() * 1000.0;
                        if uploaded {
                            // Keep the stage's shape in step with the file that is playing. A
                            // quarter turn swaps width and height, so the aspect inverts too.
                            let source = f32::from_bits(VIDEO_ASPECT.load(Ordering::Relaxed));
                            let aspect = if source > 0.0 {
                                if transform.quarters % 2 == 1 {
                                    1.0 / source
                                } else {
                                    source
                                }
                            } else if out_size.1 > 0 {
                                out_size.0 as f32 / out_size.1 as f32
                            } else {
                                0.0
                            };
                            if let Some(frame) = frame {
                                if let Some(u) = ui_weak.upgrade() {
                                    if (u.get_video_aspect() - aspect).abs() > 0.001 {
                                        u.set_video_aspect(aspect);
                                    }
                                    u.set_video_frame(frame.clone());
                                }
                                if pip_open.load(Ordering::Acquire) {
                                    if let Some(pp) = pip_weak.upgrade() {
                                        pp.set_video_frame(frame);
                                    }
                                }
                            }
                            telemetry.lock().unwrap().record_presented(copy_ms);
                        }
                        // Release only after the upload finishes: at most one frame closure
                        // may be queued on the Slint event loop at any instant.
                        in_flight.store(false, Ordering::Release);
                    });
                    if result.is_err() {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(1));
            })?;
    }

    // ---- SMTC / media keys: poll on the UI thread, never act from the WinRT callback ----
    let media_key_timer = Timer::default();
    {
        let ui_weak = ui.as_weak();
        let p = Arc::clone(&player);
        let instance_arc = Arc::clone(&instance);
        let queue = Arc::clone(&queue);
        let current = Arc::clone(&current_path);
        let smtc = Arc::clone(&smtc);
        let pip_weak = pip_weak.clone();
        let sc = Arc::clone(&subtitles);
        let preset_db = db_path.clone();
        media_key_timer.start(TimerMode::Repeated, Duration::from_millis(80), move || {
            while let Ok(key) = media_key_rx.try_recv() {
                match key {
                    smtc::MediaKey::Play => {
                        if p.play().is_ok() {
                            if let Some(s) = smtc.as_ref() {
                                s.set_playing(true);
                            }
                            if let Some(u) = ui_weak.upgrade() {
                                u.set_playing(true);
                            }
                        }
                    }
                    smtc::MediaKey::Pause => {
                        p.pause();
                        if let Some(s) = smtc.as_ref() {
                            s.set_playing(false);
                        }
                        if let Some(u) = ui_weak.upgrade() {
                            u.set_playing(false);
                        }
                    }
                    smtc::MediaKey::Stop => {
                        p.stop();
                        if let Some(s) = smtc.as_ref() {
                            s.set_playing(false);
                        }
                        if let Some(u) = ui_weak.upgrade() {
                            u.set_playing(false);
                            u.set_seek_position(0.0);
                        }
                    }
                    smtc::MediaKey::Next | smtc::MediaKey::Previous => {
                        skip_media(
                            key == smtc::MediaKey::Next,
                            &instance_arc,
                            &p,
                            &queue,
                            &current,
                            &smtc,
                            &ui_weak,
                            &pip_weak,
                            &preset_db,
                            &sc,
                        );
                    }
                }
            }
        });
    }

    // ---- telemetry / UI ticker (4 Hz UI, 1 Hz bench rows) ----
    {
        let ui_weak = ui.as_weak();
        let pip_weak = pip_weak.clone();
        let lib_w = lib.as_weak();
        let p = Arc::clone(&player);
        let telemetry = Arc::clone(&telemetry);
        let subs = Arc::clone(&subtitles);
        let bench = args.bench;
        // Phase 7: the ticker is the single place that notices a media change, so every
        // open path (drop, queue, file dialog, startup) refreshes the timeline the same way.
        let current = Arc::clone(&current_path);
        let instance_arc = Arc::clone(&instance);
        let preset_db = db_path.clone();
        let wave_dir = data_dir.join("waveforms");
        let resume_min_remaining = settings.get().resume_min_remaining_ms;
        let al = Arc::clone(&audio_track_list);
        let sl = Arc::clone(&subtitle_track_list);
        std::thread::Builder::new()
            .name("vfm-telemetry".into())
            .spawn(move || {
                let mut last_decoded = 0u64;
                let mut last_dropped = 0u64;
                let mut tick = 0u32;
                let mut timeline_for: Option<PathBuf> = None;
                let mut timeline_len = 0i64;
                // (target position, when the request was made) for a pending resume pause.
                let mut resume_pause: Option<(i64, Instant)> = None;
                // The bench clock starts when libVLC actually plays, not when this thread
                // starts: window construction can take seconds and used to consume the whole
                // measurement window, reporting 0 decoded frames.
                let mut bench_started: Option<Instant> = None;
                let mut bench_base = vfm_core::TelemetrySnapshot::default();
                let spawned = Instant::now();
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    tick += 1;
                    // Read the live bridge: a new file may have re-armed the pipeline.
                    let (decoded, dropped) = match p.frame_bridge() {
                        Some(current) => (current.decoded(), current.dropped()),
                        None => (0, 0),
                    };
                    {
                        let mut t = telemetry.lock().unwrap();
                        for _ in 0..decoded.saturating_sub(last_decoded) {
                            t.record_decoded();
                        }
                        for _ in 0..dropped.saturating_sub(last_dropped) {
                            t.record_dropped();
                        }
                    }
                    last_decoded = decoded;
                    last_dropped = dropped;

                    let time_ms = p.time_ms();
                    let length_ms = p.length_ms();
                    let t = telemetry.lock().unwrap().snapshot();

                    if let Some((target, since)) = resume_pause {
                        // 3 == libvlc_Playing: the seek has settled, so the pause sticks.
                        if p.state_code() == 3 {
                            p.set_time_ms(target);
                            p.pause();
                            resume_pause = None;
                        } else if since.elapsed() > Duration::from_secs(4) {
                            resume_pause = None;
                        }
                    }

                    // ---- Phase 7: chapters, bookmarks, tags and the waveform ----
                    // Refreshed when the open file changes, and once more when its
                    // duration first becomes known (libVLC reports 0 while opening).
                    'timeline: {
                        let open_now = current.lock().unwrap().clone();
                        let switched = timeline_for.as_deref() != Some(open_now.as_path());
                        let duration_arrived = !switched && length_ms > 0 && timeline_len == 0;
                        if switched || duration_arrived {
                            timeline_for = Some(open_now.clone());
                            timeline_len = length_ms;
                            if open_now.as_os_str().is_empty() {
                                // Nothing open: drop the previous file's chapters,
                                // bookmarks, tags and waveform instead of leaving them
                                // attached to the new (empty) slot.
                                let ui_weak_tl = ui_weak.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(u) = ui_weak_tl.upgrade() {
                                        timeline_ui::clear_timeline(&u);
                                        u.set_music_mode(false);
                                    }
                                });
                                break 'timeline;
                            }
                            let chapters = p.chapters();
                            // Track lists are read from the live player after the demuxer has
                            // exposed them, so opening a new file replaces the previous
                            // file's tracks instead of leaving stale names in the popovers.
                            let a_tracks = subtitles_ui::live_tracks(
                                &p.audio_track_descriptions(),
                                "Audio",
                                p.audio_track(),
                            );
                            let s_tracks = subtitles_ui::live_tracks(
                                &p.subtitle_track_descriptions(),
                                "Sub",
                                p.subtitle_track(),
                            );
                            let bookmarks = open_bookmarks(&preset_db, &open_now);
                            let meta = instance_arc.media_from_path(&open_now).ok().map(|m| {
                                // `metadata()` and `tracks()` both read the preparse result,
                                // which libVLC fills asynchronously: wait for it once, then
                                // both readers see real data instead of empty defaults.
                                let tracks = m.tracks_wait(Duration::from_millis(300));
                                (m.metadata(), tracks)
                            });
                            // Two independent signals: the parsed track list and the live
                            // player. Both must agree before the video stage is replaced,
                            // so a half-parsed file never hides a real video track.
                            let audio_only = meta
                                .as_ref()
                                .map(|(_, tracks)| tracks.video.is_none())
                                .unwrap_or(false)
                                && p.video_track_count() == 0;
                            let chapter_rows =
                                timeline_ui::chapter_rows(&chapters, length_ms, time_ms);
                            let bookmark_rows = timeline_ui::bookmark_rows(&bookmarks, length_ms);
                            let markers = timeline_ui::bookmark_markers(&bookmarks, length_ms);
                            // Saved progress is only offered once, when the file is opened.
                            let resume = if switched {
                                LibraryStore::open(&preset_db)
                                    .ok()
                                    .and_then(|db| db.progress(&open_now).ok().flatten())
                                    .filter(|p| should_resume(p, resume_min_remaining))
                            } else {
                                None
                            };
                            let meta_rows = match &meta {
                                Some((m, tracks)) => {
                                    timeline_ui::metadata_rows(m, tracks, i18n::ui_language())
                                }
                                None => Vec::new(),
                            };
                            // Music mode reads the tags, falling back to the file name.
                            let (music_title, music_sub) = match meta.as_ref() {
                                Some((m, _)) => {
                                    let title = if m.title.trim().is_empty() {
                                        filename_label(&open_now)
                                    } else {
                                        m.title.clone()
                                    };
                                    let sub = [m.artist.clone(), m.album.clone()]
                                        .iter()
                                        .filter(|v| !v.trim().is_empty())
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join(" · ");
                                    (title, sub)
                                }
                                None => (filename_label(&open_now), String::new()),
                            };
                            let ui_weak_tl = ui_weak.clone();
                            let resume_at = resume.as_ref().map(|p| p.position_ms);
                            let resume_label = resume
                                .as_ref()
                                .map(|p| {
                                    format!(
                                        "{} / {}",
                                        format_timecode_long(p.position_ms),
                                        format_timecode_long(p.duration_ms)
                                    )
                                })
                                .unwrap_or_default();
                            let al_for_ui = Arc::clone(&al);
                            let sl_for_ui = Arc::clone(&sl);
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(u) = ui_weak_tl.upgrade() {
                                    timeline_ui::push_chapter_rows(&u, chapter_rows);
                                    timeline_ui::push_bookmark_rows(&u, bookmark_rows);
                                    timeline_ui::push_metadata_rows(&u, meta_rows);
                                    timeline_ui::push_markers(&u, markers);
                                    // Audio-only files always show the waveform: it is the
                                    // only thing on screen during music playback.
                                    let wanted = u.get_sidebar_state().waveform || audio_only;
                                    u.set_waveform_visible(wanted);
                                    u.set_music_mode(audio_only);
                                    u.set_music_title(music_title.into());
                                    u.set_music_subtitle(music_sub.into());
                                    *al_for_ui.lock().unwrap() = a_tracks.clone();
                                    *sl_for_ui.lock().unwrap() = s_tracks.clone();
                                    subtitles_ui::push_track_names(&u, &a_tracks, &s_tracks);
                                    if resume_at.is_some() {
                                        // The playhead is parked at the saved point; wait
                                        // for the answer instead of silently resuming.
                                        u.set_continue_label(resume_label.into());
                                        u.set_continue_show(true);
                                    }
                                }
                            });
                            if let Some(at) = resume_at {
                                // libVLC drops pause requests made while the demuxer is
                                // still opening, so the seek lands now and the pause is
                                // retried on the next tick once the state is Playing.
                                p.set_time_ms(at);
                                resume_pause = Some((at, Instant::now()));
                            }
                            if switched {
                                clear_waveform(&ui_weak);
                                spawn_waveform(ui_weak.clone(), wave_dir.clone(), open_now);
                            }
                        }
                    }

                    // Slint only hands out a strong handle on the UI thread, so marshal the
                    // transport read-out through the event loop instead of upgrading here.
                    let elapsed = format_timecode_long(time_ms);
                    let duration = format_timecode_long(length_ms);
                    let duration_ms = length_ms.clamp(0, i32::MAX as i64) as i32;
                    let position = p.position();
                    let playing = p.is_playing();
                    // Subtitle overlay data (plain values, safe to move to the UI thread).
                    let sub_cue = {
                        let sc = subs.lock().unwrap();
                        sc.session.active_at(time_ms)
                    };
                    let telemetry_text = format!(
                        "{:.1}/{:.1} fps  drop {}  copy {:.2} ms",
                        t.decoded_fps, t.presented_fps, t.dropped_frames, t.avg_copy_ms
                    );
                    let ui_weak_tick = ui_weak.clone();
                    let pip_weak_tick = pip_weak.clone();
                    let lib_weak_tick = lib_w.clone();
                    let elapsed_lib = elapsed.clone();
                    let duration_lib = duration.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(u) = ui_weak_tick.upgrade() {
                            u.set_elapsed(elapsed.into());
                            u.set_duration(duration.into());
                            u.set_duration_ms(duration_ms);
                            u.set_seek_position(position);
                            u.set_playing(playing);
                            u.set_telemetry(telemetry_text.into());
                            if let Some(cue) = sub_cue {
                                let c = cue.style.color;
                                let o = cue.style.outline_color;
                                u.set_subtitle_text(cue.text.into());
                                u.set_subtitle_font_size(
                                    cue.style.scaled_size(28.0).round().max(8.0),
                                );
                                u.set_subtitle_color(slint::Color::from_rgb_u8(c.0, c.1, c.2));
                                u.set_subtitle_outline_color(slint::Color::from_rgb_u8(
                                    o.0, o.1, o.2,
                                ));
                                u.set_subtitle_outline_width(cue.style.outline.max(0.0));
                                u.set_subtitle_position(cue.style.position);
                            } else {
                                u.set_subtitle_text("".into());
                            }
                        }
                        if let Some(pp) = pip_weak_tick.upgrade() {
                            pp.set_seek_position(position);
                            pp.set_playing(playing);
                        }
                        if let Some(l) = lib_weak_tick.upgrade() {
                            l.set_elapsed(elapsed_lib.into());
                            l.set_duration(duration_lib.into());
                            l.set_seek_position(position);
                            l.set_playing(playing);
                        }
                    });
                    if bench_started.is_some() && tick.is_multiple_of(4) {
                        let w = telemetry.lock().unwrap().take_window();
                        println!(
                            "| {:>6.1} | {:>6.1} | {:>8.2} | {:>9.1} | {:>5.2} | {:>5.2} |",
                            w.decoded_fps,
                            w.presented_fps,
                            w.drop_rate_pct,
                            w.avg_copy_ms,
                            w.p95_copy_ms,
                            w.elapsed_secs
                        );
                    }
                    if let Some(secs) = bench {
                        // Start on the first decoded frame: `is_playing` can stay false while
                        // the vout is being created, but a frame is unambiguous evidence.
                        if decoded > 0 && bench_started.is_none() {
                            bench_started = Some(Instant::now());
                            // Everything counted before this instant (startup probes, the
                            // first frame) must not dilute the measured rate.
                            bench_base = telemetry.lock().unwrap().snapshot();
                        }
                        // A bench that never starts playing must not hang forever, and it must
                        // not print a zeroed summary as if it had measured anything.
                        if bench_started.is_none()
                            && spawned.elapsed() > Duration::from_secs(secs.saturating_add(45))
                        {
                            eprintln!(
                                "VFM bench: playback never started after {}s (state={}, time={} ms) — no numbers to report",
                                spawned.elapsed().as_secs(),
                                p.state_code(),
                                p.time_ms()
                            );
                            std::process::exit(2);
                        }
                        let elapsed = bench_started
                            .map(|start| start.elapsed())
                            .unwrap_or_default();
                        if elapsed >= Duration::from_secs(secs) {
                            let s = telemetry.lock().unwrap().snapshot();
                            // Rates are measured over the window that actually played, not
                            // over the process lifetime (startup can take a long time).
                            let measured = elapsed.as_secs_f32().max(1e-3);
                            let decoded = s.decoded_frames.saturating_sub(bench_base.decoded_frames);
                            // The two counters are sampled from different threads, so the
                            // presenter can be one frame ahead of the decoder's tally.
                            let presented = s
                                .presented_frames
                                .saturating_sub(bench_base.presented_frames)
                                .min(decoded);
                            let dropped = s.dropped_frames.saturating_sub(bench_base.dropped_frames);
                            let drop_rate = if decoded + dropped == 0 {
                                0.0
                            } else {
                                100.0 * dropped as f32 / (decoded + dropped) as f32
                            };
                            // One seek after the measured window, so the FPS averages above stay
                            // clean. The latency ends when a *new* frame has been decoded at the
                            // target position: `time_ms` alone can report the request before the
                            // demuxer has landed a keyframe.
                            let target = (p.length_ms() / 2).max(0);
                            let decoded_before =
                                p.frame_bridge().map(|f| f.decoded()).unwrap_or(0);
                            let seek_start = Instant::now();
                            p.set_time_ms(target);
                            let mut seek_ms = None;
                            while seek_start.elapsed() < Duration::from_secs(3) {
                                std::thread::sleep(Duration::from_millis(10));
                                let decoded_now =
                                    p.frame_bridge().map(|f| f.decoded()).unwrap_or(0);
                                if decoded_now > decoded_before
                                    && (p.time_ms() - target).abs() < 500
                                {
                                    seek_ms = Some(seek_start.elapsed().as_millis());
                                    break;
                                }
                            }
                            println!(
                                "\n=== VFM Phase 0 summary ({} chroma, {}s, {}×{}) ===",
                                args.chroma, secs, args.width, args.height
                            );
                            println!("decoded frames : {decoded}");
                            println!("presented      : {presented}");
                            println!("dropped        : {dropped} ({drop_rate:.2}%)");
                            println!("decoded fps    : {:.2}", decoded as f32 / measured);
                            println!("presented fps  : {:.2}", presented as f32 / measured);
                            println!("avg copy (ms)  : {:.3}", s.avg_copy_ms);
                            println!("p95 copy (ms)  : {:.3}", s.p95_copy_ms);
                            match seek_ms {
                                Some(ms) => println!("seek latency   : {ms} ms (to {target} ms)"),
                                None => println!("seek latency   : >3000 ms (to {target} ms)"),
                            }
                            let _ = slint::quit_event_loop();
                            // Detached presenter thread may still be inside invoke_from_event_loop;
                            // exit explicitly after printing the summary in bench mode.
                            std::process::exit(0);
                        }
                    }
                }
            })?;
    }

    // ---- Phase 8: settings window, hotkeys, onboarding ----
    let settings_win = SettingsWindow::new()?;
    let onboarding_win = OnboardingWindow::new()?;
    let settings_ui = Rc::new(settings_ui::SettingsUi::new());
    settings_ui.attach(&settings_win);
    let hotkey_config = Rc::new(RefCell::new(hotkeys_ui::load(&data_dir)));

    // Push everything the player derives from settings. Called after every edit.
    fn apply_live(ui: &PlayerWindow, s: &Settings) {
        ui.set_osd_enabled(s.osd);
        ui.set_waveform_visible(s.waveform);
        ui.set_click_pause(s.click_pause);
        ui.set_double_click_action(if s.double_click == vfm_core::DoubleClickAction::Play {
            1
        } else {
            0
        });
        ui.set_wheel_action(if s.wheel_action == vfm_core::WheelAction::Seek {
            1
        } else {
            0
        });
        ui.set_subtitle_font_size(28.0 * s.subtitle_scale as f32);
        ui.set_subtitle_position(s.subtitle_position as f32);
        ui.set_subtitle_outline_color(if s.subtitle_outline {
            slint::Color::from_rgb_u8(0, 0, 0)
        } else {
            slint::Color::from_argb_u8(0, 0, 0, 0)
        });
    }

    /// "#RRGGBB" -> Slint colour, falling back to the default accent.
    fn parse_colour(hex: &str) -> slint::Color {
        let trimmed = hex.trim().trim_start_matches('#');
        let fallback = slint::Color::from_rgb_u8(0xD3, 0x79, 0x18);
        if trimmed.len() != 6 {
            return fallback;
        }
        let mut parts = [0u8; 3];
        for (index, part) in parts.iter_mut().enumerate() {
            match u8::from_str_radix(&trimmed[index * 2..index * 2 + 2], 16) {
                Ok(value) => *part = value,
                Err(_) => return fallback,
            }
        }
        slint::Color::from_rgb_u8(parts[0], parts[1], parts[2])
    }

    fn theme_mode(theme: vfm_core::Theme) -> i32 {
        match theme {
            vfm_core::Theme::Light => 1,
            vfm_core::Theme::Dark => 0,
            vfm_core::Theme::System => {
                if vfm_platform::system_uses_light_theme() {
                    1
                } else {
                    0
                }
            }
        }
    }

    // Refresh the whole settings window (rows, categories, labels, hotkey list).
    fn refresh_settings(
        win: &SettingsWindow,
        state: &settings_ui::SettingsUi,
        store: &SettingsStore,
        keys: &vfm_core::HotkeyConfig,
    ) {
        let language = i18n::ui_language();
        let s = store.get();
        strings_ui::apply_settings(win, language);
        win.global::<Theme>().set_mode(theme_mode(s.theme));
        apply_accent(&win.global::<Theme>(), parse_colour(&s.accent));
        win.global::<Theme>().set_rtl(i18n::is_rtl(language));
        win.set_browse_label(i18n::translate(language, "settings.browse").into());
        win.set_close_label(i18n::translate(language, "settings.done").into());
        win.set_reset_all_label(i18n::translate(language, "settings.reset-all").into());
        win.set_hotkeys_title(i18n::translate(language, "settings.hotkeys-title").into());
        win.set_hotkeys_hint(i18n::translate(language, "settings.hotkeys-hint").into());
        win.set_hotkey_rows(ModelRc::from(Rc::new(VecModel::from(hotkeys_ui::rows(
            keys, language,
        )))));
        state.refresh_chrome(win, &s, language);
        state.refresh(win, &s, language);
    }

    // Initial paint for both new windows.
    {
        refresh_settings(
            &settings_win,
            &settings_ui,
            &settings,
            &hotkey_config.borrow(),
        );
        apply_live(&ui, &settings.get());
        let language = i18n::ui_language();
        strings_ui::apply_onboarding(&onboarding_win, language);
        onboarding_win
            .global::<Theme>()
            .set_mode(theme_mode(settings.get().theme));
        onboarding_win
            .global::<Theme>()
            .set_rtl(i18n::is_rtl(language));
        let s = settings.get();
        onboarding_win.set_theme_index(match s.theme {
            vfm_core::Theme::System => 0,
            vfm_core::Theme::Dark => 1,
            vfm_core::Theme::Light => 2,
        });
        onboarding_win.set_accent_index(
            settings_ui::ACCENTS
                .iter()
                .position(|a| a.eq_ignore_ascii_case(&s.accent))
                .unwrap_or(0) as i32,
        );
        let swatches: Vec<slint::Color> = settings_ui::ACCENTS
            .iter()
            .map(|a| parse_colour(a))
            .collect();
        onboarding_win.set_accents(ModelRc::from(Rc::new(VecModel::from(swatches.clone()))));
        settings_win.set_accents(ModelRc::from(Rc::new(VecModel::from(swatches))));
        let theme_labels: Vec<slint::SharedString> = ["opt.system", "opt.dark", "opt.light"]
            .iter()
            .map(|k| slint::SharedString::from(i18n::translate(language, k)))
            .collect();
        onboarding_win.set_theme_labels(ModelRc::from(Rc::new(VecModel::from(theme_labels))));
    }

    // Open/close plumbing.
    {
        let sw = settings_win.as_weak();
        let ui_w = ui.as_weak();
        ui.on_toggle_settings(move || {
            if let Some(w) = sw.upgrade() {
                w.show().ok();
            }
            if let Some(u) = ui_w.upgrade() {
                u.invoke_osd(
                    "Ctrl+I".into(),
                    i18n::translate(i18n::ui_language(), "settings.title").into(),
                );
            }
        });
    }
    {
        let sw = settings_win.as_weak();
        settings_win.on_done(move || {
            if let Some(w) = sw.upgrade() {
                w.hide().ok();
            }
        });
    }
    {
        // Sidebar toggle used by Ctrl+E.
        let ui_w = ui.as_weak();
        ui.on_toggle_sidebar(move || {
            if let Some(u) = ui_w.upgrade() {
                let mut state = u.get_sidebar_state();
                state.show = !state.show;
                u.set_sidebar_state(state);
                u.invoke_osd(
                    "Ctrl+E".into(),
                    i18n::translate(i18n::ui_language(), "sidebar.title").into(),
                );
            }
        });
    }

    // ---- settings edits ----
    {
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        let ui_w = ui.as_weak();
        settings_win.on_setting_bool(move |key, value| {
            let Some(w) = sw.upgrade() else { return };
            state.apply_bool(&store, i18n::ui_language(), key.as_str(), value);
            state.refresh_chrome(&w, &store.get(), i18n::ui_language());
            if let Some(u) = ui_w.upgrade() {
                apply_live(&u, &store.get());
            }
        });
    }
    {
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        let ui_w = ui.as_weak();
        let player_ref = Arc::clone(&player);
        settings_win.on_setting_choice(move |key, index| {
            let Some(w) = sw.upgrade() else { return };
            state.apply_choice(&store, i18n::ui_language(), key.as_str(), index);
            state.refresh_chrome(&w, &store.get(), i18n::ui_language());
            let s = store.get();
            if let Some(u) = ui_w.upgrade() {
                apply_live(&u, &s);
                if key == "general.theme" {
                    let mode = theme_mode(s.theme);
                    u.global::<Theme>().set_mode(mode);
                    w.global::<Theme>().set_mode(mode);
                }
                if key == "general.accent" {
                    let colour = parse_colour(&s.accent);
                    apply_accent(&u.global::<Theme>(), colour);
                    apply_accent(&w.global::<Theme>(), colour);
                }
                if key == "video.aspect-ratio" {
                    let ratio = if s.aspect_ratio.is_empty() {
                        None
                    } else {
                        Some(s.aspect_ratio.as_str())
                    };
                    let _ = player_ref.set_aspect_ratio(ratio);
                }
            }
        });
    }
    {
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        let ui_w = ui.as_weak();
        let player_ref = Arc::clone(&player);
        settings_win.on_setting_number(move |key, value| {
            let Some(w) = sw.upgrade() else { return };
            state.apply_number(&store, i18n::ui_language(), key.as_str(), value);
            state.refresh_chrome(&w, &store.get(), i18n::ui_language());
            let s = store.get();
            if let Some(u) = ui_w.upgrade() {
                apply_live(&u, &s);
                match key.as_str() {
                    "playback.volume" => u.invoke_volume_changed(s.volume),
                    "video.zoom" => player_ref.set_zoom(s.zoom as f32),
                    "video.brightness" | "video.contrast" | "video.saturation" => {
                        // (contrast, brightness, hue, saturation, gamma)
                        player_ref.set_video_adjustments(
                            s.contrast as f32,
                            s.brightness as f32,
                            0.0,
                            s.saturation as f32,
                            1.0,
                        );
                    }
                    _ => {}
                }
            }
        });
    }
    {
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        settings_win.on_setting_committed(move |key, value| {
            let Some(w) = sw.upgrade() else { return };
            state.apply_number(&store, i18n::ui_language(), key.as_str(), value);
            state.refresh_chrome(&w, &store.get(), i18n::ui_language());
            state.commit(&store);
        });
    }
    {
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        settings_win.on_setting_text(move |key, text| {
            let Some(w) = sw.upgrade() else { return };
            state.apply_text(&store, i18n::ui_language(), key.as_str(), text.as_str());
            state.refresh_chrome(&w, &store.get(), i18n::ui_language());
        });
    }
    {
        // Native folder picker; falls back to opening the current folder in Explorer only
        // when the user cancels, so the field always ends up with something usable.
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        let data = data_dir.clone();
        settings_win.on_setting_browse(move |key| {
            let Some(w) = sw.upgrade() else { return };
            let s = store.get();
            let raw = s
                .value_of_key(key.as_str())
                .map(|v| v.as_text())
                .unwrap_or_default();
            let start = if raw.trim().is_empty() {
                data.clone()
            } else {
                PathBuf::from(raw.trim())
            };
            if let Some(folder) = vfm_platform::pick_folder("Choose a folder") {
                state.apply_text(
                    &store,
                    i18n::ui_language(),
                    key.as_str(),
                    &folder.display().to_string(),
                );
                state.refresh_chrome(&w, &store.get(), i18n::ui_language());
            } else if start.is_dir() {
                let _ = std::process::Command::new("explorer").arg(&start).spawn();
            }
            w.set_open_help(String::new().into());
        });
    }
    {
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        let ui_w = ui.as_weak();
        settings_win.on_setting_reset(move |key| {
            let Some(w) = sw.upgrade() else { return };
            state.reset(&store, i18n::ui_language(), key.as_str());
            state.refresh_chrome(&w, &store.get(), i18n::ui_language());
            if let Some(u) = ui_w.upgrade() {
                apply_live(&u, &store.get());
            }
        });
    }
    {
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        let ui_w = ui.as_weak();
        settings_win.on_reset_all(move || {
            let Some(w) = sw.upgrade() else { return };
            state.reset_all(&store, i18n::ui_language());
            state.refresh(&w, &store.get(), i18n::ui_language());
            if let Some(u) = ui_w.upgrade() {
                apply_live(&u, &store.get());
            }
        });
    }
    {
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        settings_win.on_category_picked(move |index| {
            let Some(w) = sw.upgrade() else { return };
            state.set_category(&w, &store.get(), i18n::ui_language(), index);
        });
    }
    {
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        settings_win.on_search_changed(move |query| {
            let Some(w) = sw.upgrade() else { return };
            state.set_query(&w, &store.get(), i18n::ui_language(), query.as_str());
        });
    }
    {
        let sw = settings_win.as_weak();
        let state = Rc::clone(&settings_ui);
        let store = settings.clone();
        settings_win.on_advanced_toggled(move || {
            let Some(w) = sw.upgrade() else { return };
            state.toggle_advanced(&w, &store.get(), i18n::ui_language());
        });
    }

    // ---- hotkey dialog ----
    {
        let sw = settings_win.as_weak();
        let keys = Rc::clone(&hotkey_config);
        settings_win.on_hotkeys_open(move || {
            let Some(w) = sw.upgrade() else { return };
            let language = i18n::ui_language();
            w.set_hotkey_rows(ModelRc::from(Rc::new(VecModel::from(hotkeys_ui::rows(
                &keys.borrow(),
                language,
            )))));
            w.set_capture_action(-1);
            w.set_hotkeys_status("".into());
            w.set_hotkeys_conflict(false);
            w.set_hotkeys_show(true);
        });
    }
    {
        let sw = settings_win.as_weak();
        settings_win.on_hotkeys_close(move || {
            if let Some(w) = sw.upgrade() {
                w.set_hotkeys_show(false);
                w.set_capture_action(-1);
            }
        });
    }
    {
        let sw = settings_win.as_weak();
        settings_win.on_hotkey_capture(move |action| {
            if let Some(w) = sw.upgrade() {
                w.set_capture_action(action);
                w.set_capture_text(
                    i18n::translate(i18n::ui_language(), "settings.key-capture").into(),
                );
                w.set_hotkeys_status("".into());
                w.set_hotkeys_conflict(false);
            }
        });
    }
    {
        let sw = settings_win.as_weak();
        let keys = Rc::clone(&hotkey_config);
        let data = data_dir.clone();
        settings_win.on_hotkey_captured(move |text, ctrl, shift, alt| {
            let Some(w) = sw.upgrade() else { return };
            let action_index = w.get_capture_action();
            if action_index < 0 {
                return;
            }
            let Some(action) = hotkeys_ui::action_at(action_index) else {
                return;
            };
            let Some(combo) = hotkeys_ui::combo_from(text.as_str(), ctrl, shift, alt) else {
                return;
            };
            let mut config = keys.borrow().clone();
            match hotkeys_ui::assign(&mut config, action, &combo) {
                Ok(()) => {
                    *keys.borrow_mut() = config.clone();
                    let _ = hotkeys_ui::save(&data, &config);
                    w.set_capture_action(-1);
                    w.set_hotkeys_status(combo.into());
                    w.set_hotkeys_conflict(false);
                }
                Err(reason) => {
                    w.set_hotkeys_status(hotkeys_ui::status_text(
                        i18n::ui_language(),
                        &reason,
                        &combo,
                    ));
                    w.set_hotkeys_conflict(reason == "conflict");
                }
            }
            let language = i18n::ui_language();
            w.set_hotkey_rows(ModelRc::from(Rc::new(VecModel::from(hotkeys_ui::rows(
                &keys.borrow(),
                language,
            )))));
        });
    }
    {
        let sw = settings_win.as_weak();
        let keys = Rc::clone(&hotkey_config);
        let data = data_dir.clone();
        settings_win.on_hotkey_reset(move || {
            let Some(w) = sw.upgrade() else { return };
            let config = vfm_core::HotkeyConfig::default();
            *keys.borrow_mut() = config.clone();
            let _ = hotkeys_ui::save(&data, &config);
            let language = i18n::ui_language();
            w.set_hotkey_rows(ModelRc::from(Rc::new(VecModel::from(hotkeys_ui::rows(
                &config, language,
            )))));
            w.set_hotkeys_status(i18n::translate(language, "settings.reset-keys").into());
            w.set_hotkeys_conflict(false);
        });
    }
    {
        let sw = settings_win.as_weak();
        let keys = Rc::clone(&hotkey_config);
        let data = data_dir.clone();
        settings_win.on_hotkey_export(move || {
            let Some(w) = sw.upgrade() else { return };
            let language = i18n::ui_language();
            let (message, ok) = match hotkeys_ui::export(&data, &keys.borrow()) {
                Ok(path) => (
                    format!(
                        "{}: {}",
                        i18n::translate(language, "settings.export-ok"),
                        path.display()
                    ),
                    false,
                ),
                Err(e) => (e, true),
            };
            w.set_hotkeys_status(message.into());
            w.set_hotkeys_conflict(ok);
        });
    }
    {
        let sw = settings_win.as_weak();
        let keys = Rc::clone(&hotkey_config);
        let data = data_dir.clone();
        settings_win.on_hotkey_import(move || {
            let Some(w) = sw.upgrade() else { return };
            let language = i18n::ui_language();
            let (message, failed) = match hotkeys_ui::import(&data) {
                Ok(config) => {
                    let _ = hotkeys_ui::save(&data, &config);
                    *keys.borrow_mut() = config.clone();
                    let rows =
                        ModelRc::from(Rc::new(VecModel::from(hotkeys_ui::rows(&config, language))));
                    w.set_hotkey_rows(rows);
                    (
                        i18n::translate(language, "settings.import-ok").to_string(),
                        false,
                    )
                }
                Err(_) => (
                    i18n::translate(language, "settings.import-fail").to_string(),
                    true,
                ),
            };
            w.set_hotkeys_status(message.into());
            w.set_hotkeys_conflict(failed);
        });
    }

    // Filled once the open-media window exists; the hotkey handler is registered first.
    let open_win_slot: Rc<RefCell<Option<slint::Weak<OpenMediaWindow>>>> =
        Rc::new(RefCell::new(None));

    // ---- hotkey dispatch ----
    {
        let p = Arc::clone(&player);
        let store = settings.clone();
        let keys = Rc::clone(&hotkey_config);
        let ui_w = ui.as_weak();
        let open_slot = Rc::clone(&open_win_slot);
        ui.on_hotkey(move |text, ctrl, shift, alt| {
            let Some(u) = ui_w.upgrade() else {
                return false;
            };
            let Some(combo) = hotkeys_ui::combo_from(text.as_str(), ctrl, shift, alt) else {
                return false;
            };
            let action = keys
                .borrow()
                .bindings
                .iter()
                .find(|b| vfm_core::hotkeys::normalize_combo(&b.key) == combo)
                .map(|b| b.action);
            let Some(action) = action else { return false };

            let language = i18n::ui_language();
            let s = store.get();
            let mut hint = hotkeys_ui::label_key(action);
            match action {
                vfm_core::Action::TogglePlay => u.invoke_toggle_play(),
                vfm_core::Action::Play => {
                    let _ = p.play();
                }
                vfm_core::Action::Pause => p.pause(),
                vfm_core::Action::Stop => u.invoke_stop_playback(),
                vfm_core::Action::Next => u.invoke_next(),
                vfm_core::Action::Previous => u.invoke_previous(),
                vfm_core::Action::VolumeUp => {
                    u.invoke_volume_changed((p.volume() + s.volume_step).min(125))
                }
                vfm_core::Action::VolumeDown => {
                    u.invoke_volume_changed((p.volume() - s.volume_step).max(0))
                }
                vfm_core::Action::Mute => u.invoke_toggle_mute(),
                vfm_core::Action::SeekForward | vfm_core::Action::SeekBackward => {
                    let delta = if action == vfm_core::Action::SeekForward {
                        s.seek_step_ms
                    } else {
                        -s.seek_step_ms
                    };
                    let target = (p.time_ms() + delta).clamp(0, p.length_ms().max(0));
                    p.set_time_ms(target);
                    u.set_seek_position(if p.length_ms() > 0 {
                        target as f32 / p.length_ms() as f32
                    } else {
                        0.0
                    });
                }
                vfm_core::Action::Fullscreen => u.invoke_toggle_fullscreen(),
                vfm_core::Action::PictureInPicture => u.invoke_toggle_pip(),
                vfm_core::Action::ToggleQueue => u.invoke_toggle_queue(),
                vfm_core::Action::ToggleLibrary => u.invoke_open_library(),
                vfm_core::Action::ToggleSettings => u.invoke_toggle_settings(),
                vfm_core::Action::ToggleSidebar => u.invoke_toggle_sidebar(),
                vfm_core::Action::Subtitles => u.invoke_show_subtitles(),
                vfm_core::Action::Audio => u.invoke_show_audio(),
                vfm_core::Action::Video => u.invoke_show_video(),
                vfm_core::Action::Bookmark => u.invoke_bookmark_add(),
                vfm_core::Action::BookmarkNext => u.invoke_bookmark_jump(1),
                vfm_core::Action::BookmarkPrevious => u.invoke_bookmark_jump(-1),
                vfm_core::Action::FrameStep => p.next_frame(),
                vfm_core::Action::Snapshot => u.invoke_record(),
                vfm_core::Action::Repeat => {
                    let next = match s.repeat {
                        vfm_core::RepeatMode::Off => vfm_core::RepeatMode::All,
                        vfm_core::RepeatMode::All => vfm_core::RepeatMode::One,
                        vfm_core::RepeatMode::One => vfm_core::RepeatMode::Off,
                    };
                    let _ = store.update(|st| st.repeat = next);
                    hint = match next {
                        vfm_core::RepeatMode::Off => "opt.off",
                        vfm_core::RepeatMode::All => "opt.all",
                        vfm_core::RepeatMode::One => "opt.one",
                    };
                }
                vfm_core::Action::Shuffle => {
                    let next = !s.shuffle;
                    let _ = store.update(|st| st.shuffle = next);
                    hint = if next { "settings.on" } else { "settings.off" };
                }
                vfm_core::Action::HideUi => u.invoke_hide_ui(),
                vfm_core::Action::Quit => u.invoke_quit(),
                vfm_core::Action::OpenFile
                | vfm_core::Action::OpenFolder
                | vfm_core::Action::OpenNetwork => {
                    let mode = match action {
                        vfm_core::Action::OpenFolder => open_dialog::MODE_FOLDER,
                        vfm_core::Action::OpenNetwork => open_dialog::MODE_NETWORK,
                        _ => open_dialog::MODE_FILE,
                    };
                    if let Some(weak) = open_slot.borrow().as_ref() {
                        if let Some(w) = weak.upgrade() {
                            w.set_mode(mode);
                            w.show().ok();
                            return true;
                        }
                    }
                    u.invoke_open_library();
                }
                vfm_core::Action::Search | vfm_core::Action::Back | vfm_core::Action::Forward => {
                    u.invoke_open_library();
                }
                vfm_core::Action::AbLoop => {
                    if p.length_ms() <= 0 {
                        hint = "ab.error";
                    } else {
                        let now = p.time_ms().max(0);
                        let mut ab = AB_LOOP.lock().unwrap();
                        match ab.0 {
                            None => {
                                *ab = (Some(now), None);
                                hint = "ab.set-a";
                            }
                            Some(a) if ab.1.is_none() => {
                                // One press on the same spot would loop nothing; nudge B so a
                                // double press always gives a visible repeat.
                                let b = if now > a + 400 { now } else { a + 5000 };
                                *ab = (Some(a), Some(b.min(p.length_ms().max(a + 500))));
                                hint = "ab.set-b";
                            }
                            _ => {
                                *ab = (None, None);
                                hint = "ab.clear";
                            }
                        }
                    }
                }
                vfm_core::Action::RateUp
                | vfm_core::Action::RateDown
                | vfm_core::Action::RateReset => {
                    let current = p.rate();
                    let next = match action {
                        vfm_core::Action::RateUp => (current * 1.25).min(4.0),
                        vfm_core::Action::RateDown => (current / 1.25).max(0.25),
                        _ => 1.0,
                    };
                    let _ = p.set_rate(next);
                    let mut st = u.get_sidebar_state();
                    st.speed = presets_ui::speed_slider(next);
                    presets_ui::refresh_labels(&mut st);
                    u.set_sidebar_state(st);
                    // Show the actual value rather than the action name.
                    u.invoke_osd(combo.as_str().into(), format!("{next:.2}×").into());
                    return true;
                }
                vfm_core::Action::ZoomCycle => {
                    let zooms = [1.0f32, 1.5, 2.0, 2.5, 0.5];
                    let current = presets_ui::zoom(u.get_sidebar_state().zoom);
                    let position = zooms
                        .iter()
                        .position(|z| (*z - current).abs() < 0.06)
                        .map(|i| (i + 1) % zooms.len())
                        .unwrap_or(0);
                    let next = zooms[position];
                    let mut st = u.get_sidebar_state();
                    st.zoom = presets_ui::zoom_slider(next);
                    presets_ui::refresh_labels(&mut st);
                    u.set_sidebar_state(st);
                    p.set_zoom(next);
                    u.invoke_osd(combo.as_str().into(), format!("{next:.1}×").into());
                    return true;
                }
            }
            u.invoke_osd(
                combo.as_str().into(),
                i18n::translate(language, hint).into(),
            );
            true
        });
    }

    // ---- onboarding ----
    {
        let ow = onboarding_win.as_weak();
        let store = settings.clone();
        let ui_w = ui.as_weak();
        onboarding_win.on_theme_picked(move |index| {
            let Some(w) = ow.upgrade() else { return };
            w.set_theme_index(index);
            let theme = match index {
                1 => vfm_core::Theme::Dark,
                2 => vfm_core::Theme::Light,
                _ => vfm_core::Theme::System,
            };
            let _ = store.update(|s| s.theme = theme);
            let mode = theme_mode(theme);
            w.global::<Theme>().set_mode(mode);
            if let Some(u) = ui_w.upgrade() {
                u.global::<Theme>().set_mode(mode);
            }
        });
    }
    {
        let ow = onboarding_win.as_weak();
        let store = settings.clone();
        let ui_w = ui.as_weak();
        onboarding_win.on_accent_picked(move |index| {
            let Some(w) = ow.upgrade() else { return };
            let Some(accent) = settings_ui::ACCENTS.get(index.max(0) as usize) else {
                return;
            };
            w.set_accent_index(index);
            let _ = store.update(|s| s.accent = (*accent).to_string());
            let colour = parse_colour(accent);
            apply_accent(&w.global::<Theme>(), colour);
            if let Some(u) = ui_w.upgrade() {
                apply_accent(&u.global::<Theme>(), colour);
            }
        });
    }
    {
        let ow = onboarding_win.as_weak();
        let store = settings.clone();
        onboarding_win.on_start(move || {
            let _ = store.update(|s| s.onboarding_done = true);
            if let Some(w) = ow.upgrade() {
                w.hide().ok();
            }
        });
    }
    {
        let ow = onboarding_win.as_weak();
        let store = settings.clone();
        onboarding_win.on_skip(move || {
            let _ = store.update(|s| s.onboarding_done = true);
            if let Some(w) = ow.upgrade() {
                w.hide().ok();
            }
        });
    }

    // ---- Phase 10: control-bar layout and profiles ----
    let layout_db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
    let layout_state = Rc::new(RefCell::new(layout_ui::LayoutUi::load(
        &settings, &layout_db,
    )));
    let customize_win = CustomizeWindow::new()?;
    {
        let language = i18n::ui_language();
        customize_win
            .global::<Theme>()
            .set_mode(theme_mode(settings.get().theme));
        customize_win
            .global::<Theme>()
            .set_rtl(i18n::is_rtl(language));
        apply_accent(
            &customize_win.global::<Theme>(),
            parse_colour(&settings.get().accent),
        );
        strings_ui::apply_customize(&customize_win, language);
        layout_ui::push(&customize_win, &layout_state.borrow(), language);
        layout_ui::status(&customize_win, language, "customize.saved", false);
        customize_win.set_status(String::new().into());
    }
    // The player renders the trailing cluster from the saved layout: only the four controls
    // the shipped bar can draw take a slot, but their order is the user's.
    fn apply_bar_layout(ui: &PlayerWindow, layout: &vfm_core::layout::Layout) {
        use vfm_core::layout::ControlId;
        let known = [
            ControlId::Audio,
            ControlId::Subtitles,
            ControlId::Settings,
            ControlId::Playlist,
        ];
        let (rows, count) = layout.trailing_slots(&known);
        let slot = |control: ControlId| {
            rows.iter()
                .find(|(candidate, _)| *candidate == control)
                .map(|(_, slot)| *slot)
                .unwrap_or(-1)
        };
        ui.set_bar_audio_slot(slot(ControlId::Audio));
        ui.set_bar_subtitle_slot(slot(ControlId::Subtitles));
        ui.set_bar_settings_slot(slot(ControlId::Settings));
        ui.set_bar_queue_slot(slot(ControlId::Playlist));
        ui.set_bar_tail_count(count);
    }
    apply_bar_layout(&ui, &layout_state.borrow().layout);

    // Re-push the window and the player after every edit.
    let refresh_layout = {
        let win = customize_win.as_weak();
        let ui_w = ui.as_weak();
        let state = Rc::clone(&layout_state);
        let store = settings.clone();
        Rc::new(move |message: &str, error: bool| {
            let language = i18n::ui_language();
            state.borrow().persist(&store);
            if let Some(u) = ui_w.upgrade() {
                apply_bar_layout(&u, &state.borrow().layout);
            }
            if let Some(w) = win.upgrade() {
                layout_ui::push(&w, &state.borrow(), language);
                layout_ui::status(&w, language, message, error);
            }
        })
    };
    {
        let state = Rc::clone(&layout_state);
        let refresh = Rc::clone(&refresh_layout);
        customize_win.on_add(move |id| {
            state.borrow_mut().add(id.as_str());
            refresh("customize.saved", false);
        });
    }
    {
        let state = Rc::clone(&layout_state);
        let refresh = Rc::clone(&refresh_layout);
        customize_win.on_remove(move |id| {
            state.borrow_mut().remove(id.as_str());
            refresh("customize.saved", false);
        });
    }
    {
        let state = Rc::clone(&layout_state);
        let refresh = Rc::clone(&refresh_layout);
        customize_win.on_move_up(move |id| {
            state.borrow_mut().move_up(id.as_str());
            refresh("customize.saved", false);
        });
    }
    {
        let state = Rc::clone(&layout_state);
        let refresh = Rc::clone(&refresh_layout);
        customize_win.on_move_down(move |id| {
            state.borrow_mut().move_down(id.as_str());
            refresh("customize.saved", false);
        });
    }
    {
        let state = Rc::clone(&layout_state);
        let refresh = Rc::clone(&refresh_layout);
        customize_win.on_reset(move || {
            state.borrow_mut().reset();
            refresh("customize.saved", false);
        });
    }
    {
        let state = Rc::clone(&layout_state);
        let refresh = Rc::clone(&refresh_layout);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        customize_win.on_save_profile(move |_| {
            let name = format!(
                "{} {}",
                i18n::translate(i18n::ui_language(), "customize.profiles"),
                state.borrow().profiles.len() + 1
            );
            let ok = state.borrow_mut().save_profile(&db, &name);
            refresh(
                if ok {
                    "customize.saved"
                } else {
                    "customize.import-fail"
                },
                !ok,
            );
        });
    }
    {
        let state = Rc::clone(&layout_state);
        let refresh = Rc::clone(&refresh_layout);
        customize_win.on_load_profile(move |index| {
            if state.borrow_mut().apply_profile(index.max(0) as usize) {
                refresh("customize.saved", false);
            }
        });
    }
    {
        let state = Rc::clone(&layout_state);
        let refresh = Rc::clone(&refresh_layout);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        customize_win.on_delete_profile(move |index| {
            let ok = state
                .borrow_mut()
                .delete_profile(&db, index.max(0) as usize);
            refresh(
                if ok {
                    "customize.deleted"
                } else {
                    "customize.import-fail"
                },
                !ok,
            );
        });
    }
    {
        let state = Rc::clone(&layout_state);
        let refresh = Rc::clone(&refresh_layout);
        let folder = data_dir.join("layouts");
        customize_win.on_export_profile(move |index| {
            let ok = state
                .borrow()
                .export_profile(index.max(0) as usize, &folder)
                .is_some();
            refresh(
                if ok {
                    "customize.saved"
                } else {
                    "customize.import-fail"
                },
                !ok,
            );
        });
    }
    {
        let state = Rc::clone(&layout_state);
        let refresh = Rc::clone(&refresh_layout);
        let db = LibraryStore::open(&data_dir.join("library.sqlite"))?;
        customize_win.on_import_profile(move || {
            let picked =
                vfm_platform::pick_files("Import layout profile", "Layout profile", &["json"]);
            let ok = picked
                .first()
                .map(|path| state.borrow_mut().import_profile(&db, path))
                .unwrap_or(false);
            refresh(
                if ok {
                    "customize.imported"
                } else {
                    "customize.import-fail"
                },
                !ok,
            );
        });
    }
    {
        let cw = customize_win.as_weak();
        customize_win.on_dismissed(move || {
            if let Some(w) = cw.upgrade() {
                w.hide().ok();
            }
        });
    }
    {
        let cw = customize_win.as_weak();
        ui.on_customize(move || {
            if let Some(w) = cw.upgrade() {
                w.show().ok();
            }
        });
    }
    if args.customize {
        customize_win.show().ok();
    }

    // ---- Phase 9: open-media dialog, conversion and streaming ----
    let open_win = OpenMediaWindow::new()?;
    *open_win_slot.borrow_mut() = Some(open_win.as_weak());
    let open_state = Rc::new(RefCell::new(open_dialog::OpenDialogState::new()));
    {
        let language = i18n::ui_language();
        open_win
            .global::<Theme>()
            .set_mode(theme_mode(settings.get().theme));
        open_win.global::<Theme>().set_rtl(i18n::is_rtl(language));
        apply_accent(
            &open_win.global::<Theme>(),
            parse_colour(&settings.get().accent),
        );
        strings_ui::apply_open(&open_win, language);
        open_dialog::push(&open_win, &open_state.borrow(), language);
        open_dialog::status(&open_win, language, "open.status.ready", false);
        if let Ok(db) = LibraryStore::open(&data_dir.join("library.sqlite")) {
            let entries = db.recent_entries(24).unwrap_or_default();
            open_dialog::push_recent(&open_win, &entries);
        }
    }

    // Right-click menu / toolbar "Open media": raise the real open dialog on the File panel.
    {
        let ow = open_win.as_weak();
        ui.on_open_media_dialog(move || {
            if let Some(w) = ow.upgrade() {
                w.set_mode(open_dialog::MODE_FILE);
                w.show().ok();
            }
        });
    }

    // Open a file, folder or location in the shared player.
    let do_open: Rc<dyn Fn(open_media::Target) -> Result<(), String>> = {
        let instance = Arc::clone(&instance);
        let player = Arc::clone(&player);
        let ui_w = ui.as_weak();
        let pip_w = pip_weak.clone();
        let queue = Arc::clone(&queue);
        let current = Arc::clone(&current_path);
        let controller = Arc::clone(&lib_controller);
        let db_path = data_dir.join("library.sqlite");
        let preset_db = data_dir.join("library.sqlite");
        let subtitles = Arc::clone(&subtitles);
        Rc::new(move |target: open_media::Target| -> Result<(), String> {
            let label_of = |path: &Path| -> String {
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string())
            };
            match target {
                open_media::Target::File(path) if path.is_dir() => {
                    let db = LibraryStore::open(&db_path).map_err(|e| e.to_string())?;
                    let count = controller
                        .lock()
                        .unwrap()
                        .scan_folder(&db, &path, false)
                        .map_err(|e| e.to_string())?;
                    if count == 0 {
                        return Err("no playable file in that folder".to_string());
                    }
                    let rows: Vec<vfm_core::LibraryEntry> = controller
                        .lock()
                        .unwrap()
                        .rows
                        .iter()
                        .filter(|e| !e.is_stream())
                        .cloned()
                        .collect();
                    if let Some(first) = rows.first() {
                        let mut q = queue.lock().unwrap();
                        q.clear();
                        let items: Vec<MediaItem> =
                            rows.iter().map(library_ui::item_from_entry).collect();
                        q.enqueue_many(items);
                        let _ = q.play(0);
                        let media = instance
                            .media_from_path(&first.path)
                            .map_err(|e| e.to_string())?;
                        player.set_media(&media);
                        player.play().map_err(|e| e.to_string())?;
                        *current.lock().unwrap() = first.path.clone();
                        apply_preset_for_path(&preset_db, &ui_w, &player, &subtitles, &first.path);
                        let label = first.title.clone();
                        if let Some(u) = ui_w.upgrade() {
                            u.set_filename(label.clone().into());
                            u.set_playing(true);
                            u.set_seek_position(0.0);
                            library_ui::push_queue_rows(&u, &q);
                            u.window().show().ok();
                        }
                        if let Some(pp) = pip_w.upgrade() {
                            pp.set_filename(label.into());
                            pp.set_playing(true);
                        }
                    }
                    Ok(())
                }
                open_media::Target::File(path) => {
                    // Through `swap_media`, not raw set_media+play: it re-arms the callback
                    // pipeline to this file's real size, so opening a 4:3 clip from the dialog
                    // does not inherit the previous file's 16:9 buffer.
                    swap_media(&instance, &player, &path).map_err(|e| e.to_string())?;
                    *current.lock().unwrap() = path.clone();
                    apply_preset_for_path(&preset_db, &ui_w, &player, &subtitles, &path);
                    let label = label_of(&path);
                    if let Some(u) = ui_w.upgrade() {
                        u.set_filename(label.clone().into());
                        u.set_playing(true);
                        u.set_seek_position(0.0);
                        u.window().show().ok();
                    }
                    if let Some(pp) = pip_w.upgrade() {
                        pp.set_filename(label.into());
                        pp.set_playing(true);
                    }
                    Ok(())
                }
                open_media::Target::Location(url) => {
                    let media = instance
                        .media_from_location(&url)
                        .map_err(|e| e.to_string())?;
                    player.set_media(&media);
                    player.play().map_err(|e| e.to_string())?;
                    *current.lock().unwrap() = PathBuf::from(format!("stream:{url}"));
                    let label = url.clone();
                    if let Some(u) = ui_w.upgrade() {
                        u.set_filename(label.clone().into());
                        u.set_playing(true);
                        u.set_seek_position(0.0);
                        u.window().show().ok();
                    }
                    if let Some(pp) = pip_w.upgrade() {
                        pp.set_filename(label.into());
                        pp.set_playing(true);
                    }
                    Ok(())
                }
            }
        })
    };

    // A conversion or stream job owns its own player so the main one keeps playing.
    struct SideJob {
        player: MediaPlayer,
        converting: bool,
        destination: Option<PathBuf>,
    }
    let side_job: Rc<RefCell<Option<SideJob>>> = Rc::new(RefCell::new(None));

    {
        let state = Rc::clone(&open_state);
        let ow = open_win.as_weak();
        open_win.on_mode_picked(move |mode| {
            let Some(w) = ow.upgrade() else { return };
            state.borrow_mut().set_mode(mode);
            let language = i18n::ui_language();
            open_dialog::push(&w, &state.borrow(), language);
        });
    }
    {
        let state = Rc::clone(&open_state);
        let ow = open_win.as_weak();
        open_win.on_path_edited(move |text| {
            if let Some(w) = ow.upgrade() {
                state.borrow_mut().path_text = text.to_string();
                open_dialog::push(&w, &state.borrow(), i18n::ui_language());
            }
        });
    }
    {
        let state = Rc::clone(&open_state);
        open_win.on_url_edited(move |text| state.borrow_mut().url_text = text.to_string());
    }
    {
        let state = Rc::clone(&open_state);
        open_win.on_capture_video_edited(move |text| {
            state.borrow_mut().capture_video = text.to_string()
        });
    }
    {
        let state = Rc::clone(&open_state);
        open_win.on_capture_audio_edited(move |text| {
            state.borrow_mut().capture_audio = text.to_string()
        });
    }
    {
        let state = Rc::clone(&open_state);
        open_win
            .on_capture_size_edited(move |text| state.borrow_mut().capture_size = text.to_string());
    }
    {
        let state = Rc::clone(&open_state);
        open_win
            .on_capture_fps_edited(move |text| state.borrow_mut().capture_fps = text.to_string());
    }
    {
        let state = Rc::clone(&open_state);
        open_win
            .on_destination_edited(move |text| state.borrow_mut().destination = text.to_string());
    }
    {
        let state = Rc::clone(&open_state);
        let ow = open_win.as_weak();
        open_win.on_stream_port_edited(move |text| {
            state.borrow_mut().stream_port = text.to_string();
            if let Some(w) = ow.upgrade() {
                open_dialog::push(&w, &state.borrow(), i18n::ui_language());
            }
        });
    }
    {
        let state = Rc::clone(&open_state);
        let ow = open_win.as_weak();
        open_win.on_stream_path_edited(move |text| {
            state.borrow_mut().stream_path = text.to_string();
            if let Some(w) = ow.upgrade() {
                open_dialog::push(&w, &state.borrow(), i18n::ui_language());
            }
        });
    }
    {
        let state = Rc::clone(&open_state);
        let ow = open_win.as_weak();
        open_win.on_preset_picked(move |index| {
            let Some(w) = ow.upgrade() else { return };
            state.borrow_mut().set_preset(index);
            open_dialog::push(&w, &state.borrow(), i18n::ui_language());
        });
    }
    {
        let state = Rc::clone(&open_state);
        let ow = open_win.as_weak();
        open_win.on_browse_path(move || {
            let Some(w) = ow.upgrade() else { return };
            let (label, extensions) = open_media::OPEN_FILTERS
                .first()
                .map(|(label, exts)| (*label, exts.to_vec()))
                .unwrap_or(("Video files", Vec::new()));
            let picked = vfm_platform::pick_files("Open media", label, &extensions);
            if let Some(first) = picked.first() {
                let mut s = state.borrow_mut();
                s.path_text = first.display().to_string();
                let language = i18n::ui_language();
                drop(s);
                open_dialog::push(&w, &state.borrow(), language);
            }
        });
    }
    {
        let state = Rc::clone(&open_state);
        let ow = open_win.as_weak();
        open_win.on_browse_folder(move || {
            let Some(w) = ow.upgrade() else { return };
            if let Some(folder) = vfm_platform::pick_folder("Choose a folder") {
                state.borrow_mut().path_text = folder.display().to_string();
                open_dialog::push(&w, &state.borrow(), i18n::ui_language());
            }
        });
    }
    {
        let state = Rc::clone(&open_state);
        let ow = open_win.as_weak();
        open_win.on_browse_destination(move || {
            let Some(w) = ow.upgrade() else { return };
            if let Some(folder) = vfm_platform::pick_folder("Save converted file in") {
                let current = state.borrow().destination.clone();
                let name = Path::new(&current)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "converted.mp4".to_string());
                state.borrow_mut().destination = folder.join(name).display().to_string();
                open_dialog::push(&w, &state.borrow(), i18n::ui_language());
            }
        });
    }
    {
        let state = Rc::clone(&open_state);
        let ow = open_win.as_weak();
        open_win.on_use_clipboard(move || {
            let Some(w) = ow.upgrade() else { return };
            let text = vfm_platform::clipboard_text().unwrap_or_default();
            let mut s = state.borrow_mut();
            s.clipboard_text = text.clone();
            if s.mode == open_dialog::MODE_CLIPBOARD {
                s.url_text = text;
            }
            drop(s);
            open_dialog::push(&w, &state.borrow(), i18n::ui_language());
        });
    }
    {
        let ow = open_win.as_weak();
        let do_open = Rc::clone(&do_open);
        let db_path = data_dir.join("library.sqlite");
        open_win.on_recent_picked(move |index| {
            let Some(w) = ow.upgrade() else { return };
            let Ok(db) = LibraryStore::open(&db_path) else {
                return;
            };
            let entries = db.recent_entries(24).unwrap_or_default();
            let Some(entry) = entries.get(index.max(0) as usize) else {
                return;
            };
            if let Err(error) = do_open(open_media::Target::File(entry.path.clone())) {
                eprintln!("open recent failed: {error}");
                open_dialog::status(&w, i18n::ui_language(), "open.status.failed", true);
                return;
            }
            w.hide().ok();
        });
    }
    {
        let ow = open_win.as_weak();
        let state = Rc::clone(&open_state);
        let do_open = Rc::clone(&do_open);
        let instance = Arc::clone(&instance);
        let side_job = Rc::clone(&side_job);
        open_win.on_action(move || {
            let Some(w) = ow.upgrade() else { return };
            let language = i18n::ui_language();
            let mode = state.borrow().mode;
            if mode == open_dialog::MODE_CONVERT || mode == open_dialog::MODE_STREAM {
                if let Some(job) = side_job.borrow().as_ref() {
                    job.player.stop();
                }
                let (sout, converting, destination) = if mode == open_dialog::MODE_CONVERT {
                    match state.borrow().convert_plan() {
                        Ok((destination, sout)) => (sout, true, Some(destination)),
                        Err(failure) => {
                            open_dialog::status(&w, language, failure.key(), true);
                            return;
                        }
                    }
                } else {
                    match state.borrow().stream_plan() {
                        Ok((_, sout, url)) => {
                            w.set_stream_url(url.into());
                            (sout, false, None)
                        }
                        Err(failure) => {
                            open_dialog::status(&w, language, failure.key(), true);
                            return;
                        }
                    }
                };
                let source = state.borrow().source.clone();
                let Some(source) = source else {
                    open_dialog::status(&w, language, "open.status.no-source", true);
                    return;
                };
                let media = match instance.media_from_path(&source) {
                    Ok(media) => media,
                    Err(error) => {
                        eprintln!("side job media failed: {error}");
                        open_dialog::status(&w, language, "open.status.failed", true);
                        return;
                    }
                };
                let _ = media.add_option(&format!("sout={sout}"));
                // Keep libVLC from opening a second video window for the transcode.
                let _ = media.add_option("vout=dummy");
                let _ = media.add_option("aout=dummy");
                let job_player = match media.player() {
                    Ok(player) => player,
                    Err(error) => {
                        eprintln!("side job player failed: {error}");
                        open_dialog::status(&w, language, "open.status.failed", true);
                        return;
                    }
                };
                if let Err(error) = job_player.play() {
                    eprintln!("side job play failed: {error}");
                    open_dialog::status(&w, language, "open.status.failed", true);
                    return;
                }
                *side_job.borrow_mut() = Some(SideJob {
                    player: job_player,
                    converting,
                    destination,
                });
                w.set_busy(true);
                w.set_progress(0.0);
                open_dialog::status(
                    &w,
                    language,
                    if converting {
                        "open.status.converting"
                    } else {
                        "open.status.streaming"
                    },
                    false,
                );
                return;
            }
            let target = match state.borrow().target() {
                Ok(target) => target,
                Err(failure) => {
                    open_dialog::status(&w, language, failure.key(), true);
                    return;
                }
            };
            // The convert/stream panels need the current media as their source.
            if let open_media::Target::File(path) = &target {
                state.borrow_mut().source = Some(path.clone());
            }
            match do_open(target) {
                Ok(()) => {
                    open_dialog::status(&w, language, "open.status.opening", false);
                    w.hide().ok();
                }
                Err(error) => {
                    eprintln!("open failed: {error}");
                    open_dialog::status(&w, language, "open.status.failed", true);
                }
            }
        });
    }
    {
        let side_job = Rc::clone(&side_job);
        let ow = open_win.as_weak();
        open_win.on_stop(move || {
            let Some(w) = ow.upgrade() else { return };
            if let Some(job) = side_job.borrow_mut().take() {
                job.player.stop();
            }
            w.set_busy(false);
            w.set_progress(0.0);
            open_dialog::status(&w, i18n::ui_language(), "open.status.stopped", false);
        });
    }
    {
        let ow = open_win.as_weak();
        open_win.on_dismissed(move || {
            if let Some(w) = ow.upgrade() {
                w.hide().ok();
            }
        });
    }

    // Progress pump for the side job (conversion or streaming).
    {
        let side_job = Rc::clone(&side_job);
        let ow = open_win.as_weak();
        let progress_timer = Timer::default();
        progress_timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            let Some(w) = ow.upgrade() else { return };
            let mut guard = side_job.borrow_mut();
            let Some(job) = guard.as_mut() else { return };
            let total = job.player.length_ms();
            let position = job.player.time_ms();
            if total > 0 {
                let fraction = (position as f32 / total as f32).clamp(0.0, 1.0);
                w.set_progress(fraction);
            }
            let state_code = job.player.state_code();
            let finished = state_code == 6 || state_code == 7 || (total > 0 && position >= total);
            if finished {
                let converting = job.converting;
                let destination = job.destination.clone();
                job.player.stop();
                *guard = None;
                drop(guard);
                w.set_busy(false);
                w.set_progress(if converting { 1.0 } else { 0.0 });
                let language = i18n::ui_language();
                if state_code == 7 {
                    open_dialog::status(&w, language, "open.status.failed", true);
                } else if converting {
                    let message = match &destination {
                        Some(path) => format!(
                            "{}: {}",
                            i18n::translate(language, "open.status.done"),
                            path.display()
                        ),
                        None => i18n::translate(language, "open.status.done").to_string(),
                    };
                    w.set_status(message.into());
                    w.set_status_error(false);
                } else {
                    open_dialog::status(&w, language, "open.status.done", false);
                }
            }
        });
    }

    if let Some(mode) = args.open {
        let mut s = open_state.borrow_mut();
        s.set_mode(mode);
        let current = current_path.lock().unwrap().clone();
        s.source = if current.is_file() {
            Some(current)
        } else {
            None
        };
        drop(s);
        open_dialog::push(&open_win, &open_state.borrow(), i18n::ui_language());
        open_win.show().ok();
    }

    if args.settings || args.hotkeys {
        settings_win.show().ok();
        if args.hotkeys {
            settings_win.set_hotkeys_show(true);
            let language = i18n::ui_language();
            settings_win.set_hotkey_rows(ModelRc::from(Rc::new(VecModel::from(hotkeys_ui::rows(
                &hotkey_config.borrow(),
                language,
            )))));
        }
    }
    if !settings.get().onboarding_done || args.onboarding {
        onboarding_win.show().ok();
    }

    if args.library {
        // Library-first: hide the player chrome, show the library window.
        ui.window().hide().ok();
        lib.window().show().ok();
    } else if args.media.is_empty() {
        // Settings/onboarding-only launch: nothing to play, so leave the player idle and
        // keep it out of the way.
        ui.window().hide().ok();
    } else {
        player.play()?;
        let generation = OPEN_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(path) = initial.clone() {
            align_video_to_source(Arc::clone(&instance), Arc::clone(&player), path, generation);
        }
    }
    ui.run()?;
    {
        let window = ui.window();
        let position = window.position();
        let size = window.size();
        let state = vfm_core::WindowState {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            maximized: window.is_maximized(),
        };
        settings.update(|s| s.last_window = Some(state))?;
    }
    if settings.get().remember_position {
        let pos = player.time_ms();
        let len = player.length_ms();
        let played_path = current_path.lock().unwrap().clone();
        let mut progress = WatchProgress::new(&played_path, pos, len);
        progress.update_watched();
        db.save_progress(&progress)?;
    }
    player.stop();
    Ok(())
}

/// Hidden design-system gallery. It never touches libVLC, so it can run on any host.
fn run_gallery(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let ui = GalleryWindow::new()?;
    let initial_language = if args.gallery_language.eq_ignore_ascii_case("ar") {
        Language::Ar
    } else {
        Language::En
    };
    let initial_mode = if args.gallery_theme.eq_ignore_ascii_case("light") {
        1
    } else {
        0
    };
    ui.global::<Theme>().set_mode(initial_mode);
    ui.global::<Theme>().set_rtl(i18n::is_rtl(initial_language));
    apply_accent(&ui.global::<Theme>(), accent_color(0));
    strings_ui::apply_gallery(&ui, initial_language);

    let language = Arc::new(Mutex::new(initial_language));
    let accent_index = Arc::new(Mutex::new(0usize));

    {
        ui.on_close_gallery(move || {
            let _ = slint::quit_event_loop();
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_toggle_theme(move || {
            let Some(u) = ui_weak.upgrade() else { return };
            let next = if u.global::<Theme>().get_mode() == 0 {
                1
            } else {
                0
            };
            u.global::<Theme>().set_mode(next);
        });
    }
    {
        let ui_weak = ui.as_weak();
        let language = Arc::clone(&language);
        ui.on_toggle_language(move || {
            let Some(u) = ui_weak.upgrade() else { return };
            let next = {
                let mut current = language.lock().unwrap();
                *current = if *current == Language::En {
                    Language::Ar
                } else {
                    Language::En
                };
                *current
            };
            strings_ui::apply_gallery(&u, next);
            u.global::<Theme>().set_rtl(i18n::is_rtl(next));
        });
    }
    {
        let ui_weak = ui.as_weak();
        let accent_index = Arc::clone(&accent_index);
        ui.on_cycle_accent(move || {
            let Some(u) = ui_weak.upgrade() else { return };
            let next = {
                let mut index = accent_index.lock().unwrap();
                *index = (*index + 1) % 5;
                *index
            };
            apply_accent(&u.global::<Theme>(), accent_color(next));
        });
    }

    ui.run()?;
    Ok(())
}

/// Ink colour that stays readable on top of `accent`.
///
/// The design uses near-black on the warm default accent; saturated accents (blue, red,
/// violet) need white instead, so the choice is made from relative luminance and only falls
/// back to black when it wins by a clear margin.
fn on_accent(colour: slint::Color) -> slint::Color {
    fn channel(value: u8) -> f32 {
        let v = value as f32 / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    let luminance = 0.2126 * channel(colour.red())
        + 0.7152 * channel(colour.green())
        + 0.0722 * channel(colour.blue());
    let contrast_black = (luminance + 0.05) / 0.05;
    let contrast_white = 1.05 / (luminance + 0.05);
    if contrast_black > contrast_white * 1.6 {
        slint::Color::from_rgb_u8(0x0B, 0x0B, 0x0B)
    } else {
        slint::Color::from_rgb_u8(0xFF, 0xFF, 0xFF)
    }
}

/// Set the accent everywhere it is derived from: the swatch plus its readable ink.
fn apply_accent(theme: &Theme<'_>, colour: slint::Color) {
    theme.set_accent(colour);
    theme.set_text_on_accent(on_accent(colour));
}

fn accent_color(index: usize) -> slint::Color {
    match index % 5 {
        0 => slint::Color::from_argb_u8(255, 0xD3, 0x79, 0x18),
        1 => slint::Color::from_argb_u8(255, 0x2E, 0x7D, 0xD1),
        2 => slint::Color::from_argb_u8(255, 0x18, 0x9E, 0x63),
        3 => slint::Color::from_argb_u8(255, 0x8E, 0x44, 0xAD),
        _ => slint::Color::from_argb_u8(255, 0xC0, 0x39, 0x2B),
    }
}

/// Re-resolve presets whenever the media changes. Called from every swap path so a folder
/// preset applies to the next episode without user interaction.
fn apply_preset_for_path(
    db_path: &Path,
    ui: &slint::Weak<PlayerWindow>,
    player: &MediaPlayer,
    subtitles: &Arc<Mutex<subtitles_ui::SubtitleController>>,
    path: &Path,
) {
    let Some(u) = ui.upgrade() else { return };
    let Ok(db) = LibraryStore::open(db_path) else {
        return;
    };
    let language = i18n::ui_language();
    match presets_ui::apply_for_media(&db, &u, player, subtitles, path) {
        Some(name) => println!("VFM auto-applied preset '{name}' to {}", path.display()),
        None => {
            // No preset matched: the scope label still has to show where a new
            // preset for this episode would land.
            let mut state = u.get_sidebar_state();
            state.preset_scope_label =
                presets_ui::scope_label(&presets_ui::scope_for_media(path), language).into();
            u.set_sidebar_state(state);
        }
    }
}

/// Resolve a user-supplied media path to an absolute one.
///
/// Paths typed on the command line may be relative, but library rows, watch
/// progress and folder-scoped presets are all keyed by absolute path. Keeping
/// one representation means "open the next episode" finds the same preset
/// scope no matter how the file was opened.
/// `Path::canonicalize` returns a verbatim `\\?\D:\...` path on Windows, and libVLC cannot open
/// those: the input ends immediately with zero decoded frames. Hand the engine a plain drive
/// path instead (UNC shares keep their `\\server\share` form).
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    match path.to_str() {
        Some(text) if text.starts_with(r"\\?\UNC\") => PathBuf::from(format!(r"\\{}", &text[8..])),
        Some(text) if text.starts_with(r"\\?\") => PathBuf::from(&text[4..]),
        _ => path,
    }
}

fn absolutize(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        path.canonicalize()
            .unwrap_or_else(|_| match std::env::current_dir() {
                Ok(cwd) => cwd.join(path),
                Err(_) => path.to_path_buf(),
            })
    };
    without_verbatim_prefix(absolute)
}

// ---- Phase 7: bookmark helpers shared by the Ctrl+B shortcut and the sidebar list ----

fn open_bookmarks(db_path: &Path, media: &Path) -> Vec<vfm_core::Bookmark> {
    LibraryStore::open(db_path)
        .ok()
        .and_then(|db| vfm_core::list_bookmarks(&db, media).ok())
        .unwrap_or_default()
}

/// Repaint the bookmark list and the ticks over the seek bar.
fn push_bookmarks(ui: &PlayerWindow, items: &[vfm_core::Bookmark], duration_ms: i64) {
    timeline_ui::push_bookmark_rows(ui, timeline_ui::bookmark_rows(items, duration_ms));
    timeline_ui::push_markers(ui, timeline_ui::bookmark_markers(items, duration_ms));
}

/// Drop a bookmark at `time_ms` and return the refreshed list. An empty label falls back
/// to the timecode, so the list never shows blank rows.
fn add_bookmark_at(
    ui: &PlayerWindow,
    db_path: &Path,
    media: &Path,
    time_ms: i64,
    duration_ms: i64,
) -> Option<Vec<vfm_core::Bookmark>> {
    let db = LibraryStore::open(db_path).ok()?;
    let saved = match vfm_core::add_bookmark(&db, media, time_ms, "") {
        Ok(saved) => saved,
        Err(e) => {
            eprintln!("add bookmark failed: {e}");
            return None;
        }
    };
    let items = vfm_core::list_bookmarks(&db, media).unwrap_or_default();
    push_bookmarks(ui, &items, duration_ms);
    ui.set_bookmark_index(timeline_ui::index_of(&items, saved.id));
    Some(items)
}

/// Blank the waveform strip while the next file is being analysed, so the previous
/// file's shape never shows under the new one.
fn clear_waveform(ui: &slint::Weak<PlayerWindow>) {
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(u) = ui.upgrade() {
            u.set_waveform_peaks(slint::ModelRc::new(
                slint::VecModel::from(Vec::<f32>::new()),
            ));
        }
    });
}

/// Analyse the waveform off the UI thread and push the resulting path when it is ready.
/// Analysis decodes the whole audio track, so it must never run on a tick.
fn spawn_waveform(ui: slint::Weak<PlayerWindow>, dir: PathBuf, media: PathBuf) {
    let _ = std::thread::Builder::new()
        .name("vfm-waveform".into())
        .spawn(move || {
            let cache = vfm_core::WaveformCache::new(dir);
            let peaks = cache
                .get_or_compute(&media, vfm_core::DEFAULT_WAVEFORM_BUCKETS)
                .map(|waveform| {
                    timeline_ui::waveform_peaks_for(&waveform, i18n::is_rtl(i18n::ui_language()))
                })
                .unwrap_or_default();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = ui.upgrade() {
                    timeline_ui::push_waveform(&u, peaks);
                }
            });
        });
}

fn filename_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[allow(clippy::too_many_arguments)]
fn skip_media(
    next: bool,
    instance: &Arc<Instance>,
    player: &Arc<MediaPlayer>,
    queue: &Arc<Mutex<Queue>>,
    current: &Arc<Mutex<PathBuf>>,
    smtc: &Arc<Option<smtc::Smtc>>,
    ui: &slint::Weak<PlayerWindow>,
    pip: &slint::Weak<PipWindow>,
    db_path: &Path,
    subs: &Arc<Mutex<subtitles_ui::SubtitleController>>,
) {
    let path = {
        let mut q = queue.lock().unwrap();
        let index = if next { q.advance() } else { q.previous() };
        index.and_then(|i| q.items().get(i).map(|item| item.path.clone()))
    };
    let Some(path) = path else { return };
    if let Err(e) = swap_media(instance, player, &path) {
        eprintln!("media skip failed: {e}");
        return;
    }
    *current.lock().unwrap() = path.clone();
    apply_preset_for_path(db_path, ui, player, subs, &path);
    if let Some(s) = smtc.as_ref() {
        s.set_playing(true);
    }
    let label: slint::SharedString = filename_label(&path).into();
    if let Some(u) = ui.upgrade() {
        u.set_filename(label.clone());
        u.set_playing(true);
        u.set_seek_position(0.0);
    }
    if let Some(pp) = pip.upgrade() {
        pp.set_filename(label);
        pp.set_playing(true);
        pp.set_seek_position(0.0);
    }
}

/// Bumped for every open so a slow probe from a previous file cannot re-arm the pipeline
/// underneath the one the user just asked for.
static OPEN_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Wait for libVLC to report the decoded size, then make the pipeline match it.
///
/// The size is unknown before playback: `libvlc_media_parse` is asynchronous in 3.0, so a
/// freshly created `Media` handle reports no tracks however long we wait on it. The handle the
/// *player* owns is demuxed by then, and `libvlc_video_get_size` only echoes the format we
/// requested — so `player.media_tracks()` is the one honest source of truth.
///
/// libVLC scales whatever it decodes into the requested format without preserving the aspect,
/// so if the buffer disagrees with the source the picture comes back stretched. Re-arm, then
/// put the playhead back where the user was.
fn align_video_to_source(
    instance: Arc<Instance>,
    player: Arc<MediaPlayer>,
    path: PathBuf,
    generation: u64,
) {
    std::thread::Builder::new()
        .name("vfm-probe".into())
        .spawn(move || {
            let mut size = None;
            let mut from_player = None;
            for _ in 0..150 {
                if OPEN_GENERATION.load(Ordering::Relaxed) != generation {
                    return;
                }
                if let Some(video) = player.media_tracks().video {
                    if video.width > 0 && video.height > 0 {
                        size = Some((video.width, video.height));
                        from_player = Some((video.width, video.height));
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            // The player's own media handle stays empty on this build, so fall back to a
            // preparse on a fresh handle and wait for the demuxer to fill it.
            let mut from_parse = None;
            if size.is_none() {
                if let Ok(media) = instance.media_from_path(&path) {
                    if let Some(video) = media.tracks_wait(Duration::from_millis(3000)).video {
                        if video.width > 0 && video.height > 0 {
                            size = Some((video.width, video.height));
                            from_parse = Some((video.width, video.height));
                        }
                    }
                }
            }
            if std::env::var_os("VFM_DEBUG_TRACKS").is_some() {
                eprintln!(
                    "vfm-tracks: player={from_player:?} preparse={from_parse:?} \
                     buffer={:?}",
                    player.frame_bridge().map(|f| (f.width(), f.height()))
                );
            }
            if OPEN_GENERATION.load(Ordering::Relaxed) != generation {
                return;
            }
            let Some((width, height)) = size else {
                eprintln!(
                    "vfm: could not learn the source size of {} — the picture may be scaled \
                     into the wrong shape",
                    path.display()
                );
                return;
            };
            VIDEO_ASPECT.store(aspect_bits(width, height), Ordering::Relaxed);
            let Some(frames) = player.frame_bridge() else {
                return;
            };
            if frames.width() == width && frames.height() == height {
                return;
            }
            if OPEN_GENERATION.load(Ordering::Relaxed) != generation {
                return;
            }
            // Re-arming stops the player, so remember where the user was and put it back:
            // otherwise opening a file would silently jump to 0:00 once the size is known.
            let resume_at = player.time_ms();
            match player.rearm_video_callbacks(width, height, frames.format()) {
                Ok(true) => {
                    let _ = player.play();
                    if resume_at > 0 {
                        player.set_time_ms(resume_at);
                    }
                }
                Ok(false) => {}
                Err(e) => eprintln!("could not match the video size: {e}"),
            }
        })
        .ok();
}

/// A-B loop points in milliseconds: `(point A, point B)`. `None` means not set yet;
/// `B` set without `A` never happens — the hotkey walks idle → A → A+B → idle.
static AB_LOOP: Mutex<(Option<i64>, Option<i64>)> = Mutex::new((None, None));

/// Aspect ratio (width / height) of the media currently loaded, as `f32` bits.
///
/// Written when a file is opened, read by the presenter to lay the frame out at the video's
/// own shape. There is one player per process, so a plain static is enough.
static VIDEO_ASPECT: AtomicU32 = AtomicU32::new(0);

/// Aspect ratio of a source size, packed into an `AtomicU32` (0.0 when unknown).
fn aspect_bits(width: u32, height: u32) -> u32 {
    if width == 0 || height == 0 {
        return 0.0f32.to_bits();
    }
    (width as f32 / height as f32).to_bits()
}

/// Rotation and mirroring chosen in the sidebar, packed for the presenter thread:
/// bits 0-1 clockwise quarter turns, bit 2 horizontal flip, bit 3 vertical flip.
pub(crate) static VIDEO_TRANSFORM: AtomicU32 = AtomicU32::new(0);

/// Publish the sidebar's rotation/flip so the next presented frame is transformed.
pub(crate) fn store_video_transform(state: &SidebarState) {
    let quarters = state.rotation_index.rem_euclid(4) as u32;
    let bits = quarters | (u32::from(state.flip_h) << 2) | (u32::from(state.flip_v) << 3);
    VIDEO_TRANSFORM.store(bits, Ordering::Relaxed);
}

fn video_transform() -> vfm_core::VideoTransform {
    let bits = VIDEO_TRANSFORM.load(Ordering::Relaxed);
    vfm_core::VideoTransform {
        quarters: (bits & 3) as u8,
        flip_h: bits & 4 != 0,
        flip_v: bits & 8 != 0,
    }
}

/// Swap the media on an existing player. `libvlc_media_player_set_media` retains the media,
/// so the local `Media` handle may drop immediately after the call.
fn swap_media(
    instance: &Arc<Instance>,
    player: &Arc<MediaPlayer>,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let media = instance.media_from_path(path)?;
    // A new file invalidates the previous A-B loop points, just like VLC's own.
    *AB_LOOP.lock().unwrap() = (None, None);
    // A pre-parse only works for formats libVLC can probe instantly; when it reports nothing
    // the worker below corrects the pipeline once the player has demuxed the file. Until then
    // the stage uses `contain`, which never stretches.
    match media.tracks_wait(Duration::from_millis(250)).video {
        Some(video) if video.width > 0 && video.height > 0 => {
            VIDEO_ASPECT.store(aspect_bits(video.width, video.height), Ordering::Relaxed);
            let format = player
                .frame_bridge()
                .map(|frames| frames.format())
                .unwrap_or(PixelFormat::Rgba);
            player.rearm_video_callbacks(video.width, video.height, format)?;
        }
        _ => VIDEO_ASPECT.store(0.0f32.to_bits(), Ordering::Relaxed),
    }
    player.set_media(&media);
    player.play()?;
    // The real size is only readable from the player's own media handle once it is demuxed.
    let generation = OPEN_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    align_video_to_source(
        Arc::clone(instance),
        Arc::clone(player),
        path.to_path_buf(),
        generation,
    );
    Ok(())
}

/// Headless acceptance harness for Phase 1: plays the supplied files in sequence,
/// consumes typed libVLC events, persists watch progress, and verifies resume policy.
fn engine_test(
    args: &Args,
    settings_store: &SettingsStore,
    db: &LibraryStore,
    seconds_per_file: u64,
    data_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let settings: Settings = settings_store.get();
    let instance = Instance::new()?;
    let fmt = PixelFormat::parse(&args.chroma)?;
    let mut machine = PlayerStateMachine::new();
    let (smtc_tx, _smtc_rx) = mpsc::channel();
    let _smtc = match smtc::Smtc::attach(smtc_tx) {
        Ok(s) => {
            println!("SMTC attached");
            Some(s)
        }
        Err(e) => {
            eprintln!("SMTC unavailable: {e}");
            None
        }
    };

    for (index, raw) in args.media.iter().enumerate() {
        let path = PathBuf::from(raw);
        if !path.is_file() {
            eprintln!("skip missing file: {}", path.display());
            continue;
        }
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        db.upsert_media(&path, &title, 0)?;

        let resume = if settings.remember_position {
            db.progress(&path)?
                .filter(|p| should_resume(p, settings.resume_min_remaining_ms))
        } else {
            None
        };
        if let Some(p) = &resume {
            println!(
                "[{index}] resume {} at {}",
                path.display(),
                format_timecode(p.position_ms)
            );
        } else {
            println!("[{index}] play {}", path.display());
        }

        let media = instance.media_from_path(&path)?;
        let mut player = media.player()?;
        player.enable_video_callbacks(args.width, args.height, fmt)?;
        let events = player.subscribe_events()?;
        player.play()?;
        // libVLC ignores set_time before playback starts. Retry briefly after play so a
        // resumed session never silently rewinds to the beginning.
        if let Some(p) = &resume {
            for _ in 0..20 {
                player.set_time_ms(p.position_ms);
                if player.time_ms() >= p.position_ms.saturating_sub(500) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        let deadline = Instant::now() + Duration::from_secs(seconds_per_file);
        let mut last_len = 0i64;
        while Instant::now() < deadline {
            // Drain typed events; the state machine turns them into side-effect actions.
            while let Ok(ev) = events.try_recv() {
                let core_ev = match ev {
                    PlayerEvent::Playing => CoreEvent::Playing,
                    PlayerEvent::Paused => CoreEvent::Paused,
                    PlayerEvent::Stopped => CoreEvent::Stopped,
                    PlayerEvent::EndReached => CoreEvent::EndReached,
                    PlayerEvent::EncounteredError => CoreEvent::Error("libVLC error".into()),
                    PlayerEvent::TimeChanged | PlayerEvent::PositionChanged => {
                        CoreEvent::TimeChanged(player.time_ms())
                    }
                    PlayerEvent::LengthChanged => CoreEvent::LengthChanged(player.length_ms()),
                    PlayerEvent::Muted => CoreEvent::Muted,
                    PlayerEvent::Unmuted => CoreEvent::Unmuted,
                    PlayerEvent::VolumeChanged => CoreEvent::VolumeChanged(player.volume()),
                    PlayerEvent::MediaChanged => CoreEvent::MediaChanged(path.clone()),
                    PlayerEvent::ChapterChanged
                    | PlayerEvent::TrackListChanged
                    | PlayerEvent::TitleChanged
                    | PlayerEvent::SeekableChanged
                    | PlayerEvent::Opening
                    | PlayerEvent::Buffering
                    | PlayerEvent::Overflow => {
                        continue;
                    }
                };
                for action in machine.apply(core_ev) {
                    match action {
                        PlayerAction::SeekTo(ms) => player.set_time_ms(ms),
                        PlayerAction::SaveProgress {
                            path,
                            position_ms,
                            duration_ms,
                        } => {
                            let mut p = WatchProgress::new(path, position_ms, duration_ms);
                            p.update_watched();
                            db.save_progress(&p)?;
                        }
                        PlayerAction::AdvanceQueue => break,
                        PlayerAction::ShowError(msg) => eprintln!("engine error: {msg}"),
                        PlayerAction::None => {}
                    }
                }
            }
            let len = player.length_ms();
            if len != last_len {
                machine.apply(CoreEvent::LengthChanged(len));
                last_len = len;
            }
            std::thread::sleep(Duration::from_millis(80));
        }

        let pos = player.time_ms();
        let len = player.length_ms();
        player.stop();
        let mut progress = WatchProgress::new(&path, pos, len);
        progress.update_watched();
        db.save_progress(&progress)?;
        println!(
            "[{index}] saved progress {} / {} (watched={})",
            format_timecode(pos),
            format_timecode(len),
            progress.watched
        );
    }

    println!(
        "engine test complete: {} file(s), database at {}",
        args.media.len(),
        db_path_hint(data_dir)
    );
    Ok(())
}

fn db_path_hint(data_dir: &Path) -> String {
    data_dir
        .join("library.sqlite")
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn verbatim_paths_are_handed_to_libvlc_as_plain_paths() {
        assert_eq!(
            without_verbatim_prefix(PathBuf::from(r"\\?\D:\media\a.mp4")),
            PathBuf::from(r"D:\media\a.mp4")
        );
        assert_eq!(
            without_verbatim_prefix(PathBuf::from(r"\\?\UNC\server\share\a.mp4")),
            PathBuf::from(r"\\server\share\a.mp4")
        );
        assert_eq!(
            without_verbatim_prefix(PathBuf::from(r"D:\media\a.mp4")),
            PathBuf::from(r"D:\media\a.mp4")
        );
    }

    #[test]
    fn a_relative_media_path_becomes_a_plain_absolute_path() {
        // `Cargo.toml` exists in the crate directory, so canonicalize succeeds here.
        let path = absolutize(Path::new("Cargo.toml"));
        assert!(path.is_absolute());
        assert!(
            !path.to_string_lossy().starts_with(r"\\?\"),
            "libVLC cannot open verbatim paths: {path:?}"
        );
    }

    #[test]
    fn association_flags_do_not_require_media() {
        let args = Args::try_parse_from(["VFM", "--associate"]).unwrap();
        assert!(args.associate);
        assert!(args.media.is_empty());

        let args = Args::try_parse_from(["VFM", "--unassociate"]).unwrap();
        assert!(args.unassociate);
        assert!(args.media.is_empty());
    }

    #[test]
    fn library_flag_does_not_require_media() {
        let args = Args::try_parse_from(["VFM", "--library"]).unwrap();
        assert!(args.library);
        assert!(args.media.is_empty());
    }

    #[test]
    fn bare_launch_opens_the_library() {
        let args = Args::try_parse_from(["VFM"]).unwrap();
        assert!(opens_library_by_default(&args));
    }

    #[test]
    fn an_explicit_destination_suppresses_the_default_library() {
        for argv in [
            vec!["VFM", "movie.mkv"],
            vec!["VFM", "--settings"],
            vec!["VFM", "--hotkeys"],
            vec!["VFM", "--onboarding"],
            vec!["VFM", "--customize"],
            vec!["VFM", "--open", "0"],
            vec!["VFM", "--library"],
        ] {
            let args = Args::try_parse_from(&argv).unwrap();
            assert!(
                !opens_library_by_default(&args),
                "{argv:?} should not be redirected to the library"
            );
        }
    }

    #[test]
    fn sidebar_flag_selects_a_tab() {
        let args = Args::try_parse_from(["VFM", "--sidebar", "4"]).unwrap();
        assert_eq!(args.sidebar, Some(4));
    }

    #[test]
    fn media_paths_are_resolved_to_absolute_form() {
        // Absolute input is kept as-is, relative input is joined onto the cwd.
        for input in ["D:/vfm-data/Show/Ep01.mkv", "Show/Ep01.mkv"] {
            let resolved = absolutize(Path::new(input));
            assert!(resolved.is_absolute(), "{input} -> {resolved:?}");
            assert!(resolved.to_string_lossy().ends_with("Ep01.mkv"));
        }
    }

    #[test]
    fn gallery_flags_do_not_require_media() {
        let args = Args::try_parse_from([
            "VFM",
            "--gallery",
            "--gallery-theme",
            "light",
            "--gallery-language",
            "ar",
        ])
        .unwrap();
        assert!(args.gallery);
        assert_eq!(args.gallery_theme, "light");
        assert_eq!(args.gallery_language, "ar");
        assert!(args.media.is_empty());
    }
}
