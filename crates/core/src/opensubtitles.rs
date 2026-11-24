//! OpenSubtitles REST client (api.opensubtitles.com v1).
//!
//! The user supplies an API key in settings; the key is never bundled. Everything that
//! can be tested without a network lives here as a pure function (hash, URL building,
//! response parsing), and the network calls are a thin shell on top so an offline build
//! or a missing key degrades to "search unavailable" instead of an error dialog.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Public API root. Kept in one place so a proxy or a mock can be swapped in tests.
pub const API_ROOT: &str = "https://api.opensubtitles.com/api/v1";

/// OpenSubtitles requires a User-Agent that identifies the application.
pub const DEFAULT_USER_AGENT: &str = "Vedio for me v0.1";

/// OpenSubtitles hashes the first and last 64 KiB of the file.
const HASH_CHUNK: u64 = 64 * 1024;

#[derive(Debug, Error)]
pub enum SubtitleApiError {
    #[error("no API key configured")]
    MissingApiKey,
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("network error: {0}")]
    Network(String),
    #[error("the server rejected the request: {0}")]
    Rejected(String),
    #[error("rate limited; retry in {0} seconds")]
    RateLimited(u64),
    #[error("the server sent an unexpected response: {0}")]
    Protocol(String),
}

/// One search result, flattened for the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleHit {
    /// `file_id` used by the download endpoint.
    pub file_id: i64,
    pub language: String,
    pub release: String,
    pub downloads: u64,
    pub rating: f32,
    pub hearing_impaired: bool,
    pub format: String,
    pub file_name: String,
}

/// Download ticket returned by `POST /download`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTicket {
    pub url: String,
    pub file_name: String,
    pub remaining: Option<u64>,
}

/// OpenSubtitles "movie hash": file size plus the sum of the little-endian `u64` words
/// in the first and last 64 KiB. Returned as 16 lowercase hex digits.
pub fn movie_hash(path: &Path) -> io::Result<(String, u64)> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    if size < HASH_CHUNK * 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file is smaller than 128 KiB, OpenSubtitles cannot hash it",
        ));
    }

    let mut hash = size;
    let mut buffer = [0u8; 8];
    for offset in [0u64, size - HASH_CHUNK] {
        file.seek(SeekFrom::Start(offset))?;
        for _ in 0..(HASH_CHUNK / 8) {
            file.read_exact(&mut buffer)?;
            hash = hash.wrapping_add(u64::from_le_bytes(buffer));
        }
    }
    Ok((format!("{hash:016x}"), size))
}

/// Search URL for a hash + size lookup.
pub fn hash_search_url(hash: &str, size: u64, languages: &[&str]) -> String {
    format!(
        "{API_ROOT}/subtitles?moviehash={hash}&moviebytesize={size}&languages={}",
        encode_languages(languages)
    )
}

/// Search URL for a free-text (file name) lookup.
pub fn name_search_url(query: &str, languages: &[&str]) -> String {
    format!(
        "{API_ROOT}/subtitles?query={}&languages={}",
        percent_encode(query),
        encode_languages(languages)
    )
}

fn encode_languages(languages: &[&str]) -> String {
    if languages.is_empty() {
        return "all".into();
    }
    languages
        .iter()
        .map(|code| percent_encode(code))
        .collect::<Vec<_>>()
        .join(",")
}

/// Minimal percent-encoding for query values: everything outside the unreserved set is
/// escaped. Avoids pulling in a URL crate for six call sites.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<SearchEntry>,
}

#[derive(Deserialize)]
struct SearchEntry {
    #[serde(default)]
    attributes: SearchAttributes,
}

#[derive(Deserialize, Default)]
struct SearchAttributes {
    #[serde(default)]
    language: String,
    #[serde(default)]
    release: String,
    #[serde(default)]
    download_count: u64,
    #[serde(default)]
    ratings: f32,
    #[serde(default)]
    hearing_impaired: bool,
    #[serde(default)]
    format: String,
    #[serde(default)]
    files: Vec<SearchFile>,
}

#[derive(Deserialize, Default)]
struct SearchFile {
    #[serde(default)]
    file_id: i64,
    #[serde(default)]
    file_name: String,
}

/// Parse a `/subtitles` response. Entries without a `file_id` cannot be downloaded and
/// are dropped rather than shown as dead rows.
pub fn parse_search_response(json: &str) -> Result<Vec<SubtitleHit>, SubtitleApiError> {
    let parsed: SearchResponse = serde_json::from_str(json)
        .map_err(|error| SubtitleApiError::Protocol(error.to_string()))?;
    Ok(parsed
        .data
        .into_iter()
        .filter_map(|entry| {
            let file = entry.attributes.files.into_iter().next()?;
            if file.file_id == 0 {
                return None;
            }
            Some(SubtitleHit {
                file_id: file.file_id,
                language: entry.attributes.language,
                release: entry.attributes.release,
                downloads: entry.attributes.download_count,
                rating: entry.attributes.ratings,
                hearing_impaired: entry.attributes.hearing_impaired,
                format: entry.attributes.format,
                file_name: file.file_name,
            })
        })
        .collect())
}

#[derive(Deserialize)]
struct DownloadResponse {
    #[serde(default)]
    link: String,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    remaining: Option<u64>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    #[serde(default)]
    message: String,
}

/// Parse a `/download` response into a ticket.
pub fn parse_download_response(json: &str) -> Result<DownloadTicket, SubtitleApiError> {
    let parsed: DownloadResponse = serde_json::from_str(json)
        .map_err(|error| SubtitleApiError::Protocol(error.to_string()))?;
    if parsed.link.is_empty() {
        return Err(SubtitleApiError::Protocol(
            "download response had no link".into(),
        ));
    }
    Ok(DownloadTicket {
        url: parsed.link,
        file_name: parsed.file_name,
        remaining: parsed.remaining,
    })
}

/// Extract the human-readable message from an error body.
pub fn parse_error_message(json: &str) -> String {
    serde_json::from_str::<ErrorResponse>(json)
        .map(|parsed| parsed.message)
        .unwrap_or_default()
}

/// Map an HTTP status to a typed error so the UI can phrase it correctly.
pub fn error_for_status(status: u16, body: &str, retry_after: Option<u64>) -> SubtitleApiError {
    match status {
        401 | 403 => SubtitleApiError::Rejected("check the API key or log in again".into()),
        429 => SubtitleApiError::RateLimited(retry_after.unwrap_or(1)),
        _ => {
            let message = parse_error_message(body);
            if message.is_empty() {
                SubtitleApiError::Rejected(format!("HTTP {status}"))
            } else {
                SubtitleApiError::Rejected(message)
            }
        }
    }
}

#[derive(Serialize)]
struct LoginBody<'a> {
    username: &'a str,
    password: &'a str,
}

#[derive(Deserialize)]
struct LoginResponse {
    #[serde(default)]
    token: String,
}

#[derive(Serialize)]
struct DownloadBody<'a> {
    file_id: &'a str,
}

/// Thin HTTP shell around the pure helpers above.
pub struct OpenSubtitlesClient {
    api_key: String,
    user_agent: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl OpenSubtitlesClient {
    /// Build a client. An empty key is accepted here and reported by the first request,
    /// so settings can be edited without the UI having to special-case construction.
    pub fn new(api_key: impl Into<String>, user_agent: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            user_agent: user_agent.into(),
            token: None,
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(20)))
                .build()
                .into(),
        }
    }

    pub fn has_api_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    fn ensure_key(&self) -> Result<(), SubtitleApiError> {
        if self.has_api_key() {
            Ok(())
        } else {
            Err(SubtitleApiError::MissingApiKey)
        }
    }

    fn get(&self, url: &str) -> Result<String, SubtitleApiError> {
        self.ensure_key()?;
        let mut request = self
            .agent
            .get(url)
            .header("Api-Key", &self.api_key)
            .header("User-Agent", &self.user_agent)
            .header("Accept", "application/json");
        if let Some(token) = &self.token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        match request.call() {
            Ok(mut response) => response
                .body_mut()
                .read_to_string()
                .map_err(|error| SubtitleApiError::Network(error.to_string())),
            Err(error) => Err(map_transport_error(error)),
        }
    }

    fn post_json(&self, url: &str, body: &impl Serialize) -> Result<String, SubtitleApiError> {
        self.ensure_key()?;
        let mut request = self
            .agent
            .post(url)
            .header("Api-Key", &self.api_key)
            .header("User-Agent", &self.user_agent)
            .header("Accept", "application/json");
        if let Some(token) = &self.token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        match request.send_json(body) {
            Ok(mut response) => response
                .body_mut()
                .read_to_string()
                .map_err(|error| SubtitleApiError::Network(error.to_string())),
            Err(error) => Err(map_transport_error(error)),
        }
    }

    /// Exchange credentials for a token. Only needed for the download endpoint's
    /// per-account quota; search works with the API key alone.
    pub fn login(&mut self, username: &str, password: &str) -> Result<(), SubtitleApiError> {
        let body = LoginBody { username, password };
        let text = self.post_json(&format!("{API_ROOT}/login"), &body)?;
        let parsed: LoginResponse = serde_json::from_str(&text)
            .map_err(|error| SubtitleApiError::Protocol(error.to_string()))?;
        if parsed.token.is_empty() {
            return Err(SubtitleApiError::Rejected("login returned no token".into()));
        }
        self.token = Some(parsed.token);
        Ok(())
    }

    pub fn search_by_hash(
        &self,
        hash: &str,
        size: u64,
        languages: &[&str],
    ) -> Result<Vec<SubtitleHit>, SubtitleApiError> {
        let text = self.get(&hash_search_url(hash, size, languages))?;
        parse_search_response(&text)
    }

    pub fn search_by_name(
        &self,
        query: &str,
        languages: &[&str],
    ) -> Result<Vec<SubtitleHit>, SubtitleApiError> {
        let text = self.get(&name_search_url(query, languages))?;
        parse_search_response(&text)
    }

    /// Ask for a one-time download link, then fetch the subtitle bytes.
    pub fn download(&self, file_id: i64) -> Result<DownloadTicket, SubtitleApiError> {
        let body = DownloadBody {
            file_id: &file_id.to_string(),
        };
        let text = self.post_json(&format!("{API_ROOT}/download"), &body)?;
        parse_download_response(&text)
    }

    /// Fetch the subtitle file itself from a download ticket.
    pub fn fetch(&self, ticket: &DownloadTicket) -> Result<Vec<u8>, SubtitleApiError> {
        let mut response = self
            .agent
            .get(&ticket.url)
            .header("User-Agent", &self.user_agent)
            .call()
            .map_err(map_transport_error)?;
        response
            .body_mut()
            .read_to_vec()
            .map_err(|error| SubtitleApiError::Network(error.to_string()))
    }
}

fn map_transport_error(error: ureq::Error) -> SubtitleApiError {
    match error {
        ureq::Error::StatusCode(code) => error_for_status(code, "", None),
        other => SubtitleApiError::Network(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn movie_hash_matches_the_reference_algorithm() {
        // 128 KiB of zeros: size + (16384 words * 0) + (16384 words * 0) = size.
        let dir = std::env::temp_dir().join(format!("vfm-oshash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zeros.bin");
        let mut file = File::create(&path).unwrap();
        file.write_all(&vec![0u8; 128 * 1024]).unwrap();
        drop(file);

        let (hash, size) = movie_hash(&path).unwrap();
        assert_eq!(size, 128 * 1024);
        assert_eq!(hash, format!("{:016x}", 128u64 * 1024));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn movie_hash_rejects_files_below_the_minimum() {
        let dir = std::env::temp_dir().join(format!("vfm-oshash-small-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tiny.bin");
        std::fs::write(&path, vec![0u8; 1024]).unwrap();
        assert!(movie_hash(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_urls_carry_hash_size_and_languages() {
        let url = hash_search_url("deadbeefdeadbeef", 1234, &["en", "ar"]);
        assert!(url.starts_with(&format!("{API_ROOT}/subtitles?")));
        assert!(url.contains("moviehash=deadbeefdeadbeef"));
        assert!(url.contains("moviebytesize=1234"));
        assert!(url.contains("languages=en,ar"));
    }

    #[test]
    fn name_search_urls_encode_spaces_and_specials() {
        let url = name_search_url("Solaris S01E01 1080p", &["en"]);
        assert!(url.contains("query=Solaris%20S01E01%201080p"));
        // "all" is the API's wildcard when no language is requested.
        assert!(name_search_url("x", &[]).contains("languages=all"));
    }

    #[test]
    fn search_response_is_flattened_and_skips_undownloadable_entries() {
        let json = r#"{
            "total_count": 3,
            "data": [
                { "attributes": { "language": "en", "release": "Ep.1080p",
                  "download_count": 42, "ratings": 8.5, "hearing_impaired": false,
                  "format": "srt", "files": [{ "file_id": 111, "file_name": "a.srt" }] } },
                { "attributes": { "language": "ar", "release": "Ep.WEB",
                  "files": [{ "file_id": 222, "file_name": "b.srt" }] } },
                { "attributes": { "language": "fr", "files": [] } }
            ]
        }"#;
        let hits = parse_search_response(json).unwrap();
        assert_eq!(hits.len(), 2, "the entry without a file_id is dropped");
        assert_eq!(hits[0].file_id, 111);
        assert_eq!(hits[0].downloads, 42);
        assert!((hits[0].rating - 8.5).abs() < f32::EPSILON);
        assert_eq!(hits[1].file_id, 222);
        assert_eq!(hits[1].file_name, "b.srt");
    }

    #[test]
    fn malformed_search_payload_is_a_protocol_error() {
        assert!(matches!(
            parse_search_response("not json"),
            Err(SubtitleApiError::Protocol(_))
        ));
    }

    #[test]
    fn download_response_requires_a_link() {
        let ticket = parse_download_response(
            r#"{"link":"https://x/y.zip","file_name":"y.srt","remaining":9}"#,
        )
        .unwrap();
        assert_eq!(ticket.url, "https://x/y.zip");
        assert_eq!(ticket.remaining, Some(9));
        assert!(matches!(
            parse_download_response(r#"{"message":"quota"}"#),
            Err(SubtitleApiError::Protocol(_))
        ));
    }

    #[test]
    fn status_codes_map_to_actionable_errors() {
        assert!(matches!(
            error_for_status(401, "", None),
            SubtitleApiError::Rejected(_)
        ));
        match error_for_status(429, "", Some(30)) {
            SubtitleApiError::RateLimited(seconds) => assert_eq!(seconds, 30),
            other => panic!("expected RateLimited, got {other:?}"),
        }
        match error_for_status(406, r#"{"message":"no subtitles found"}"#, None) {
            SubtitleApiError::Rejected(message) => assert_eq!(message, "no subtitles found"),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn a_client_without_a_key_fails_before_touching_the_network() {
        let client = OpenSubtitlesClient::new("", DEFAULT_USER_AGENT);
        assert!(!client.has_api_key());
        assert!(matches!(
            client.search_by_name("anything", &["en"]),
            Err(SubtitleApiError::MissingApiKey)
        ));
    }
}
