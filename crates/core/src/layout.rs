//! Control-bar layout: which controls exist, in what order, and how a layout is stored.
//!
//! The player renders the trailing cluster from this model, so customising the bar is a data
//! change rather than a UI change. Everything here is plain data and string parsing so a
//! layout can be validated and round-tripped in tests.

use serde::{Deserialize, Serialize};

/// A control that can be placed on the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlId {
    PlayPause,
    Previous,
    Next,
    Stop,
    Seek,
    Time,
    Volume,
    Fullscreen,
    Pip,
    Subtitles,
    Audio,
    Video,
    Playlist,
    Settings,
    Snapshot,
    Repeat,
    Shuffle,
    Speed,
    Chapters,
    Waveform,
}

impl ControlId {
    pub const ALL: &'static [ControlId] = &[
        ControlId::PlayPause,
        ControlId::Previous,
        ControlId::Next,
        ControlId::Stop,
        ControlId::Seek,
        ControlId::Time,
        ControlId::Volume,
        ControlId::Fullscreen,
        ControlId::Pip,
        ControlId::Subtitles,
        ControlId::Audio,
        ControlId::Video,
        ControlId::Playlist,
        ControlId::Settings,
        ControlId::Snapshot,
        ControlId::Repeat,
        ControlId::Shuffle,
        ControlId::Speed,
        ControlId::Chapters,
        ControlId::Waveform,
    ];

    /// Stable id used in JSON and in the i18n key `layout.<id>`.
    pub fn id(self) -> &'static str {
        match self {
            ControlId::PlayPause => "play-pause",
            ControlId::Previous => "previous",
            ControlId::Next => "next",
            ControlId::Stop => "stop",
            ControlId::Seek => "seek",
            ControlId::Time => "time",
            ControlId::Volume => "volume",
            ControlId::Fullscreen => "fullscreen",
            ControlId::Pip => "pip",
            ControlId::Subtitles => "subtitles",
            ControlId::Audio => "audio",
            ControlId::Video => "video",
            ControlId::Playlist => "playlist",
            ControlId::Settings => "settings",
            ControlId::Snapshot => "snapshot",
            ControlId::Repeat => "repeat",
            ControlId::Shuffle => "shuffle",
            ControlId::Speed => "speed",
            ControlId::Chapters => "chapters",
            ControlId::Waveform => "waveform",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.id() == id)
    }

    /// Controls that take real width and therefore cannot be duplicated side by side.
    pub fn is_wide(self) -> bool {
        matches!(
            self,
            ControlId::Seek | ControlId::Time | ControlId::Volume | ControlId::Waveform
        )
    }
}

/// Where a control sits on the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Zone {
    Leading,
    Centre,
    Trailing,
}

/// A bar layout: three zones, each an ordered list of controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub leading: Vec<ControlId>,
    pub centre: Vec<ControlId>,
    pub trailing: Vec<ControlId>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            leading: vec![ControlId::PlayPause, ControlId::Stop],
            centre: vec![ControlId::Previous, ControlId::Next],
            trailing: vec![
                // The shipped bar renders these four; the palette can still offer the rest,
                // but a default must describe buttons the player actually draws.
                ControlId::Audio,
                ControlId::Subtitles,
                ControlId::Settings,
                ControlId::Playlist,
            ],
        }
    }
}

impl Layout {
    pub fn zone(&self, zone: Zone) -> &Vec<ControlId> {
        match zone {
            Zone::Leading => &self.leading,
            Zone::Centre => &self.centre,
            Zone::Trailing => &self.trailing,
        }
    }

    fn zone_mut(&mut self, zone: Zone) -> &mut Vec<ControlId> {
        match zone {
            Zone::Leading => &mut self.leading,
            Zone::Centre => &mut self.centre,
            Zone::Trailing => &mut self.trailing,
        }
    }

    /// Every control currently on the bar, in reading order.
    pub fn all(&self) -> Vec<ControlId> {
        let mut out = Vec::new();
        out.extend(&self.leading);
        out.extend(&self.centre);
        out.extend(&self.trailing);
        out
    }

    pub fn contains(&self, control: ControlId) -> bool {
        self.all().contains(&control)
    }

    /// Add a control at the end of a zone. Returns `false` when it is already placed.
    pub fn add(&mut self, zone: Zone, control: ControlId) -> bool {
        if self.contains(control) {
            return false;
        }
        self.zone_mut(zone).push(control);
        true
    }

    /// Remove a control wherever it is. Returns the zone it was in.
    pub fn remove(&mut self, control: ControlId) -> Option<Zone> {
        for zone in [Zone::Leading, Zone::Centre, Zone::Trailing] {
            if let Some(index) = self.zone(zone).iter().position(|c| *c == control) {
                self.zone_mut(zone).remove(index);
                return Some(zone);
            }
        }
        None
    }

    /// Move a control one slot towards the start of its zone.
    pub fn move_up(&mut self, control: ControlId) -> bool {
        for zone in [Zone::Leading, Zone::Centre, Zone::Trailing] {
            let items = self.zone_mut(zone);
            if let Some(index) = items.iter().position(|c| *c == control) {
                if index == 0 {
                    return false;
                }
                items.swap(index, index - 1);
                return true;
            }
        }
        false
    }

    /// Move a control one slot towards the end of its zone.
    pub fn move_down(&mut self, control: ControlId) -> bool {
        for zone in [Zone::Leading, Zone::Centre, Zone::Trailing] {
            let items = self.zone_mut(zone);
            if let Some(index) = items.iter().position(|c| *c == control) {
                if index + 1 >= items.len() {
                    return false;
                }
                items.swap(index, index + 1);
                return true;
            }
        }
        false
    }

    /// Drop controls that no longer exist and duplicates, and make sure the play button is
    /// present — a bar without it would be unusable.
    pub fn sanitise(&mut self) {
        let mut seen = Vec::new();
        for zone in [Zone::Leading, Zone::Centre, Zone::Trailing] {
            let items = self.zone_mut(zone);
            items.retain(|control| {
                if seen.contains(control) {
                    return false;
                }
                seen.push(*control);
                true
            });
        }
        if !seen.contains(&ControlId::PlayPause) {
            self.leading.insert(0, ControlId::PlayPause);
        }
    }

    /// Slot index for each of `known` that appears in the trailing zone, measured from the
    /// right edge (0 hugs the right edge, increasing values step left), plus the number of
    /// placed controls. A control that is not placed gets slot `-1`.
    ///
    /// This is what the player uses to draw the real bar, so it is pure and tested here.
    pub fn trailing_slots(&self, known: &[ControlId]) -> (Vec<(ControlId, i32)>, i32) {
        let ordered: Vec<ControlId> = self
            .trailing
            .iter()
            .copied()
            .filter(|control| known.contains(control))
            .collect();
        let count = ordered.len() as i32;
        let rows = known
            .iter()
            .map(|control| {
                let slot = ordered
                    .iter()
                    .position(|candidate| candidate == control)
                    .map(|index| count - 1 - index as i32)
                    .unwrap_or(-1);
                (*control, slot)
            })
            .collect();
        (rows, count)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn from_json(text: &str) -> Option<Self> {
        let mut layout: Layout = serde_json::from_str(text).ok()?;
        layout.sanitise();
        Some(layout)
    }
}

/// A named layout + settings snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub layout: Layout,
    /// Settings the profile pins, as `key = value` pairs. Empty means "layout only".
    #[serde(default)]
    pub settings: Vec<(String, String)>,
}

impl Profile {
    pub fn new(name: impl Into<String>, layout: Layout) -> Self {
        Self {
            name: name.into(),
            layout,
            settings: Vec::new(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn from_json(text: &str) -> Option<Self> {
        let mut profile: Profile = serde_json::from_str(text).ok()?;
        if profile.name.trim().is_empty() {
            return None;
        }
        profile.layout.sanitise();
        Some(profile)
    }
}

/// `layout.<id>` i18n key for a control label.
pub fn label_key(control: ControlId) -> String {
    format!("layout.{}", control.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_has_the_play_button() {
        let layout = Layout::default();
        assert!(layout.contains(ControlId::PlayPause));
        assert_eq!(layout.leading.first(), Some(&ControlId::PlayPause));
    }

    #[test]
    fn adding_a_placed_control_is_a_no_op() {
        let mut layout = Layout::default();
        assert!(!layout.add(Zone::Trailing, ControlId::Audio));
        assert!(layout.add(Zone::Trailing, ControlId::Fullscreen));
        assert_eq!(layout.trailing.last(), Some(&ControlId::Fullscreen));
    }

    #[test]
    fn remove_reports_the_zone() {
        let mut layout = Layout::default();
        assert_eq!(layout.remove(ControlId::Stop), Some(Zone::Leading));
        assert_eq!(layout.remove(ControlId::Stop), None);
    }

    #[test]
    fn move_up_stops_at_the_start() {
        let mut layout = Layout::default();
        assert!(layout.move_up(ControlId::Stop));
        assert_eq!(layout.leading, vec![ControlId::Stop, ControlId::PlayPause]);
        assert!(!layout.move_up(ControlId::Stop));
    }

    #[test]
    fn move_down_stops_at_the_end() {
        let mut layout = Layout::default();
        assert!(layout.move_down(ControlId::PlayPause));
        assert_eq!(layout.leading, vec![ControlId::Stop, ControlId::PlayPause]);
        assert!(!layout.move_down(ControlId::PlayPause));
    }

    #[test]
    fn sanitise_removes_duplicates_and_restores_play() {
        let mut layout = Layout {
            leading: vec![ControlId::Stop, ControlId::Stop],
            centre: vec![],
            trailing: vec![ControlId::Stop],
        };
        layout.sanitise();
        assert_eq!(layout.all(), vec![ControlId::PlayPause, ControlId::Stop]);
    }

    #[test]
    fn json_round_trip_keeps_the_order() {
        let mut layout = Layout::default();
        layout.add(Zone::Trailing, ControlId::Waveform);
        let text = layout.to_json();
        let back = Layout::from_json(&text).unwrap();
        assert_eq!(layout, back);
    }

    #[test]
    fn trailing_slots_follow_the_saved_order() {
        let known = [
            ControlId::Audio,
            ControlId::Subtitles,
            ControlId::Settings,
            ControlId::Playlist,
        ];
        let (rows, count) = Layout::default().trailing_slots(&known);
        assert_eq!(count, 4);
        let slot = |control| {
            rows.iter()
                .find(|(candidate, _)| *candidate == control)
                .unwrap()
                .1
        };
        // Default reading order is Audio, Subtitles, Settings, Playlist, so Playlist hugs
        // the right edge and Audio is furthest left.
        assert_eq!(slot(ControlId::Playlist), 0);
        assert_eq!(slot(ControlId::Settings), 1);
        assert_eq!(slot(ControlId::Subtitles), 2);
        assert_eq!(slot(ControlId::Audio), 3);

        // Reversing the order reverses the slots.
        let mut layout = Layout::default();
        layout.trailing.reverse();
        let (rows, _) = layout.trailing_slots(&known);
        let slot = |control| {
            rows.iter()
                .find(|(candidate, _)| *candidate == control)
                .unwrap()
                .1
        };
        assert_eq!(slot(ControlId::Audio), 0);
        assert_eq!(slot(ControlId::Playlist), 3);

        // Removing one control closes the gap instead of leaving an empty slot.
        let mut layout = Layout::default();
        layout.remove(ControlId::Settings);
        let (rows, count) = layout.trailing_slots(&known);
        assert_eq!(count, 3);
        let slot = |control| {
            rows.iter()
                .find(|(candidate, _)| *candidate == control)
                .unwrap()
                .1
        };
        assert_eq!(slot(ControlId::Settings), -1);
        assert_eq!(slot(ControlId::Playlist), 0);
        assert_eq!(slot(ControlId::Audio), 2);
    }

    #[test]
    fn json_uses_kebab_case_ids() {
        let text = Layout::default().to_json();
        assert!(text.contains("play-pause"));
        assert!(text.contains("playlist"));
    }

    #[test]
    fn unknown_ids_are_rejected_cleanly() {
        assert!(
            Layout::from_json("{\"leading\":[\"nope\"],\"centre\":[],\"trailing\":[]}").is_none()
        );
        assert!(ControlId::from_id("nope").is_none());
    }

    #[test]
    fn profile_round_trips_and_rejects_empty_names() {
        let profile = Profile::new("Cinema", Layout::default());
        let back = Profile::from_json(&profile.to_json()).unwrap();
        assert_eq!(back.name, "Cinema");
        assert!(Profile::from_json(
            "{\"name\":\"  \",\"layout\":{\"leading\":[],\"centre\":[],\"trailing\":[]}}"
        )
        .is_none());
    }

    #[test]
    fn every_control_has_a_label_key() {
        for control in ControlId::ALL {
            assert_eq!(label_key(*control), format!("layout.{}", control.id()));
        }
    }
}
