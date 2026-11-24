//! Hotkey persistence and the rows the shortcut dialog renders.

use std::path::{Path, PathBuf};

use slint::SharedString;
use vfm_core::hotkeys::{find_conflicts, normalize_combo, Action, HotkeyConfig};
use vfm_core::Language;

use crate::i18n::translate;
use crate::HotkeyRow;

/// File name inside the data directory.
pub const FILE_NAME: &str = "hotkeys.toml";
/// Where import/export land until the native file picker arrives.
pub const EXCHANGE_NAME: &str = "hotkeys-exchange.toml";

pub fn load(dir: &Path) -> HotkeyConfig {
    let path = dir.join(FILE_NAME);
    match std::fs::read_to_string(&path) {
        Ok(text) => HotkeyConfig::from_toml(&text).unwrap_or_default(),
        Err(_) => HotkeyConfig::default(),
    }
}

pub fn save(dir: &Path, config: &HotkeyConfig) -> std::io::Result<()> {
    let path = dir.join(FILE_NAME);
    let text = config
        .to_toml()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}

pub fn export(dir: &Path, config: &HotkeyConfig) -> Result<PathBuf, String> {
    let path = dir.join(EXCHANGE_NAME);
    let text = config.to_toml().map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn import(dir: &Path) -> Result<HotkeyConfig, String> {
    let path = dir.join(EXCHANGE_NAME);
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    HotkeyConfig::from_toml(&text).map_err(|e| e.to_string())
}

/// i18n key for an action label.
pub fn label_key(action: Action) -> &'static str {
    match action {
        Action::TogglePlay => "hotkey.toggle_play",
        Action::Play => "hotkey.play",
        Action::Pause => "hotkey.pause",
        Action::Stop => "hotkey.stop",
        Action::Next => "hotkey.next",
        Action::Previous => "hotkey.previous",
        Action::VolumeUp => "hotkey.volume_up",
        Action::VolumeDown => "hotkey.volume_down",
        Action::Mute => "hotkey.mute",
        Action::SeekForward => "hotkey.seek_forward",
        Action::SeekBackward => "hotkey.seek_backward",
        Action::Fullscreen => "hotkey.fullscreen",
        Action::PictureInPicture => "hotkey.picture_in_picture",
        Action::ToggleQueue => "hotkey.toggle_queue",
        Action::ToggleLibrary => "hotkey.toggle_library",
        Action::ToggleSettings => "hotkey.toggle_settings",
        Action::Subtitles => "hotkey.subtitles",
        Action::Audio => "hotkey.audio",
        Action::Video => "hotkey.video",
        Action::Bookmark => "hotkey.bookmark",
        Action::BookmarkNext => "hotkey.bookmark_next",
        Action::BookmarkPrevious => "hotkey.bookmark_previous",
        Action::ToggleSidebar => "hotkey.toggle_sidebar",
        Action::FrameStep => "hotkey.frame_step",
        Action::Snapshot => "hotkey.snapshot",
        Action::Repeat => "hotkey.repeat",
        Action::Shuffle => "hotkey.shuffle",
        Action::HideUi => "hotkey.hide_ui",
        Action::Quit => "hotkey.quit",
        Action::OpenFile => "hotkey.open_file",
        Action::OpenFolder => "hotkey.open_folder",
        Action::OpenNetwork => "hotkey.open_network",
        Action::Search => "hotkey.search",
        Action::Back => "hotkey.back",
        Action::Forward => "hotkey.forward",
        Action::AbLoop => "hotkey.ab_loop",
        Action::RateUp => "hotkey.rate_up",
        Action::RateDown => "hotkey.rate_down",
        Action::RateReset => "hotkey.rate_reset",
        Action::ZoomCycle => "hotkey.zoom_cycle",
    }
}

/// Action ids are positions in [`Action::all`], which is what the Slint rows carry.
pub fn action_index(action: Action) -> i32 {
    Action::all()
        .iter()
        .position(|candidate| *candidate == action)
        .unwrap_or(0) as i32
}

pub fn action_at(index: i32) -> Option<Action> {
    Action::all().get(index.max(0) as usize).copied()
}

pub fn rows(config: &HotkeyConfig, language: Language) -> Vec<HotkeyRow> {
    let conflicts = find_conflicts(&config.bindings);
    Action::all()
        .iter()
        .map(|action| {
            let combo = config
                .bindings
                .iter()
                .find(|b| b.action == *action)
                .map(|b| b.key.clone())
                .unwrap_or_default();
            let conflict = conflicts.iter().any(|c| {
                c.key == normalize_combo(&combo) && (c.first == *action || c.second == *action)
            });
            HotkeyRow {
                label: translate(language, label_key(*action)).into(),
                combo: combo.into(),
                action: action_index(*action),
                conflict,
            }
        })
        .collect()
}

/// Turn a captured key event into the canonical combo string.
///
/// Windows reports Ctrl+letter as an ASCII control character (Ctrl+B arrives as `0x02`),
/// so the letter is recovered from the code before normalising.
pub fn combo_from(text: &str, ctrl: bool, shift: bool, alt: bool) -> Option<String> {
    let mut chars = text.chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        // Named keys such as "ArrowLeft" or multi-character text: keep as-is.
        return Some(normalize_combo(&with_modifiers(text, ctrl, shift, alt)));
    }
    let code = first as u32;
    let key = if code < 0x20 || code == 0x7f {
        if (1..=26).contains(&code) {
            ((b'A' + (code as u8 - 1)) as char).to_string()
        } else if code == 0x09 {
            "Tab".to_string()
        } else if code == 0x0d {
            "Enter".to_string()
        } else if code == 0x08 {
            "Backspace".to_string()
        } else {
            return None;
        }
    } else if first == ' ' {
        "Space".to_string()
    } else {
        first.to_string()
    };
    Some(normalize_combo(&with_modifiers(&key, ctrl, shift, alt)))
}

fn with_modifiers(key: &str, ctrl: bool, shift: bool, alt: bool) -> String {
    let mut combo = String::new();
    if ctrl {
        combo.push_str("Ctrl+");
    }
    if alt {
        combo.push_str("Alt+");
    }
    if shift {
        combo.push_str("Shift+");
    }
    combo.push_str(key);
    combo
}

/// Apply a new combo, reporting why it was rejected.
pub fn assign(config: &mut HotkeyConfig, action: Action, combo: &str) -> Result<(), String> {
    let key = normalize_combo(combo);
    if key.is_empty() {
        return Err("empty".into());
    }
    if vfm_core::hotkeys::apply_binding(&mut config.bindings, action, &key) {
        Ok(())
    } else {
        Err("conflict".into())
    }
}

/// Human-readable status for the dialog.
pub fn status_text(language: Language, error: &str, combo: &str) -> SharedString {
    match error {
        "conflict" => format!(
            "{} — {}",
            translate(language, "settings.conflict"),
            translate(language, "settings.conflict-hint")
        ),
        "empty" => translate(language, "settings.shortcuts-empty").to_string(),
        _ => combo.to_string(),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vfm_core::hotkeys::Binding;

    fn en() -> Language {
        Language::En
    }

    #[test]
    fn rows_cover_every_action() {
        let config = HotkeyConfig::default();
        let rows = rows(&config, en());
        assert_eq!(rows.len(), Action::all().len());
        assert!(rows.iter().all(|r| !r.label.is_empty()));
        // Only the Play/Pause aliases ship unbound; everything else has a combo.
        let unbound: Vec<&str> = rows
            .iter()
            .filter(|r| r.combo.is_empty())
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(unbound, vec!["Play", "Pause"]);
        assert!(rows.iter().all(|r| !r.conflict));
    }

    #[test]
    fn conflicts_are_flagged_on_both_rows() {
        let mut config = HotkeyConfig::default();
        config.bindings.push(Binding::new(Action::Play, "Space")); // Space is TogglePlay
        let rows = rows(&config, en());
        let flagged: Vec<i32> = rows
            .iter()
            .filter(|r| r.conflict)
            .map(|r| r.action)
            .collect();
        assert!(flagged.contains(&action_index(Action::Play)));
        assert!(flagged.contains(&action_index(Action::TogglePlay)));
    }

    #[test]
    fn action_index_round_trips() {
        for action in Action::all() {
            assert_eq!(action_at(action_index(*action)), Some(*action));
        }
        assert_eq!(action_at(-5), Some(Action::TogglePlay));
        assert_eq!(action_at(999), None);
    }

    #[test]
    fn capture_builds_canonical_combos() {
        assert_eq!(combo_from("b", true, false, false).unwrap(), "Ctrl+B");
        // Ctrl+B arrives as STX on Windows.
        assert_eq!(combo_from("\u{2}", true, false, false).unwrap(), "Ctrl+B");
        assert_eq!(combo_from("F11", false, false, false).unwrap(), "F11");
        assert_eq!(
            combo_from("ArrowLeft", false, false, true).unwrap(),
            "Alt+ArrowLeft"
        );
        assert_eq!(combo_from(" ", false, false, false).unwrap(), "Space");
        assert_eq!(combo_from("x", false, true, false).unwrap(), "Shift+X");
        assert_eq!(combo_from("\u{7f}", false, false, false), None);
    }

    #[test]
    fn assign_reports_conflicts_and_empty() {
        let mut config = HotkeyConfig::default();
        assert!(assign(&mut config, Action::Play, "Space").is_err());
        assert!(assign(&mut config, Action::Play, "   ").is_err());
        assert!(assign(&mut config, Action::Play, "Ctrl+Alt+P").is_ok());
        let stored = config
            .bindings
            .iter()
            .find(|b| b.action == Action::Play)
            .unwrap();
        assert_eq!(stored.key, "Ctrl+Alt+P");
    }

    #[test]
    fn save_load_and_exchange_round_trip() {
        let dir = std::env::temp_dir().join(format!("vfm-keys-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut config = HotkeyConfig::default();
        assign(&mut config, Action::Play, "Ctrl+Alt+P").unwrap();
        save(&dir, &config).unwrap();

        let back = load(&dir);
        assert_eq!(back, config);

        let path = export(&dir, &config).unwrap();
        assert!(path.is_file());
        let imported = import(&dir).unwrap();
        assert_eq!(imported, config);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = std::env::temp_dir().join(format!("vfm-keys-missing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load(&dir), HotkeyConfig::default());
        assert!(import(&dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
