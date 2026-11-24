//! Phase 7 glue: chapters, bookmarks, the waveform strip and the metadata panel.
//!
//! Everything here is a pure conversion from domain values to the structs the Slint
//! sidebar and seek bar expect, so the row builders can be unit-tested without a window.

use slint::{ModelRc, SharedString, VecModel};
use vfm_core::{format_timecode, Bookmark, Language, Waveform};

use crate::{i18n, MetadataRow, PlayerWindow, TimelineRow};
use vlc_wrapper::{Chapter, MediaMetadata, TrackSummary};

/// Bars drawn inside the seek bar. The stored waveform keeps 900 buckets for zooming,
/// but the bar is only a few hundred pixels wide, so it is downsampled before rendering.
pub const PATH_BUCKETS: usize = 320;

fn s(text: impl Into<String>) -> SharedString {
    SharedString::from(text.into())
}

/// Envelope for the seek bar: downsampled to one entry per bar.
///
/// In RTL the list is reversed so bucket 0 still sits at the start of the media, which the
/// seek bar draws from the right.
pub fn waveform_peaks_for(waveform: &Waveform, rtl: bool) -> Vec<f32> {
    if waveform.is_empty() {
        return Vec::new();
    }
    let mut peaks = vfm_core::downsample_waveform(&waveform.peaks, PATH_BUCKETS);
    if rtl {
        peaks.reverse();
    }
    peaks
}

/// 0..1 positions of every bookmark, for the ticks drawn over the seek bar.
pub fn bookmark_markers(bookmarks: &[Bookmark], duration_ms: i64) -> Vec<f32> {
    if duration_ms <= 0 {
        return Vec::new();
    }
    bookmarks
        .iter()
        .map(|item| (item.time_ms as f32 / duration_ms as f32).clamp(0.0, 1.0))
        .collect()
}

/// Chapter rows, marking the chapter that currently contains `position_ms`.
pub fn chapter_rows(chapters: &[Chapter], duration_ms: i64, position_ms: i64) -> Vec<TimelineRow> {
    let total = duration_ms.max(1) as f32;
    chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            let active = position_ms >= chapter.start_ms
                && (chapter.duration_ms <= 0
                    || position_ms < chapter.start_ms + chapter.duration_ms);
            TimelineRow {
                // Unnamed chapters are numbered rather than left blank; the timecode
                // column already carries the timestamp.
                name: s(if chapter.name.trim().is_empty() {
                    format!("{}.", index + 1)
                } else {
                    chapter.name.trim().to_string()
                }),
                time: s(format_timecode(chapter.start_ms)),
                position: (chapter.start_ms as f32 / total).clamp(0.0, 1.0),
                active,
            }
        })
        .collect()
}

/// Bookmark rows in time order. Selection is tracked by index in the panel, so no row is
/// marked active here.
pub fn bookmark_rows(bookmarks: &[Bookmark], duration_ms: i64) -> Vec<TimelineRow> {
    let total = duration_ms.max(1) as f32;
    bookmarks
        .iter()
        .map(|item| TimelineRow {
            name: s(item.display_name()),
            time: s(format_timecode(item.time_ms)),
            position: (item.time_ms as f32 / total).clamp(0.0, 1.0),
            active: false,
        })
        .collect()
}

/// Index of the bookmark row matching `bookmark`, used after a rename or a delete.
pub fn index_of(bookmarks: &[Bookmark], id: i64) -> i32 {
    bookmarks
        .iter()
        .position(|item| item.id == id)
        .map(|index| index as i32)
        .unwrap_or(-1)
}

fn push(rows: &mut Vec<MetadataRow>, label: &str, value: &str) {
    if !value.trim().is_empty() {
        rows.push(MetadataRow {
            label: s(label),
            value: s(value.trim()),
        });
    }
}

/// Labels and values for the metadata panel. Only populated fields are emitted so the
/// panel collapses to nothing for files without tags.
pub fn metadata_rows(
    meta: &MediaMetadata,
    tracks: &TrackSummary,
    language: Language,
) -> Vec<MetadataRow> {
    let mut rows = Vec::new();
    // Keys are literals so `translate` yields `&'static str`; a closure would tie the
    // returned reference to the (short-lived) argument instead.
    push(&mut rows, "Title", &meta.title);
    push(
        &mut rows,
        i18n::translate(language, "metadata.show"),
        &meta.show_name,
    );
    push(
        &mut rows,
        i18n::translate(language, "metadata.season"),
        &meta.season,
    );
    push(
        &mut rows,
        i18n::translate(language, "metadata.episode"),
        &meta.episode,
    );
    push(&mut rows, "Artist", &meta.artist);
    push(&mut rows, "Album", &meta.album);
    push(&mut rows, "Album artist", &meta.album_artist);
    push(&mut rows, "Genre", &meta.genre);
    push(&mut rows, "Date", &meta.date);
    push(&mut rows, "Publisher", &meta.publisher);
    push(
        &mut rows,
        i18n::translate(language, "metadata.actors"),
        &meta.actors,
    );
    push(&mut rows, "Language", &meta.language);

    if let Some(video) = &tracks.video {
        let mut value = format!("{}×{}", video.width, video.height);
        if video.frame_rate > 0.1 {
            value.push_str(&format!(" · {:.2} fps", video.frame_rate));
        }
        if video.bitrate > 0 {
            value.push_str(&format!(" · {} kb/s", video.bitrate / 1000));
        }
        rows.push(MetadataRow {
            label: s(i18n::translate(language, "metadata.video")),
            value: s(value),
        });
    }
    for (index, audio) in tracks.audio.iter().enumerate() {
        let mut value = format!("{} ch · {} Hz", audio.channels, audio.rate);
        if audio.bitrate > 0 {
            value.push_str(&format!(" · {} kb/s", audio.bitrate / 1000));
        }
        let label = if tracks.audio.len() > 1 {
            format!(
                "{} {}",
                i18n::translate(language, "metadata.audio"),
                index + 1
            )
        } else {
            i18n::translate(language, "metadata.audio").to_string()
        };
        rows.push(MetadataRow {
            label: s(label),
            value: s(value),
        });
    }
    rows
}

// ---- UI push helpers ----

pub fn push_chapter_rows(ui: &PlayerWindow, rows: Vec<TimelineRow>) {
    ui.set_chapter_rows(ModelRc::new(VecModel::from(rows)));
}

pub fn push_bookmark_rows(ui: &PlayerWindow, rows: Vec<TimelineRow>) {
    ui.set_bookmark_rows(ModelRc::new(VecModel::from(rows)));
}

pub fn push_metadata_rows(ui: &PlayerWindow, rows: Vec<MetadataRow>) {
    ui.set_metadata_rows(ModelRc::new(VecModel::from(rows)));
}

pub fn push_markers(ui: &PlayerWindow, markers: Vec<f32>) {
    ui.set_seek_markers(ModelRc::new(VecModel::from(markers)));
}

pub fn push_waveform(ui: &PlayerWindow, peaks: Vec<f32>) {
    ui.set_waveform_peaks(ModelRc::new(VecModel::from(peaks)));
}

pub fn clear_timeline(ui: &PlayerWindow) {
    push_chapter_rows(ui, Vec::new());
    push_bookmark_rows(ui, Vec::new());
    push_metadata_rows(ui, Vec::new());
    push_markers(ui, Vec::new());
    ui.set_waveform_peaks(ModelRc::new(VecModel::from(Vec::<f32>::new())));
    ui.set_waveform_visible(false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn chapter(start_ms: i64, duration_ms: i64, name: &str) -> Chapter {
        Chapter {
            start_ms,
            duration_ms,
            name: name.into(),
        }
    }

    fn bookmark(id: i64, time_ms: i64, name: &str) -> Bookmark {
        Bookmark {
            id,
            media: PathBuf::from("D:/Shows/ep.mkv"),
            time_ms,
            name: name.into(),
        }
    }

    #[test]
    fn an_empty_waveform_produces_no_bars() {
        let empty = Waveform {
            peaks: Vec::new(),
            duration_ms: 0,
        };
        assert!(waveform_peaks_for(&empty, false).is_empty());
    }

    #[test]
    fn the_envelope_is_downsampled_to_the_bar_count() {
        let waveform = Waveform {
            peaks: vec![0.5; 900],
            duration_ms: 180_000,
        };
        assert_eq!(waveform_peaks_for(&waveform, false).len(), PATH_BUCKETS);
    }

    #[test]
    fn rtl_reverses_the_envelope() {
        let mut peaks = vec![0.1; PATH_BUCKETS];
        peaks[0] = 1.0;
        let waveform = Waveform {
            peaks,
            duration_ms: 180_000,
        };
        let ltr = waveform_peaks_for(&waveform, false);
        let rtl = waveform_peaks_for(&waveform, true);
        assert!(ltr[0] > ltr[ltr.len() - 1]);
        assert!(
            rtl[0] < rtl[rtl.len() - 1],
            "RTL must put bucket 0 at the end"
        );
    }

    #[test]
    fn markers_are_fractions_of_the_duration() {
        let items = vec![bookmark(1, 30_000, "a"), bookmark(2, 90_000, "b")];
        assert_eq!(bookmark_markers(&items, 120_000), vec![0.25, 0.75]);
    }

    #[test]
    fn markers_need_a_known_duration() {
        let items = vec![bookmark(1, 30_000, "a")];
        assert!(bookmark_markers(&items, 0).is_empty());
    }

    #[test]
    fn chapters_number_themselves_when_the_file_has_no_names() {
        let rows = chapter_rows(
            &[chapter(0, 60_000, "  "), chapter(60_000, 60_000, "Finale")],
            120_000,
            0,
        );
        assert_eq!(rows[0].name.as_str(), "1.");
        assert_eq!(rows[0].time.as_str(), "0:00");
        assert_eq!(rows[1].name.as_str(), "Finale");
        assert_eq!(rows[1].time.as_str(), "1:00");
    }

    #[test]
    fn only_the_chapter_holding_the_playhead_is_active() {
        let chapters = [chapter(0, 60_000, "a"), chapter(60_000, 60_000, "b")];
        let rows = chapter_rows(&chapters, 120_000, 65_000);
        assert!(!rows[0].active);
        assert!(rows[1].active);
    }

    #[test]
    fn the_last_chapter_stays_active_at_the_very_end() {
        let chapters = [chapter(0, 60_000, "a"), chapter(60_000, 0, "b")];
        let rows = chapter_rows(&chapters, 120_000, 119_000);
        assert!(
            rows[1].active,
            "an open-ended last chapter must not deactivate"
        );
    }

    #[test]
    fn bookmark_rows_show_the_label_and_the_timecode() {
        let rows = bookmark_rows(&[bookmark(7, 125_000, "")], 500_000);
        assert_eq!(rows[0].name.as_str(), "2:05");
        assert_eq!(rows[0].time.as_str(), "2:05");
        assert!((rows[0].position - 0.25).abs() < 0.001);
    }

    #[test]
    fn a_deleted_bookmark_loses_its_index() {
        let items = vec![bookmark(1, 10_000, "a"), bookmark(2, 20_000, "b")];
        assert_eq!(index_of(&items, 2), 1);
        assert_eq!(index_of(&items, 99), -1);
    }

    #[test]
    fn metadata_rows_skip_empty_fields() {
        let meta = MediaMetadata {
            title: "Pilot".into(),
            show_name: "Solaris".into(),
            ..Default::default()
        };
        let rows = metadata_rows(&meta, &TrackSummary::default(), Language::En);
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, vec!["Title", "Show"]);
    }

    #[test]
    fn metadata_rows_localise_the_show_label() {
        let meta = MediaMetadata {
            show_name: "Solaris".into(),
            ..Default::default()
        };
        let rows = metadata_rows(&meta, &TrackSummary::default(), Language::Ar);
        assert_eq!(rows[0].label.as_str(), "المسلسل");
    }

    #[test]
    fn track_summaries_become_video_and_audio_rows() {
        let tracks = TrackSummary {
            video: Some(vlc_wrapper::VideoTrackInfo {
                codec: 0,
                width: 1920,
                height: 1080,
                frame_rate: 23.976,
                bitrate: 4_500_000,
            }),
            audio: vec![
                vlc_wrapper::AudioTrackInfo {
                    codec: 0,
                    channels: 6,
                    rate: 48_000,
                    bitrate: 448_000,
                },
                vlc_wrapper::AudioTrackInfo {
                    codec: 0,
                    channels: 2,
                    rate: 48_000,
                    bitrate: 192_000,
                },
            ],
        };
        let rows = metadata_rows(&MediaMetadata::default(), &tracks, Language::En);
        assert_eq!(rows[0].value.as_str(), "1920×1080 · 23.98 fps · 4500 kb/s");
        assert_eq!(rows[1].label.as_str(), "Audio 1");
        assert_eq!(rows[1].value.as_str(), "6 ch · 48000 Hz · 448 kb/s");
        assert_eq!(rows[2].label.as_str(), "Audio 2");
    }
}
