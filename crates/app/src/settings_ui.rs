//! Settings window glue: turns the core schema into Slint rows and writes edits back.
//!
//! Rows live in a [`VecModel`] so a slider drag can update one row in place
//! (`set_row_data`) instead of replacing the whole model — replacing it would destroy the
//! component under the pointer and cut the drag short.

use std::cell::RefCell;
use std::rc::Rc;

use slint::{Model, ModelRc, SharedString, VecModel};
use vfm_core::settings_schema::{Category, SettingId, SettingKind, SettingValue};
use vfm_core::{Language, Settings, SettingsStore};

use crate::i18n::translate;
use crate::{CategoryRow, SettingRow, SettingsWindow};

/// Kind codes shared with `settings.slint`.
pub const KIND_TOGGLE: i32 = 0;
pub const KIND_CHOICE: i32 = 1;
pub const KIND_SLIDER: i32 = 2;
pub const KIND_TEXT: i32 = 3;
pub const KIND_SECRET: i32 = 4;
pub const KIND_FOLDER: i32 = 5;
pub const KIND_SWATCH: i32 = 6;

/// Accent palette offered in General. Eight hues that stay legible on both themes.
pub const ACCENTS: &[&str] = &[
    "#D37918", "#C0392B", "#2E9E5B", "#2D7FF9", "#7C5CE6", "#D6336C", "#0E9AA7", "#8D6E63",
];

#[derive(Debug, Clone)]
pub struct ViewState {
    pub category: Category,
    pub query: String,
    pub show_advanced: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            category: Category::General,
            query: String::new(),
            show_advanced: false,
        }
    }
}

impl ViewState {
    pub fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }
}

/// Slider read-out precision and unit divisor, derived from the setting's key.
fn readout(id: SettingId, value: f64) -> (String, &'static str, f64) {
    match id {
        SettingId::SeekStepMs => (format!("{:.1}", value / 1000.0), " s", 1.0),
        SettingId::ResumeMinRemainingMs => {
            (format!("{}", (value / 1000.0).round() as i64), " s", 1.0)
        }
        SettingId::Volume | SettingId::VolumeStep => {
            (format!("{}", value.round() as i64), " %", 1.0)
        }
        SettingId::NetworkTimeoutMs => (format!("{:.1}", value / 1000.0), " s", 1.0),
        SettingId::NetworkCachingMs => (format!("{}", value.round() as i64), " ms", 1.0),
        SettingId::SidebarTab => {
            let names = ["Playback", "Subtitles", "Audio", "Video", "Presets"];
            let index = value.round() as usize;
            (
                names.get(index).copied().unwrap_or("Playback").to_string(),
                "",
                1.0,
            )
        }
        _ => {
            if value.fract().abs() < 0.001 {
                (format!("{}", value.round() as i64), "", 1.0)
            } else {
                (format!("{value:.2}"), "", 1.0)
            }
        }
    }
}

fn option_label(language: Language, option: &vfm_core::settings_schema::Choice) -> String {
    translate(language, option.label_key()).to_string()
}

/// Build one row from the schema plus the current value.
fn build_row(
    id: SettingId,
    settings: &Settings,
    language: Language,
    options_start: i32,
    tag: String,
) -> (SettingRow, Vec<SharedString>) {
    let value = settings.value(id);
    let mut pool: Vec<SharedString> = Vec::new();
    let kind;
    let (mut min, mut max, mut step) = (0.0f32, 0.0f32, 0.0f32);
    let mut choice_index = -1i32;
    let mut count = 0i32;
    let mut suffix = String::new();
    let value_text;
    let mut checked = false;
    let mut number = 0.0f32;

    match id.kind() {
        SettingKind::Toggle => {
            kind = KIND_TOGGLE;
            checked = value.as_bool();
            value_text = translate(
                language,
                if checked {
                    "settings.on"
                } else {
                    "settings.off"
                },
            )
            .to_string();
        }
        SettingKind::Choice(choices) => {
            kind = KIND_CHOICE;
            let current = value.as_choice();
            for (index, option) in choices.iter().enumerate() {
                if option.id == current {
                    choice_index = index as i32;
                }
                pool.push(option_label(language, option).into());
            }
            count = choices.len() as i32;
            value_text = pool
                .get(choice_index.max(0) as usize)
                .map(|s| s.to_string())
                .unwrap_or_default();
        }
        SettingKind::Slider {
            min: lo,
            max: hi,
            step: st,
        } => {
            kind = KIND_SLIDER;
            min = lo as f32;
            max = hi as f32;
            step = st as f32;
            number = value.as_number() as f32;
            let (text, unit, _) = readout(id, value.as_number());
            value_text = text;
            suffix = unit.to_string();
        }
        SettingKind::Text { secret } => {
            kind = if secret { KIND_SECRET } else { KIND_TEXT };
            value_text = value.as_text();
        }
        SettingKind::Folder => {
            kind = KIND_FOLDER;
            value_text = value.as_text();
        }
        SettingKind::Color => {
            kind = KIND_SWATCH;
            let current = value.as_text();
            for accent in ACCENTS {
                if accent.eq_ignore_ascii_case(&current) {
                    choice_index = pool.len() as i32;
                }
                pool.push((*accent).into());
            }
            count = ACCENTS.len() as i32;
            value_text = current;
        }
    }

    let default_value = Settings::default().value(id);
    let row = SettingRow {
        key: id.key().into(),
        label: translate(language, id.label_key()).into(),
        help: translate(language, id.help_key()).into(),
        kind,
        checked,
        number,
        min,
        max,
        step,
        choice_index,
        options_start,
        options_count: count,
        value_text: value_text.into(),
        suffix: suffix.into(),
        advanced: id.advanced(),
        modified: value != default_value,
        tag: tag.into(),
    };
    (row, pool)
}

fn visible_ids(state: &ViewState, language: Language) -> Vec<SettingId> {
    let text = |key: &str| translate(language, key).to_string();
    if state.searching() {
        vfm_core::settings_schema::search(&text, &state.query)
    } else {
        vfm_core::settings_schema::in_category(state.category)
    }
}

/// Owns the three models the settings window binds to.
pub struct SettingsUi {
    pub rows: Rc<VecModel<SettingRow>>,
    pub categories: Rc<VecModel<CategoryRow>>,
    pub options: Rc<VecModel<SharedString>>,
    pub state: RefCell<ViewState>,
    /// Setting ids parallel to `rows`, so a row index maps back without re-parsing keys.
    keys: RefCell<Vec<SettingId>>,
    /// Last time a slider edit was written to disk.
    last_flush: RefCell<std::time::Instant>,
}

impl Default for SettingsUi {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsUi {
    pub fn new() -> Self {
        Self {
            rows: Rc::new(VecModel::default()),
            categories: Rc::new(VecModel::default()),
            options: Rc::new(VecModel::default()),
            state: RefCell::new(ViewState::default()),
            keys: RefCell::new(Vec::new()),
            last_flush: RefCell::new(std::time::Instant::now()),
        }
    }

    /// Bind the models once; `refresh` fills them.
    pub fn attach(&self, win: &SettingsWindow) {
        win.set_rows(ModelRc::from(self.rows.clone()));
        win.set_categories(ModelRc::from(self.categories.clone()));
        win.set_options(ModelRc::from(self.options.clone()));
    }

    /// Rebuild every row for the current view state (models only).
    pub fn refresh_models(&self, settings: &Settings, language: Language) {
        let state = self.state.borrow().clone();
        let searching = state.searching();
        let ids = visible_ids(&state, language);

        let mut rows = Vec::with_capacity(ids.len());
        let mut pool: Vec<SharedString> = Vec::new();
        let mut keys = Vec::with_capacity(ids.len());
        for id in ids {
            let tag = if searching {
                translate(language, id.category().label_key()).to_string()
            } else {
                String::new()
            };
            let (row, options) = build_row(id, settings, language, pool.len() as i32, tag);
            pool.extend(options);
            rows.push(row);
            keys.push(id);
        }

        // Only rows whose advanced flag matches the toggle are shown; `keys` stays parallel.
        let show_advanced = state.show_advanced;
        let mut visible_rows = Vec::new();
        let mut visible_keys = Vec::new();
        for (row, key) in rows.into_iter().zip(keys) {
            if row.advanced && !show_advanced && !searching {
                continue;
            }
            visible_rows.push(row);
            visible_keys.push(key);
        }

        self.rows.set_vec(visible_rows);
        self.options.set_vec(pool);
        *self.keys.borrow_mut() = visible_keys;

        self.refresh_category_models(settings, language, &state);
    }

    /// Rebuild the rows and push the derived window properties.
    pub fn refresh(&self, win: &SettingsWindow, settings: &Settings, language: Language) {
        let state = self.state.borrow().clone();
        let searching = state.searching();
        self.refresh_models(settings, language);
        self.refresh_categories(win, settings, language, &state);
        // Only write the field when it actually differs: the field is bound to the same
        // state, so echoing it back would re-enter the change handler.
        if win.get_search_text() != state.query.as_str() {
            win.set_search_text(state.query.clone().into());
        }
        win.set_show_advanced(state.show_advanced);
        win.set_searching(searching);
        win.set_empty(self.rows.row_count() == 0);
        win.set_heading(if searching {
            translate(language, "settings.results").into()
        } else {
            translate(language, state.category.label_key()).into()
        });
        win.set_advanced_label(translate(language, "settings.show-advanced").into());
    }

    /// Category model only; the window push lives in `refresh_categories`.
    fn refresh_category_models(&self, settings: &Settings, language: Language, state: &ViewState) {
        let mut rows = Vec::with_capacity(Category::ALL.len());
        for (index, category) in Category::ALL.iter().enumerate() {
            let ids = vfm_core::settings_schema::in_category(*category);
            let modified = ids
                .iter()
                .filter(|id| settings.value(**id) != Settings::default().value(**id))
                .count() as i32;
            rows.push(CategoryRow {
                label: translate(language, category.label_key()).into(),
                count: ids.len() as i32,
                modified,
                index: index as i32,
                selected: *category == state.category && !state.searching(),
            });
        }
        self.categories.set_vec(rows);
    }

    fn refresh_categories(
        &self,
        win: &SettingsWindow,
        settings: &Settings,
        language: Language,
        state: &ViewState,
    ) {
        self.refresh_category_models(settings, language, state);
        win.set_current_category(
            Category::ALL
                .iter()
                .position(|c| *c == state.category)
                .unwrap_or(0) as i32,
        );
    }

    fn index_of(&self, key: &str) -> Option<usize> {
        self.keys.borrow().iter().position(|id| id.key() == key)
    }

    /// Rewrite a single row after an edit, keeping the model (and any live drag) intact.
    fn refresh_row(&self, settings: &Settings, language: Language, key: &str) {
        let Some(index) = self.index_of(key) else {
            return;
        };
        let Some(id) = SettingId::from_key(key) else {
            return;
        };
        let Some(existing) = self.rows.row_data(index) else {
            return;
        };
        let (row, pool) = build_row(
            id,
            settings,
            language,
            existing.options_start,
            existing.tag.to_string(),
        );
        // Choice pools are stable per setting, so only the values need rewriting.
        for (offset, label) in pool.iter().enumerate() {
            self.options
                .set_row_data(existing.options_start as usize + offset, label.clone());
        }
        self.rows.set_row_data(index, row);
    }

    pub fn set_category(
        &self,
        win: &SettingsWindow,
        settings: &Settings,
        language: Language,
        index: i32,
    ) {
        if let Some(category) = Category::ALL.get(index.max(0) as usize) {
            let mut state = self.state.borrow_mut();
            state.category = *category;
            state.query.clear();
            drop(state);
            self.refresh(win, settings, language);
        }
    }

    pub fn set_query(
        &self,
        win: &SettingsWindow,
        settings: &Settings,
        language: Language,
        query: &str,
    ) {
        self.state.borrow_mut().query = query.to_string();
        self.refresh(win, settings, language);
    }

    pub fn toggle_advanced(&self, win: &SettingsWindow, settings: &Settings, language: Language) {
        let mut state = self.state.borrow_mut();
        state.show_advanced = !state.show_advanced;
        drop(state);
        self.refresh(win, settings, language);
    }

    /// Resolve a row's `key` + option index into the schema value and store it.
    pub fn apply_choice(&self, store: &SettingsStore, language: Language, key: &str, index: i32) {
        let Some(id) = SettingId::from_key(key) else {
            return;
        };
        let value = match id.kind() {
            SettingKind::Choice(choices) => choices
                .get(index.max(0) as usize)
                .map(|option| SettingValue::Choice(option.id)),
            SettingKind::Color => ACCENTS
                .get(index.max(0) as usize)
                .map(|accent| SettingValue::Text((*accent).to_string())),
            _ => None,
        };
        let Some(value) = value else { return };
        let _ = store.update(|s| {
            s.set_value(id, &value);
        });
        self.after_edit(store, language, id);
    }

    pub fn apply_bool(&self, store: &SettingsStore, language: Language, key: &str, checked: bool) {
        let Some(id) = SettingId::from_key(key) else {
            return;
        };
        let _ = store.update(|s| {
            s.set_value(id, &SettingValue::Bool(checked));
        });
        self.after_edit(store, language, id);
    }

    pub fn apply_number(&self, store: &SettingsStore, language: Language, key: &str, value: f32) {
        let Some(id) = SettingId::from_key(key) else {
            return;
        };
        // Quantise to the declared step so the stored value is exactly what the UI shows.
        let snapped = match id.kind() {
            SettingKind::Slider { min, max, step } => {
                let steps = ((value as f64 - min) / step).round();
                (min + steps * step).clamp(min, max)
            }
            _ => value as f64,
        };
        // A drag fires many of these; keep the file write off the hot path and flush on a
        // short timer (or when the gesture ends, via `commit`).
        store.update_quiet(|s| {
            s.set_value(id, &SettingValue::Number(snapped));
        });
        let due = {
            let mut last = self.last_flush.borrow_mut();
            let due = last.elapsed() > std::time::Duration::from_millis(250);
            if due {
                *last = std::time::Instant::now();
            }
            due
        };
        if due {
            let _ = store.flush();
        }
        self.after_edit(store, language, id);
    }

    /// Persist whatever a slider drag left in memory.
    pub fn commit(&self, store: &SettingsStore) {
        let _ = store.flush();
        *self.last_flush.borrow_mut() = std::time::Instant::now();
    }

    pub fn apply_text(&self, store: &SettingsStore, language: Language, key: &str, text: &str) {
        let Some(id) = SettingId::from_key(key) else {
            return;
        };
        let _ = store.update(|s| {
            s.set_value(id, &SettingValue::Text(text.trim().to_string()));
        });
        self.after_edit(store, language, id);
    }

    pub fn reset(&self, store: &SettingsStore, language: Language, key: &str) {
        let Some(id) = SettingId::from_key(key) else {
            return;
        };
        let _ = store.update(|s| s.reset_value(id));
        self.after_edit(store, language, id);
    }

    fn after_edit(&self, store: &SettingsStore, language: Language, id: SettingId) {
        let settings = store.get();
        self.refresh_row(&settings, language, id.key());
        self.refresh_category_models(&settings, language, &self.state.borrow().clone());
    }

    /// Push the derived values the window shows outside the list (dirty count, labels).
    pub fn refresh_chrome(&self, win: &SettingsWindow, settings: &Settings, language: Language) {
        win.set_dirty_count(
            SettingId::ALL
                .iter()
                .filter(|id| settings.value(**id) != Settings::default().value(**id))
                .count() as i32,
        );
        win.set_modified_label(translate(language, "settings.modified").into());
        win.set_no_results_label(translate(language, "settings.no-results").into());
        win.set_shortcuts_label(translate(language, "settings.shortcuts").into());
    }

    pub fn reset_all(&self, store: &SettingsStore, language: Language) {
        let _ = store.update(|s| {
            for id in SettingId::ALL {
                s.reset_value(*id);
            }
        });
        let settings = store.get();
        self.refresh_category_models(&settings, language, &self.state.borrow().clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vfm_core::settings_schema::in_category;

    fn lang() -> Language {
        Language::En
    }

    #[test]
    fn every_category_builds_a_row_per_setting() {
        for category in Category::ALL {
            let settings = Settings::default();
            let ids = in_category(*category);
            let mut pool = 0i32;
            for id in &ids {
                let (row, options) = build_row(*id, &settings, lang(), pool, String::new());
                assert_eq!(row.key, id.key());
                assert!(!row.label.is_empty());
                assert!(!row.help.is_empty());
                pool += options.len() as i32;
            }
            assert_eq!(ids.len(), in_category(*category).len());
        }
    }

    #[test]
    fn toggle_rows_report_their_state() {
        let settings = Settings {
            waveform: false,
            ..Default::default()
        };
        let (row, _) = build_row(SettingId::Waveform, &settings, lang(), 0, String::new());
        assert_eq!(row.kind, KIND_TOGGLE);
        assert!(!row.checked);
        assert_eq!(row.value_text, "Off");
    }

    #[test]
    fn choice_rows_point_at_their_option() {
        let settings = Settings::default();
        let (row, pool) = build_row(SettingId::Theme, &settings, lang(), 7, String::new());
        assert_eq!(row.kind, KIND_CHOICE);
        assert_eq!(row.options_start, 7);
        assert_eq!(row.options_count, 3);
        assert_eq!(row.choice_index, 0); // system is first
        assert_eq!(pool.len(), 3);
        assert_eq!(row.value_text, pool[0].as_str());
    }

    #[test]
    fn accent_row_uses_the_swatch_palette() {
        let settings = Settings::default();
        let (row, pool) = build_row(SettingId::Accent, &settings, lang(), 0, String::new());
        assert_eq!(row.kind, KIND_SWATCH);
        assert_eq!(row.choice_index, 0);
        assert_eq!(pool.len(), ACCENTS.len());
    }

    #[test]
    fn slider_rows_carry_range_and_readout() {
        let settings = Settings::default();
        let (row, _) = build_row(SettingId::Volume, &settings, lang(), 0, String::new());
        assert_eq!(row.kind, KIND_SLIDER);
        assert_eq!(row.min, 0.0);
        assert_eq!(row.max, 125.0);
        assert_eq!(row.value_text, "100");
        assert_eq!(row.suffix, " %");
    }

    #[test]
    fn milliseconds_settings_read_out_in_seconds() {
        let settings = Settings::default();
        let (row, _) = build_row(SettingId::SeekStepMs, &settings, lang(), 0, String::new());
        assert_eq!(row.value_text, "5.0");
        assert_eq!(row.suffix, " s");
    }

    #[test]
    fn modified_flag_tracks_the_default() {
        let mut settings = Settings::default();
        let (row, _) = build_row(SettingId::Volume, &settings, lang(), 0, String::new());
        assert!(!row.modified);
        settings.volume = 40;
        let (row, _) = build_row(SettingId::Volume, &settings, lang(), 0, String::new());
        assert!(row.modified);
    }

    #[test]
    fn advanced_rows_are_hidden_until_asked_for() {
        let ui = SettingsUi::new();
        let settings = Settings::default();

        ui.state.borrow_mut().category = Category::Privacy;
        ui.refresh_models(&settings, lang());
        let without = ui.rows.row_count();

        ui.state.borrow_mut().show_advanced = true;
        ui.refresh_models(&settings, lang());
        let with = ui.rows.row_count();

        assert_eq!(with, in_category(Category::Privacy).len());
        assert!(without < with, "advanced rows should be hidden by default");
    }

    #[test]
    fn search_shows_matches_from_every_category() {
        let ui = SettingsUi::new();
        let settings = Settings::default();

        ui.state.borrow_mut().query = "volume".into();
        ui.refresh_models(&settings, lang());
        let keys: Vec<String> = (0..ui.rows.row_count())
            .filter_map(|i| ui.rows.row_data(i).map(|r| r.key.to_string()))
            .collect();
        assert!(keys.iter().any(|k| k == "playback.volume"));
        assert!(keys.iter().any(|k| k == "controls.volume-step"));
        // A search shows the category tag on every row.
        assert!(ui
            .rows
            .row_data(0)
            .map(|r| !r.tag.is_empty())
            .unwrap_or(false));
    }

    #[test]
    fn edits_write_through_and_refresh_the_row() {
        let ui = SettingsUi::new();
        let store = SettingsStore::new();
        ui.state.borrow_mut().category = Category::Interface;
        ui.refresh_models(&store.get(), lang());

        ui.apply_bool(&store, lang(), "interface.waveform", false);
        assert!(!store.get().waveform);
        let index = ui.index_of("interface.waveform").unwrap();
        assert!(!ui.rows.row_data(index).unwrap().checked);
        assert!(ui.rows.row_data(index).unwrap().modified);

        ui.apply_number(&store, lang(), "playback.volume", 42.4);
        assert_eq!(store.get().volume, 42);

        ui.apply_choice(&store, lang(), "general.theme", 1);
        assert_eq!(store.get().theme, vfm_core::Theme::Dark);

        ui.apply_text(&store, lang(), "privacy.opensubtitles-key", "  abc  ");
        assert_eq!(store.get().opensubtitles_api_key, "abc");

        ui.reset(&store, lang(), "playback.volume");
        assert_eq!(store.get().volume, 100);
    }

    #[test]
    fn slider_values_snap_to_the_declared_step() {
        let ui = SettingsUi::new();
        let store = SettingsStore::new();
        ui.apply_number(&store, lang(), "video.zoom", 1.37);
        // step is 0.05
        assert!(
            (store.get().zoom - 1.35).abs() < 1e-9,
            "{}",
            store.get().zoom
        );
    }

    #[test]
    fn category_rows_report_modified_counts() {
        let ui = SettingsUi::new();
        let store = SettingsStore::new();
        let _ = store.update(|s| s.volume = 10);
        ui.refresh_models(&store.get(), lang());
        let playback = (0..ui.categories.row_count())
            .filter_map(|i| ui.categories.row_data(i))
            .find(|c| c.label == "Playback")
            .unwrap();
        assert!(playback.modified >= 1);
    }

    #[test]
    fn reset_all_clears_every_override() {
        let ui = SettingsUi::new();
        let store = SettingsStore::new();
        let _ = store.update(|s| {
            s.volume = 5;
            s.theme = vfm_core::Theme::Light;
            s.zoom = 2.0;
        });
        ui.reset_all(&store, lang());
        assert_eq!(store.get(), Settings::default());
    }
}
