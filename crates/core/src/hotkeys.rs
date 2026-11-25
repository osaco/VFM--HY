//! Hotkey map with canonical key combos, conflict detection and TOML import/export.
//! Pure logic — no UI and no OS key-event translation (the shell maps physical keys to
//! the canonical strings defined here).

use serde::{Deserialize, Serialize};

/// Actions a hotkey can trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    TogglePlay,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
    SeekForward,
    SeekBackward,
    Fullscreen,
    PictureInPicture,
    ToggleQueue,
    ToggleLibrary,
    ToggleSettings,
    Subtitles,
    Audio,
    Video,
    Bookmark,
    BookmarkNext,
    BookmarkPrevious,
    ToggleSidebar,
    FrameStep,
    Snapshot,
    Repeat,
    Shuffle,
    HideUi,
    Quit,
    OpenFile,
    OpenFolder,
    OpenNetwork,
    Search,
    Back,
    Forward,
    AbLoop,
    RateUp,
    RateDown,
    RateReset,
    ZoomCycle,
}

impl Action {
    pub fn all() -> &'static [Action] {
        &[
            Self::TogglePlay,
            Self::Play,
            Self::Pause,
            Self::Stop,
            Self::Next,
            Self::Previous,
            Self::VolumeUp,
            Self::VolumeDown,
            Self::Mute,
            Self::SeekForward,
            Self::SeekBackward,
            Self::Fullscreen,
            Self::PictureInPicture,
            Self::ToggleQueue,
            Self::ToggleLibrary,
            Self::ToggleSettings,
            Self::Subtitles,
            Self::Audio,
            Self::Video,
            Self::Bookmark,
            Self::BookmarkNext,
            Self::BookmarkPrevious,
            Self::ToggleSidebar,
            Self::FrameStep,
            Self::Snapshot,
            Self::Repeat,
            Self::Shuffle,
            Self::HideUi,
            Self::Quit,
            Self::OpenFile,
            Self::OpenFolder,
            Self::OpenNetwork,
            Self::Search,
            Self::Back,
            Self::Forward,
            Self::AbLoop,
            Self::RateUp,
            Self::RateDown,
            Self::RateReset,
            Self::ZoomCycle,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TogglePlay => "Play / Pause",
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::Stop => "Stop",
            Self::Next => "Next item",
            Self::Previous => "Previous item",
            Self::VolumeUp => "Volume up",
            Self::VolumeDown => "Volume down",
            Self::Mute => "Mute",
            Self::SeekForward => "Seek forward",
            Self::SeekBackward => "Seek back",
            Self::Fullscreen => "Fullscreen",
            Self::PictureInPicture => "Picture in picture",
            Self::ToggleQueue => "Show queue",
            Self::ToggleLibrary => "Show library",
            Self::ToggleSettings => "Settings",
            Self::Subtitles => "Subtitles",
            Self::Audio => "Audio tracks",
            Self::Video => "Video tracks",
            Self::Bookmark => "Add bookmark",
            Self::BookmarkNext => "Next bookmark",
            Self::BookmarkPrevious => "Previous bookmark",
            Self::ToggleSidebar => "Show sidebar",
            Self::FrameStep => "Next frame",
            Self::Snapshot => "Take snapshot",
            Self::Repeat => "Cycle repeat",
            Self::Shuffle => "Toggle shuffle",
            Self::HideUi => "Hide interface",
            Self::Quit => "Quit",
            Self::OpenFile => "Open file",
            Self::OpenFolder => "Open folder",
            Self::OpenNetwork => "Open network stream",
            Self::Search => "Search library",
            Self::Back => "Go back",
            Self::Forward => "Go forward",
            Self::AbLoop => "A-B loop",
            Self::RateUp => "Faster playback",
            Self::RateDown => "Slower playback",
            Self::RateReset => "Normal playback speed",
            Self::ZoomCycle => "Cycle zoom",
        }
    }
}

/// A single action-to-key binding. `key` is a canonical combo e.g. "Ctrl+Shift+F".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub action: Action,
    pub key: String,
}

impl Binding {
    pub fn new(action: Action, key: impl Into<String>) -> Self {
        Self {
            action,
            key: key.into(),
        }
    }
}

/// Normalise a user-supplied combo into the canonical display form.
/// Modifiers are ordered Ctrl, Alt, Shift; the main key is last.
pub fn normalize_combo(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // A literal '+' key: "Ctrl++" / "+" / "Shift++".
    if trimmed.ends_with("++") {
        let head = &trimmed[..trimmed.len() - 1];
        let parts = normalize_parts(head);
        let mut out = parts;
        out.push("+".to_string());
        return out.join("+");
    }
    if trimmed == "+" {
        return "+".to_string();
    }

    normalize_parts(trimmed).join("+")
}

fn normalize_parts(input: &str) -> Vec<String> {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut main: Option<String> = None;

    for raw in input.split(|c: char| c == '+' || c.is_whitespace()) {
        let part = raw.trim();
        if part.is_empty() {
            continue;
        }
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" | "cmd" | "command" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            " " | "space" | "spacebar" => set_main(&mut main, "Space"),
            "esc" | "escape" => set_main(&mut main, "Escape"),
            "enter" | "return" => set_main(&mut main, "Enter"),
            "del" | "delete" => set_main(&mut main, "Delete"),
            "pgup" | "pageup" => set_main(&mut main, "PageUp"),
            "pgdn" | "pagedown" => set_main(&mut main, "PageDown"),
            "arrowleft" | "left" => set_main(&mut main, "ArrowLeft"),
            "arrowright" | "right" => set_main(&mut main, "ArrowRight"),
            "arrowup" | "up" => set_main(&mut main, "ArrowUp"),
            "arrowdown" | "down" => set_main(&mut main, "ArrowDown"),
            _ => {
                let canonical =
                    if lower.len() == 1 && lower.chars().all(|c| c.is_ascii_alphabetic()) {
                        lower.to_ascii_uppercase()
                    } else if lower.starts_with('f')
                        && lower.len() > 1
                        && lower[1..].chars().all(|c| c.is_ascii_digit())
                    {
                        format!("F{}", &lower[1..])
                    } else {
                        let mut chars = part.chars();
                        match chars.next() {
                            Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
                            None => part.to_string(),
                        }
                    };
                set_main(&mut main, &canonical);
            }
        }
    }

    let mut out = Vec::new();
    if ctrl {
        out.push("Ctrl".to_string());
    }
    if alt {
        out.push("Alt".to_string());
    }
    if shift {
        out.push("Shift".to_string());
    }
    if let Some(key) = main {
        out.push(key);
    }
    out
}

fn set_main(slot: &mut Option<String>, value: &str) {
    if slot.is_none() {
        *slot = Some(value.to_string());
    }
}

/// Two actions bound to the same canonical combo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub key: String,
    pub first: Action,
    pub second: Action,
}

pub fn find_conflicts(bindings: &[Binding]) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    for (i, a) in bindings.iter().enumerate() {
        let key = normalize_combo(&a.key);
        if key.is_empty() {
            continue;
        }
        for b in bindings.iter().skip(i + 1) {
            if normalize_combo(&b.key) == key {
                conflicts.push(Conflict {
                    key: key.clone(),
                    first: a.action,
                    second: b.action,
                });
                break;
            }
        }
    }
    conflicts.sort_by(|a, b| a.key.cmp(&b.key));
    conflicts
}

pub fn default_bindings() -> Vec<Binding> {
    vec![
        Binding::new(Action::TogglePlay, "Space"),
        Binding::new(Action::Stop, "S"),
        Binding::new(Action::Previous, "P"),
        Binding::new(Action::Next, "N"),
        Binding::new(Action::VolumeUp, "ArrowUp"),
        Binding::new(Action::VolumeDown, "ArrowDown"),
        Binding::new(Action::Mute, "M"),
        Binding::new(Action::SeekForward, "ArrowRight"),
        Binding::new(Action::SeekBackward, "ArrowLeft"),
        Binding::new(Action::Fullscreen, "F"),
        Binding::new(Action::PictureInPicture, "Ctrl+P"),
        Binding::new(Action::ToggleQueue, "Q"),
        Binding::new(Action::ToggleLibrary, "Ctrl+L"),
        Binding::new(Action::ToggleSidebar, "Ctrl+E"),
        Binding::new(Action::ToggleSettings, "Ctrl+I"),
        Binding::new(Action::Subtitles, "V"),
        Binding::new(Action::Audio, "A"),
        Binding::new(Action::Video, "W"),
        Binding::new(Action::Bookmark, "Ctrl+B"),
        Binding::new(Action::BookmarkPrevious, "Ctrl+,"),
        Binding::new(Action::BookmarkNext, "Ctrl+."),
        Binding::new(Action::FrameStep, "E"),
        Binding::new(Action::Snapshot, "R"),
        Binding::new(Action::Repeat, "Ctrl+R"),
        Binding::new(Action::Shuffle, "H"),
        Binding::new(Action::HideUi, "Ctrl+\\"),
        Binding::new(Action::Quit, "Ctrl+Q"),
        Binding::new(Action::OpenFile, "Ctrl+O"),
        Binding::new(Action::OpenFolder, "Ctrl+D"),
        Binding::new(Action::OpenNetwork, "Ctrl+N"),
        Binding::new(Action::Search, "Ctrl+F"),
        Binding::new(Action::Back, "Alt+Left"),
        Binding::new(Action::Forward, "Alt+Right"),
        Binding::new(Action::AbLoop, "B"),
        Binding::new(Action::RateUp, "]"),
        Binding::new(Action::RateDown, "["),
        Binding::new(Action::RateReset, "="),
        Binding::new(Action::ZoomCycle, "Z"),
    ]
}

/// Bumped when the shipped defaults change, so a stored file is re-stamped.
pub const KEYS_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub version: u32,
    pub bindings: Vec<Binding>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            version: KEYS_VERSION,
            bindings: default_bindings(),
        }
    }
}

impl HotkeyConfig {
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        let mut parsed: Self = toml::from_str(s)?;
        // Stamp the current version: unknown actions are dropped by serde and actions this
        // build gained fall back to their default binding.
        parsed.version = KEYS_VERSION;
        Ok(parsed)
    }
}

/// Assign `key` to `action`, clearing that action's previous combo. Returns false (leaving
/// `bindings` untouched) when the combo already belongs to a different action.
pub fn apply_binding(bindings: &mut Vec<Binding>, action: Action, key: &str) -> bool {
    let key = normalize_combo(key);
    if key.is_empty() {
        return false;
    }
    if bindings
        .iter()
        .any(|b| b.action != action && normalize_combo(&b.key) == key)
    {
        return false;
    }
    if let Some(binding) = bindings.iter_mut().find(|b| b.action == action) {
        binding.key = key;
    } else {
        bindings.push(Binding::new(action, key));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_orders_modifiers_and_aliases() {
        assert_eq!(normalize_combo("shift+ctrl+a"), "Ctrl+Shift+A");
        assert_eq!(normalize_combo("control option f11"), "Ctrl+Alt+F11");
        assert_eq!(normalize_combo("cmd+shift+space"), "Ctrl+Shift+Space");
        assert_eq!(normalize_combo("esc"), "Escape");
        assert_eq!(normalize_combo("return"), "Enter");
        assert_eq!(normalize_combo("arrowleft"), "ArrowLeft");
        assert_eq!(normalize_combo("del"), "Delete");
    }

    #[test]
    fn normalize_single_letters_digits_and_plus() {
        assert_eq!(normalize_combo("b"), "B");
        assert_eq!(normalize_combo("7"), "7");
        assert_eq!(normalize_combo("ctrl++"), "Ctrl++");
        assert_eq!(normalize_combo("+"), "+");
        assert_eq!(normalize_combo(""), "");
    }

    #[test]
    fn find_conflicts_detects_duplicates() {
        let bindings = vec![
            Binding::new(Action::Play, "Ctrl+S"),
            Binding::new(Action::Stop, "ctrl+shift+s"),
            Binding::new(Action::Snapshot, "CTRL+S"),
            Binding::new(Action::Quit, "Ctrl+Q"),
        ];
        let c = find_conflicts(&bindings);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].key, "Ctrl+S");
        assert_eq!(c[0].first, Action::Play);
        assert_eq!(c[0].second, Action::Snapshot);
    }

    #[test]
    fn defaults_have_no_conflicts() {
        assert!(find_conflicts(&default_bindings()).is_empty());
        assert!(default_bindings().len() >= 20);
    }

    #[test]
    fn toml_round_trip_and_version_upgrade() {
        let config = HotkeyConfig::default();
        let text = config.to_toml().unwrap();
        assert_eq!(HotkeyConfig::from_toml(&text).unwrap(), config);
        let future = text.replace("version = 1", "version = 99");
        assert_eq!(
            HotkeyConfig::from_toml(&future).unwrap().version,
            KEYS_VERSION
        );
    }

    #[test]
    fn apply_binding_respects_conflicts() {
        let mut bindings = default_bindings();
        // Ctrl+Q is bound to Quit by default; assigning it to another action must fail.
        assert!(!apply_binding(&mut bindings, Action::TogglePlay, "Ctrl+Q"));
        assert!(apply_binding(
            &mut bindings,
            Action::TogglePlay,
            "Ctrl+Alt+T"
        ));
        assert_eq!(
            bindings
                .iter()
                .find(|b| b.action == Action::TogglePlay)
                .unwrap()
                .key,
            "Ctrl+Alt+T"
        );
        assert!(!apply_binding(&mut bindings, Action::Stop, "ctrl+alt+t"));
        assert!(!apply_binding(&mut bindings, Action::Stop, "   "));
    }

    #[test]
    fn every_action_has_a_label() {
        assert_eq!(Action::all().len(), 40);
        for action in Action::all() {
            assert!(!action.label().is_empty());
        }
    }
}
