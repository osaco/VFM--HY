//! Subtitle parsing, styling and timing. Pure data model — rendering happens in the UI layer.
//! Supports SRT and WebVTT fully, plus the common subset of ASS/SSA styling.

use std::path::Path;

/// Subtitle file formats recognised by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    Srt,
    WebVtt,
    Ass,
    Ssa,
}

impl SubtitleFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "srt" => Some(Self::Srt),
            "vtt" => Some(Self::WebVtt),
            "ass" => Some(Self::Ass),
            "ssa" => Some(Self::Ssa),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Srt => "SRT",
            Self::WebVtt => "WebVTT",
            Self::Ass => "ASS",
            Self::Ssa => "SSA",
        }
    }
}

/// Styling for one cue. `font_size_px = None` means use `relative_size` against a base size.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleStyle {
    pub font_family: String,
    pub font_size_px: Option<f32>,
    pub relative_size: f32,
    pub color: (u8, u8, u8),
    pub outline: f32,
    pub outline_color: (u8, u8, u8),
    pub bold: bool,
    pub italic: bool,
    /// 0.0 = bottom of screen, 1.0 = top.
    pub position: f32,
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            font_family: "Inter".into(),
            font_size_px: None,
            relative_size: 1.0,
            color: (255, 255, 255),
            outline: 1.0,
            outline_color: (0, 0, 0),
            bold: false,
            italic: false,
            position: 0.1,
        }
    }
}

impl SubtitleStyle {
    pub fn scaled_size(&self, base_px: f32) -> f32 {
        match self.font_size_px {
            Some(px) => px,
            None => base_px * self.relative_size,
        }
    }
}

/// One timed subtitle cue.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub index: usize,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub style: SubtitleStyle,
}

impl Cue {
    pub fn contains(&self, time_ms: i64) -> bool {
        self.start_ms <= time_ms && time_ms < self.end_ms
    }

    pub fn duration_ms(&self) -> i64 {
        (self.end_ms - self.start_ms).max(0)
    }
}

/// Parse an SRT/VTT-ish timestamp into milliseconds. Accepts `HH:MM:SS,mmm`,
/// `HH:MM:SS.mmm` and `MM:SS,mmm` / `MM:SS.mmm`.
fn parse_timestamp(input: &str) -> Option<i64> {
    let input = input.trim();
    let (time, fraction) = input.rsplit_once([',', '.']).unwrap_or((input, "0"));
    let parts: Vec<&str> = time.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut values = [0i64; 3];
    for (i, part) in parts.iter().enumerate() {
        values[i] = part.trim().parse::<i64>().ok()?;
    }
    // Two parts = MM:SS, three parts = HH:MM:SS.
    let (hours, minutes, seconds) = if parts.len() == 2 {
        (0, values[0], values[1])
    } else {
        (values[0], values[1], values[2])
    };
    if minutes >= 60 || seconds >= 60 {
        return None;
    }
    let millis: i64 = if fraction.is_empty() {
        0
    } else {
        let truncated: String = fraction.chars().take(3).collect();
        let mut m = truncated.parse::<i64>().ok()?;
        for _ in truncated.len()..3 {
            m *= 10;
        }
        m
    };
    Some(hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + millis)
}

pub fn parse_timestamp_srt(s: &str) -> Option<i64> {
    parse_timestamp(s)
}

pub fn parse_timestamp_vtt(s: &str) -> Option<i64> {
    parse_timestamp(s)
}

fn split_arrow(line: &str) -> Option<(&str, &str)> {
    let (left, right) = line.split_once("-->")?;
    Some((left.trim(), right.trim()))
}

fn strip_vtt_settings(tail: &str) -> &str {
    // "00:01.000 --> 00:03.000 align:start position:50%" — settings follow whitespace.
    tail.split_whitespace().next().unwrap_or("")
}

fn push_cue(cues: &mut Vec<Cue>, start: i64, end: i64, lines: &[String]) {
    if lines.is_empty() || end < start {
        return;
    }
    let text = lines.join("\n").trim().to_string();
    if text.is_empty() {
        return;
    }
    let index = cues.len() + 1;
    cues.push(Cue {
        index,
        start_ms: start,
        end_ms: end,
        text,
        style: SubtitleStyle::default(),
    });
}

/// Parse a SubRip file. Malformed blocks are skipped rather than aborting the parse.
pub fn parse_srt(input: &str) -> Vec<Cue> {
    let input = input.trim_start_matches('\u{feff}');
    let mut cues = Vec::new();
    let mut block: Vec<String> = Vec::new();

    let flush = |block: &mut Vec<String>, cues: &mut Vec<Cue>| {
        if block.len() < 2 {
            block.clear();
            return;
        }
        // Locate the timestamp line; the line before it is the optional numeric index.
        let ts_pos = block
            .iter()
            .position(|l| l.contains("-->"))
            .unwrap_or(usize::MAX);
        if ts_pos == usize::MAX {
            block.clear();
            return;
        }
        if let Some((a, b)) = split_arrow(&block[ts_pos]) {
            if let (Some(start), Some(end)) = (parse_timestamp(a), parse_timestamp(b)) {
                push_cue(cues, start, end, &block[ts_pos + 1..]);
            }
        }
        block.clear();
    };

    for raw in input.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            flush(&mut block, &mut cues);
        } else {
            block.push(line.to_string());
        }
    }
    flush(&mut block, &mut cues);
    cues
}

/// Parse a WebVTT file, including cue identifiers and cue settings.
pub fn parse_webvtt(input: &str) -> Vec<Cue> {
    let input = input.trim_start_matches('\u{feff}');
    let mut cues = Vec::new();
    let mut block: Vec<String> = Vec::new();
    let mut in_note = false;
    let mut in_style = false;

    let flush = |block: &mut Vec<String>, cues: &mut Vec<Cue>| {
        let ts_pos = block
            .iter()
            .position(|l| l.contains("-->"))
            .unwrap_or(usize::MAX);
        if ts_pos == usize::MAX {
            block.clear();
            return;
        }
        if let Some((a, b)) = split_arrow(&block[ts_pos]) {
            let b = strip_vtt_settings(b);
            if let (Some(start), Some(end)) = (parse_timestamp(a), parse_timestamp(b)) {
                push_cue(cues, start, end, &block[ts_pos + 1..]);
            }
        }
        block.clear();
    };

    for raw in input.lines() {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.starts_with("WEBVTT") || trimmed.starts_with("STYLE") {
            in_style = true;
            continue;
        }
        if trimmed.starts_with("NOTE") {
            in_note = true;
            continue;
        }
        if in_style || in_note {
            if trimmed.is_empty() {
                in_style = false;
                in_note = false;
            }
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut block, &mut cues);
        } else {
            block.push(line.to_string());
        }
    }
    flush(&mut block, &mut cues);
    cues
}

/// Parse an ASS colour of the form `&HAABBGGRR` or `&HBBGGRR`. Returns (R, G, B).
fn parse_ass_color(value: &str) -> Option<(u8, u8, u8)> {
    let hex = value.trim().trim_start_matches('&').trim_start_matches('H');
    if hex.len() < 6 {
        return None;
    }
    let tail = &hex[hex.len() - 6..];
    let b = u8::from_str_radix(&tail[0..2], 16).ok()?;
    let g = u8::from_str_radix(&tail[2..4], 16).ok()?;
    let r = u8::from_str_radix(&tail[4..6], 16).ok()?;
    Some((r, g, b))
}

fn parse_ass_timestamp(value: &str) -> Option<i64> {
    // H:MM:SS.cc (centiseconds)
    let value = value.trim();
    let (time, centis) = value.split_once('.')?;
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let s: i64 = parts[2].parse().ok()?;
    let cs: i64 = centis.parse().ok()?;
    Some(h * 3_600_000 + m * 60_000 + s * 1_000 + cs * 10)
}

#[derive(Default)]
struct AssStyle {
    name: String,
    style: SubtitleStyle,
}

/// Parse the commonly used subset of ASS/SSA: script styles, Dialogue events and basic
/// inline `{\\i1}` / `{\\b1}` overrides. Unrecognised fields are ignored safely.
pub fn parse_ass(input: &str) -> Vec<Cue> {
    let input = input.trim_start_matches('\u{feff}');
    let mut styles: Vec<AssStyle> = Vec::new();
    let mut events: Vec<(i64, i64, String, String)> = Vec::new();
    let mut section = "";
    let mut style_format: Vec<String> = Vec::new();
    let mut event_format: Vec<String> = Vec::new();

    for raw in input.lines() {
        let line = raw.trim_end_matches('\r').trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line;
            continue;
        }
        if let Some(rest) = line.strip_prefix("Format:") {
            let fields: Vec<String> = rest.split(',').map(|s| s.trim().to_string()).collect();
            if section.eq_ignore_ascii_case("[V4+ Styles]")
                || section.eq_ignore_ascii_case("[V4 Styles]")
            {
                style_format = fields;
            } else if section.eq_ignore_ascii_case("[Events]") {
                event_format = fields;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("Style:") {
            if style_format.is_empty() {
                continue;
            }
            let values: Vec<&str> = rest.splitn(style_format.len(), ',').collect();
            let field = |name: &str| -> Option<&str> {
                style_format
                    .iter()
                    .position(|f| f.eq_ignore_ascii_case(name))
                    .and_then(|i| values.get(i).copied())
            };
            let mut style = SubtitleStyle::default();
            if let Some(font) = field("Fontname") {
                style.font_family = font.trim().to_string();
            }
            if let Some(size) = field("Fontsize").and_then(|s| s.trim().parse::<f32>().ok()) {
                style.font_size_px = Some(size);
            }
            if let Some(color) = field("PrimaryColour").and_then(parse_ass_color) {
                style.color = color;
            }
            if let Some(color) = field("OutlineColour").and_then(parse_ass_color) {
                style.outline_color = color;
            }
            if let Some(bold) = field("Bold") {
                style.bold = bold.trim() == "-1" || bold.trim() == "1";
            }
            if let Some(italic) = field("Italic") {
                style.italic = italic.trim() == "-1" || italic.trim() == "1";
            }
            if let Some(outline) = field("Outline").and_then(|s| s.trim().parse::<f32>().ok()) {
                style.outline = outline;
            }
            let name = field("Name").unwrap_or("Default").trim().to_string();
            styles.push(AssStyle { name, style });
            continue;
        }
        if let Some(rest) = line.strip_prefix("Dialogue:") {
            if event_format.is_empty() {
                continue;
            }
            // Text is the last field and may contain commas, so split with a limit.
            let values: Vec<&str> = rest.splitn(event_format.len(), ',').collect();
            let field = |name: &str| -> Option<&str> {
                event_format
                    .iter()
                    .position(|f| f.eq_ignore_ascii_case(name))
                    .and_then(|i| values.get(i).copied())
            };
            let (Some(start), Some(end)) = (
                field("Start").and_then(parse_ass_timestamp),
                field("End").and_then(parse_ass_timestamp),
            ) else {
                continue;
            };
            let style_name = field("Style").unwrap_or("Default").trim().to_string();
            let mut text = field("Text").unwrap_or("").trim().to_string();
            if text.contains("{\\i1}") {
                text = text.replace("{\\i1}", "");
                if let Some(s) = styles.iter_mut().find(|s| s.name == style_name) {
                    s.style.italic = true;
                }
            }
            text = text.replace("{\\i0}", "").replace("{\\b0}", "");
            events.push((start, end, style_name, text));
        }
    }

    events
        .into_iter()
        .filter_map(|(start, end, style_name, text)| {
            if text.is_empty() || end < start {
                return None;
            }
            let style = styles
                .iter()
                .find(|s| s.name == style_name)
                .map(|s| s.style.clone())
                .unwrap_or_default();
            let index = 0; // assigned below
            Some(Cue {
                index,
                start_ms: start,
                end_ms: end,
                text,
                style,
            })
        })
        .enumerate()
        .map(|(i, mut cue)| {
            cue.index = i + 1;
            cue
        })
        .collect()
}

pub fn parse(input: &str, format: SubtitleFormat) -> Vec<Cue> {
    match format {
        SubtitleFormat::Srt => parse_srt(input),
        SubtitleFormat::WebVtt => parse_webvtt(input),
        SubtitleFormat::Ass | SubtitleFormat::Ssa => parse_ass(input),
    }
}

/// Shift every cue by `delta_ms`, clamping start/end to >= 0.
pub fn shift_cues(cues: &mut [Cue], delta_ms: i64) {
    for cue in cues {
        cue.start_ms = (cue.start_ms + delta_ms).max(0);
        cue.end_ms = (cue.end_ms + delta_ms).max(0);
    }
}

/// Binary search for the active cue at `time_ms`. Requires cues sorted by `start_ms`.
pub fn cue_at(cues: &[Cue], time_ms: i64) -> Option<&Cue> {
    let mut low = 0usize;
    let mut high = cues.len();
    while low < high {
        let mid = (low + high) / 2;
        if cues[mid].start_ms <= time_ms {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    if low == 0 {
        return None;
    }
    let candidate = &cues[low - 1];
    candidate.contains(time_ms).then_some(candidate)
}

/// Detect text encoding from a BOM. Defaults to UTF-8.
pub fn detect_encoding(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        "utf-8-bom"
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        "utf-16le"
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        "utf-16be"
    } else {
        "utf-8"
    }
}

/// Decode subtitle bytes using BOM detection, falling back to lossy UTF-8.
pub fn decode_subtitle_bytes(bytes: &[u8]) -> String {
    match detect_encoding(bytes) {
        "utf-8-bom" => String::from_utf8_lossy(&bytes[3..]).into_owned(),
        "utf-16le" | "utf-16be" => {
            let little = detect_encoding(bytes) == "utf-16le";
            let (words, _): (&[[u8; 2]], &[u8]) = bytes[2..].as_chunks::<2>();
            let words: Vec<u16> = words
                .iter()
                .map(|pair| {
                    if little {
                        u16::from_le_bytes(*pair)
                    } else {
                        u16::from_be_bytes(*pair)
                    }
                })
                .collect();
            String::from_utf16_lossy(&words)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// A cue plus its absolute timing after the session's live delay is applied.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedCue {
    pub index: usize,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub style: SubtitleStyle,
}

/// Runtime subtitle session: an ordered cue list plus live delay/style overrides.
/// The UI queries `active_at()` on each clock tick; changing delay or style never
/// restarts playback (Phase 5 acceptance).
#[derive(Debug, Clone)]
pub struct SubtitleSession {
    cues: Vec<Cue>,
    pub delay_ms: i64,
    /// Multiplies every cue's relative size. 1.0 = as authored.
    pub size_scale: f32,
    /// Added to every cue's vertical position, clamped to 0..=1.
    pub position_offset: f32,
    /// When true, ignore per-cue sizes and use `size_scale` against `base_size_px`.
    pub override_size: bool,
    pub base_size_px: f32,
    pub enabled: bool,
}

impl Default for SubtitleSession {
    fn default() -> Self {
        Self {
            cues: Vec::new(),
            delay_ms: 0,
            size_scale: 1.0,
            position_offset: 0.0,
            override_size: false,
            base_size_px: 28.0,
            enabled: true,
        }
    }
}

impl SubtitleSession {
    pub fn new(cues: Vec<Cue>) -> Self {
        let mut sorted = cues;
        sorted.sort_by_key(|c| c.start_ms);
        Self {
            cues: sorted,
            ..Self::default()
        }
    }

    pub fn from_text(text: &str, format: SubtitleFormat) -> Self {
        Self::new(parse(text, format))
    }

    pub fn from_bytes(bytes: &[u8], format: SubtitleFormat) -> Self {
        Self::from_text(&decode_subtitle_bytes(bytes), format)
    }

    pub fn len(&self) -> usize {
        self.cues.len()
    }
    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }
    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }

    /// Replace the cue list (e.g. loading a different external track).
    pub fn set_cues(&mut self, cues: Vec<Cue>) {
        let mut cues = cues;
        cues.sort_by_key(|c| c.start_ms);
        self.cues = cues;
    }

    /// The cue visible at media time `time_ms`, with delay and style overrides applied.
    /// Returns `None` when subtitles are disabled, the list is empty, or there is a gap.
    pub fn active_at(&self, time_ms: i64) -> Option<TimedCue> {
        if !self.enabled {
            return None;
        }
        let shifted = time_ms - self.delay_ms;
        let source = cue_at(&self.cues, shifted)?;
        let mut style = source.style.clone();
        if self.override_size {
            style.font_size_px = None;
            style.relative_size = self.size_scale;
        } else {
            style.relative_size *= self.size_scale;
        }
        style.position = (style.position + self.position_offset).clamp(0.0, 1.0);
        Some(TimedCue {
            index: source.index,
            start_ms: source.start_ms + self.delay_ms,
            end_ms: source.end_ms + self.delay_ms,
            text: source.text.clone(),
            style,
        })
    }

    /// Shift timing by `delta_ms` (positive = subtitles appear later).
    pub fn shift(&mut self, delta_ms: i64) {
        shift_cues(&mut self.cues, delta_ms);
    }

    /// Clear timing delay while keeping style overrides.
    pub fn reset_delay(&mut self) {
        self.delay_ms = 0;
    }
}

/// File-name based matching between a video and an external subtitle, used by the
/// "Add subtitles" auto-match feature. Returns true when the subtitle's stem starts with
/// the video's stem (case-insensitive), optionally followed by a language suffix.
pub fn subtitle_matches_video(video: &Path, subtitle: &Path) -> bool {
    let v = video
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let s = subtitle
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if v.is_empty() || s.is_empty() {
        return false;
    }
    s == v || s.starts_with(&format!("{v}.")) || s.starts_with(&format!("{v}_"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats_by_extension() {
        assert_eq!(
            SubtitleFormat::from_path(Path::new("a.SRT")),
            Some(SubtitleFormat::Srt)
        );
        assert_eq!(
            SubtitleFormat::from_path(Path::new("a.vtt")),
            Some(SubtitleFormat::WebVtt)
        );
        assert_eq!(
            SubtitleFormat::from_path(Path::new("a.ass")),
            Some(SubtitleFormat::Ass)
        );
        assert_eq!(
            SubtitleFormat::from_path(Path::new("a.ssa")),
            Some(SubtitleFormat::Ssa)
        );
        assert_eq!(SubtitleFormat::from_path(Path::new("a.txt")), None);
        for f in [
            SubtitleFormat::Srt,
            SubtitleFormat::WebVtt,
            SubtitleFormat::Ass,
            SubtitleFormat::Ssa,
        ] {
            assert!(!f.name().is_empty());
        }
    }

    #[test]
    fn parses_srt_timestamps() {
        assert_eq!(parse_timestamp_srt("00:00:01,500"), Some(1_500));
        assert_eq!(parse_timestamp_srt("01:02:03,004"), Some(3_723_004));
        assert_eq!(parse_timestamp_srt("01:02.250"), Some(62_250));
        assert_eq!(parse_timestamp_srt("00:00:01.100"), Some(1_100));
        assert_eq!(parse_timestamp_srt("nonsense"), None);
    }

    #[test]
    fn parses_srt_with_crlf_and_index_noise() {
        let text = "\u{feff}1\r\n00:00:01,000 --> 00:00:03,000\r\nHello\r\nworld\r\n\r\n2\r\n00:00:05,000 --> 00:00:06,500\r\nSecond\r\n";
        let cues = parse_srt(text);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Hello\nworld");
        assert_eq!(cues[0].start_ms, 1_000);
        assert_eq!(cues[1].index, 2);
        assert_eq!(cues[1].end_ms, 6_500);
    }

    #[test]
    fn parses_webvtt_with_settings_and_identifiers() {
        let text = "WEBVTT\n\nintro\n00:00:01.000 --> 00:00:03.000 align:start position:50%\nWelcome\n\n00:01.500 --> 00:03.000\nNext\n";
        let cues = parse_webvtt(text);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Welcome");
        assert_eq!(cues[0].start_ms, 1_000);
        assert_eq!(cues[1].start_ms, 1_500);
        assert_eq!(cues[1].text, "Next");
    }

    #[test]
    fn webvtt_skips_note_and_style_blocks() {
        let text = "WEBVTT\n\nNOTE this is a comment\nmore\n\nSTYLE\n::cue { color: lime }\n\n00:00:01.000 --> 00:00:02.000\nShown\n";
        let cues = parse_webvtt(text);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Shown");
    }

    #[test]
    fn parses_ass_with_styles_and_inline_tags() {
        let text = "[Script Info]\nScriptType: v4.00+\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,24,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.50,Default,,0,0,0,,{\\i1}Hello world\nDialogue: 0,0:00:05.00,0:00:06.00,Default,,0,0,0,,Plain\n";
        let cues = parse_ass(text);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_ms, 1_000);
        assert_eq!(cues[0].end_ms, 3_500);
        assert_eq!(cues[0].text, "Hello world");
        assert_eq!(cues[0].style.font_family, "Arial");
        assert_eq!(cues[0].style.font_size_px, Some(24.0));
        assert!(cues[0].style.italic);
        assert_eq!(cues[0].style.color, (255, 255, 255));
        assert_eq!(cues[1].text, "Plain");
    }

    #[test]
    fn malformed_ass_lines_never_panic() {
        let text = "[Events]\nFormat: Layer,Start,End,Style,Text\nDialogue: short\nDialogue: 0,bad,also bad,Default,text\n";
        assert!(parse_ass(text).is_empty());
    }

    #[test]
    fn shift_cues_clamps_to_zero() {
        let mut cues = parse_srt("1\n00:00:10,000 --> 00:00:12,000\nText\n");
        shift_cues(&mut cues, -15_000);
        assert_eq!(cues[0].start_ms, 0);
        assert_eq!(cues[0].end_ms, 0);
        shift_cues(&mut cues, 2_000);
        assert_eq!(cues[0].start_ms, 2_000);
    }

    #[test]
    fn cue_at_finds_active_cue_and_gaps() {
        let cues = parse_srt(
            "1\n00:00:01,000 --> 00:00:03,000\nA\n\n2\n00:00:05,000 --> 00:00:07,000\nB\n",
        );
        assert_eq!(cue_at(&cues, 0), None);
        assert_eq!(cue_at(&cues, 2_000).unwrap().text, "A");
        assert_eq!(cue_at(&cues, 3_000), None);
        assert_eq!(cue_at(&cues, 6_000).unwrap().text, "B");
    }

    #[test]
    fn detects_and_decodes_encodings() {
        assert_eq!(detect_encoding(b"plain"), "utf-8");
        assert_eq!(detect_encoding(&[0xEF, 0xBB, 0xBF, b'a']), "utf-8-bom");
        assert_eq!(decode_subtitle_bytes(&[0xEF, 0xBB, 0xBF, b'h', b'i']), "hi");
        let mut utf16 = vec![0xFF, 0xFE];
        for c in "hello".encode_utf16() {
            utf16.extend_from_slice(&c.to_le_bytes());
        }
        assert_eq!(detect_encoding(&utf16), "utf-16le");
        assert_eq!(decode_subtitle_bytes(&utf16), "hello");
        let mut be = vec![0xFE, 0xFF];
        for c in "hi".encode_utf16() {
            be.extend_from_slice(&c.to_be_bytes());
        }
        assert_eq!(decode_subtitle_bytes(&be), "hi");
    }

    #[test]
    fn style_scales_precise_and_relative() {
        let mut style = SubtitleStyle::default();
        assert_eq!(style.scaled_size(20.0), 20.0);
        style.relative_size = 1.5;
        assert_eq!(style.scaled_size(20.0), 30.0);
        style.font_size_px = Some(28.0);
        assert_eq!(style.scaled_size(20.0), 28.0);
    }

    #[test]
    fn empty_inputs_are_safe() {
        assert!(parse_srt("").is_empty());
        assert!(parse_webvtt("").is_empty());
        assert!(parse_ass("").is_empty());
        assert_eq!(decode_subtitle_bytes(&[]), "");
    }

    #[test]
    fn subtitle_session_applies_delay_without_side_effects() {
        let text =
            "1\n00:00:01,000 --> 00:00:03,000\nHello\n\n2\n00:00:05,000 --> 00:00:07,000\nWorld\n";
        let mut session = SubtitleSession::from_text(text, SubtitleFormat::Srt);
        assert_eq!(session.len(), 2);
        assert_eq!(session.active_at(2_000).unwrap().text, "Hello");
        assert!(session.active_at(4_000).is_none());
        session.delay_ms = 3_000; // subtitles appear 3s later
        assert!(session.active_at(2_000).is_none());
        assert_eq!(session.active_at(5_000).unwrap().text, "Hello");
        // style overrides are computed, not stored back into cues
        session.size_scale = 1.5;
        session.position_offset = 0.2;
        let cue = session.active_at(5_000).unwrap();
        assert_eq!(cue.style.relative_size, 1.5);
        assert!((cue.style.position - 0.3).abs() < f32::EPSILON);
        assert_eq!(session.cues()[0].style.relative_size, 1.0);
        assert!((session.cues()[0].style.position - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn subtitle_session_enable_disable_and_shift() {
        let mut session = SubtitleSession::from_text(
            "1\n00:00:01,000 --> 00:00:03,000\nHi\n",
            SubtitleFormat::Srt,
        );
        session.enabled = false;
        assert!(session.active_at(2_000).is_none());
        session.enabled = true;
        session.shift(2_000);
        assert_eq!(session.active_at(3_000).unwrap().text, "Hi");
        session.reset_delay();
        session.shift(-99_000);
        assert_eq!(session.cues()[0].start_ms, 0); // clamped
    }

    #[test]
    fn matches_video_by_stem_and_language_suffix() {
        let video = Path::new("Show/S01E01.mkv");
        assert!(subtitle_matches_video(video, Path::new("Show/S01E01.srt")));
        assert!(subtitle_matches_video(
            video,
            Path::new("Show/S01E01.ar.srt")
        ));
        assert!(subtitle_matches_video(
            video,
            Path::new("Show/S01E01_en.vtt")
        ));
        assert!(!subtitle_matches_video(video, Path::new("Show/S01E02.srt")));
        assert!(!subtitle_matches_video(video, Path::new("Show/other.srt")));
    }
}
