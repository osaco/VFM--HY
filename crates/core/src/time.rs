//! Timecode formatting and parsing. libVLC reports time in milliseconds (i64).

/// Format milliseconds as `M:SS` (or `H:MM:SS` for long clips).
pub fn format_timecode(ms: i64) -> String {
    let total = (ms / 1000).max(0);
    let secs = total % 60;
    let mins = (total / 60) % 60;
    let hours = total / 3600;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

/// Always format with an hours field: `HH:MM:SS`.
pub fn format_timecode_long(ms: i64) -> String {
    let total = (ms / 1000).max(0);
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total / 60) % 60,
        total % 60
    )
}

/// Parse `M:SS` / `H:MM:SS` / plain seconds into milliseconds.
/// Returns `None` for malformed or out-of-range input.
pub fn parse_timecode(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut values = [0i64; 3];
    for (i, part) in parts.iter().enumerate() {
        let part = *part;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        values[i] = part.parse::<i64>().ok()?;
    }
    let (h, m, sec) = if parts.len() == 3 {
        (values[0], values[1], values[2])
    } else if parts.len() == 2 {
        (0, values[0], values[1])
    } else {
        (0, 0, values[0])
    };
    if !(0..=59).contains(&m) || !(0..=59).contains(&sec) || !(0..=999).contains(&h) {
        return None;
    }
    Some(((h * 3600) + (m * 60) + sec) * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_short_and_long() {
        assert_eq!(format_timecode(0), "0:00");
        assert_eq!(format_timecode(65_400), "1:05");
        assert_eq!(format_timecode(3_723_000), "1:02:03");
        assert_eq!(format_timecode(-1), "0:00");
        assert_eq!(format_timecode_long(65_400), "00:01:05");
    }

    #[test]
    fn parses_all_supported_shapes() {
        assert_eq!(parse_timecode("0:05"), Some(5_000));
        assert_eq!(parse_timecode("1:02:03"), Some(3_723_000));
        assert_eq!(parse_timecode("42"), Some(42_000));
        assert_eq!(parse_timecode(" 1:05 "), Some(65_000));
    }

    #[test]
    fn rejects_invalid_input() {
        assert_eq!(parse_timecode(""), None);
        assert_eq!(parse_timecode("1:60"), None);
        assert_eq!(parse_timecode("a:b"), None);
        assert_eq!(parse_timecode("1:2:3:4"), None);
    }

    #[test]
    fn round_trip() {
        for ms in [0, 5_000, 65_000, 3_723_000] {
            assert_eq!(parse_timecode(&format_timecode(ms)), Some(ms));
        }
    }
}
