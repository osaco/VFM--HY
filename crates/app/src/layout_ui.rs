//! Layout customisation window: palette, zones, profiles.
//!
//! The window renders `vfm_core::layout` models; every edit is applied to the in-memory
//! `Layout`, persisted to settings, and pushed straight back so the preview is live.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};
use vfm_core::layout::{self, ControlId, Layout, Profile, Zone};
use vfm_core::{Language, LibraryStore, SettingsStore};

use crate::i18n;
use crate::{ControlRow, CustomizeWindow, PlacedRow, ProfileRow};

/// Preset scope used for saved layout profiles.
pub const SCOPE: &str = "layout";

pub struct LayoutUi {
    pub layout: Layout,
    pub profiles: Vec<Profile>,
}

impl LayoutUi {
    /// Load the saved layout (or the default) and the stored profiles.
    pub fn load(store: &SettingsStore, db: &LibraryStore) -> Self {
        let saved = store.get().layout;
        let layout = Layout::from_json(&saved).unwrap_or_default();
        let mut profiles = Vec::new();
        if let Ok(rows) = db.presets(SCOPE) {
            for (name, json) in rows {
                if let Some(mut profile) = Profile::from_json(&json) {
                    profile.name = name;
                    profiles.push(profile);
                }
            }
        }
        profiles.sort_by_key(|profile| profile.name.to_lowercase());
        Self { layout, profiles }
    }

    /// Persist the current layout into settings.
    pub fn persist(&self, store: &SettingsStore) {
        let json = self.layout.to_json();
        let _ = store.update(|s| s.layout = json);
    }

    pub fn add(&mut self, id: &str) {
        if let Some(control) = ControlId::from_id(id) {
            // New controls land at the end of the trailing zone, which is where the
            // secondary actions live on the shipped bar.
            self.layout.add(Zone::Trailing, control);
        }
    }

    pub fn remove(&mut self, id: &str) {
        if let Some(control) = ControlId::from_id(id) {
            self.layout.remove(control);
        }
    }

    pub fn move_up(&mut self, id: &str) {
        if let Some(control) = ControlId::from_id(id) {
            self.layout.move_up(control);
        }
    }

    pub fn move_down(&mut self, id: &str) {
        if let Some(control) = ControlId::from_id(id) {
            self.layout.move_down(control);
        }
    }

    pub fn reset(&mut self) {
        self.layout = Layout::default();
    }

    /// Save (or overwrite) a profile with the current layout.
    pub fn save_profile(&mut self, db: &LibraryStore, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let profile = Profile::new(name, self.layout.clone());
        let json = profile.to_json();
        if db.save_preset(name, SCOPE, &json).is_err() {
            return false;
        }
        match self.profiles.iter_mut().find(|p| p.name == name) {
            Some(existing) => *existing = profile,
            None => self.profiles.push(profile),
        }
        self.profiles
            .sort_by_key(|profile| profile.name.to_lowercase());
        true
    }

    pub fn apply_profile(&mut self, index: usize) -> bool {
        let Some(profile) = self.profiles.get(index).cloned() else {
            return false;
        };
        self.layout = profile.layout;
        true
    }

    pub fn delete_profile(&mut self, db: &LibraryStore, index: usize) -> bool {
        let Some(profile) = self.profiles.get(index).cloned() else {
            return false;
        };
        if db.delete_preset(&profile.name).is_err() {
            return false;
        }
        self.profiles.remove(index);
        true
    }

    /// Write one profile as JSON next to the settings file.
    pub fn export_profile(&self, index: usize, folder: &Path) -> Option<PathBuf> {
        let profile = self.profiles.get(index)?;
        let safe: String = profile
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        let path = folder.join(format!("layout-{safe}.json"));
        std::fs::create_dir_all(folder).ok()?;
        std::fs::write(&path, profile.to_json()).ok()?;
        Some(path)
    }

    /// Read a profile JSON file and store it.
    pub fn import_profile(&mut self, db: &LibraryStore, path: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        let Some(profile) = Profile::from_json(&text) else {
            return false;
        };
        let name = profile.name.clone();
        if db.save_preset(&name, SCOPE, &profile.to_json()).is_err() {
            return false;
        }
        self.profiles.retain(|p| p.name != name);
        self.profiles.push(profile);
        self.profiles
            .sort_by_key(|profile| profile.name.to_lowercase());
        true
    }

    /// One-line summary of a zone, for the preview chips.
    fn zone_summary(&self, zone: Zone, language: Language) -> String {
        let labels: Vec<String> = self
            .layout
            .zone(zone)
            .iter()
            .map(|control| i18n::translate(language, &layout::label_key(*control)).to_string())
            .collect();
        if labels.is_empty() {
            i18n::translate(language, "customize.empty").to_string()
        } else {
            labels.join(" · ")
        }
    }
}

/// Push the whole state into the window.
pub fn push(win: &CustomizeWindow, ui: &LayoutUi, language: Language) {
    let palette: Vec<ControlRow> = ControlId::ALL
        .iter()
        .map(|control| ControlRow {
            id: control.id().into(),
            label: i18n::translate(language, &layout::label_key(*control)).into(),
            placed: ui.layout.contains(*control),
            wide: control.is_wide(),
        })
        .collect();
    win.set_palette(ModelRc::from(Rc::new(VecModel::from(palette))));

    let mut placed = Vec::new();
    for (index, zone) in [Zone::Leading, Zone::Centre, Zone::Trailing]
        .into_iter()
        .enumerate()
    {
        for control in ui.layout.zone(zone) {
            placed.push(PlacedRow {
                id: control.id().into(),
                label: i18n::translate(language, &layout::label_key(*control)).into(),
                zone: index as i32,
            });
        }
    }
    win.set_placed(ModelRc::from(Rc::new(VecModel::from(placed))));

    let profiles: Vec<ProfileRow> = ui
        .profiles
        .iter()
        .map(|profile| ProfileRow {
            name: profile.name.clone().into(),
            detail: format!("{} controls", profile.layout.all().len()).into(),
        })
        .collect();
    win.set_profiles(ModelRc::from(Rc::new(VecModel::from(profiles))));

    let zones: Vec<SharedString> = [
        "customize.zone.leading",
        "customize.zone.centre",
        "customize.zone.trailing",
    ]
    .iter()
    .map(|key| SharedString::from(i18n::translate(language, key)))
    .collect();
    win.set_zones(ModelRc::from(Rc::new(VecModel::from(zones))));
    let summaries: Vec<SharedString> = [Zone::Leading, Zone::Centre, Zone::Trailing]
        .iter()
        .map(|zone| SharedString::from(ui.zone_summary(*zone, language)))
        .collect();
    win.set_zone_summary(ModelRc::from(Rc::new(VecModel::from(summaries))));

    win.set_heading(i18n::translate(language, "customize.title").into());
    win.set_palette_label(i18n::translate(language, "customize.palette").into());
    win.set_bar_label(i18n::translate(language, "customize.bar").into());
    win.set_profiles_label(i18n::translate(language, "customize.profiles").into());
    win.set_hint(i18n::translate(language, "customize.hint").into());
    win.set_save_label(i18n::translate(language, "customize.save").into());
    win.set_reset_label(i18n::translate(language, "customize.reset").into());
    win.set_close_label(i18n::translate(language, "customize.close").into());
    win.set_export_label(i18n::translate(language, "customize.export").into());
    win.set_import_label(i18n::translate(language, "customize.import").into());
    win.set_remove_label("\u{00d7}".into());
    win.set_up_label("\u{2191}".into());
    win.set_down_label("\u{2193}".into());
}

pub fn status(win: &CustomizeWindow, language: Language, key: &str, error: bool) {
    win.set_status(i18n::translate(language, key).into());
    win.set_status_error(error);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db() -> LibraryStore {
        LibraryStore::in_memory().expect("in-memory library")
    }

    #[test]
    fn add_and_remove_round_trip() {
        let mut ui = LayoutUi {
            layout: Layout::default(),
            profiles: Vec::new(),
        };
        ui.remove("fullscreen");
        assert!(!ui.layout.contains(ControlId::Fullscreen));
        ui.add("fullscreen");
        assert!(ui.layout.contains(ControlId::Fullscreen));
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let mut ui = LayoutUi {
            layout: Layout::default(),
            profiles: Vec::new(),
        };
        let before = ui.layout.clone();
        ui.add("nope");
        ui.remove("nope");
        ui.move_up("nope");
        ui.move_down("nope");
        assert_eq!(ui.layout, before);
    }

    #[test]
    fn profiles_save_apply_and_delete() {
        let db = memory_db();
        let mut ui = LayoutUi {
            layout: Layout::default(),
            profiles: Vec::new(),
        };
        ui.remove("pip");
        assert!(ui.save_profile(&db, "Cinema"));
        ui.reset();
        assert!(ui.layout.contains(ControlId::Pip));
        assert!(ui.apply_profile(0));
        assert!(!ui.layout.contains(ControlId::Pip));
        assert!(ui.delete_profile(&db, 0));
        assert!(ui.profiles.is_empty());
    }

    #[test]
    fn empty_profile_names_are_rejected() {
        let db = memory_db();
        let mut ui = LayoutUi {
            layout: Layout::default(),
            profiles: Vec::new(),
        };
        assert!(!ui.save_profile(&db, "   "));
    }

    #[test]
    fn import_rejects_garbage() {
        let db = memory_db();
        let mut ui = LayoutUi {
            layout: Layout::default(),
            profiles: Vec::new(),
        };
        assert!(!ui.import_profile(&db, Path::new(r"D:\nope\missing.json")));
    }
}
