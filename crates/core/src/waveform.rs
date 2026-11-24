//! Audio peak envelopes for the seek bar and music mode.
//!
//! libVLC does not expose an audio envelope, so the waveform is decoded separately with
//! `symphonia` — once per file — and cached on disk. Decoding a two-hour film takes a few
//! seconds, so the app runs this on a worker thread and paints the bar when it lands.
//!
//! The pipeline is deliberately simple: decode every audio frame, keep the peak amplitude
//! of fixed-size chunks, then max-pool those chunks down to the requested bucket count.
//! Peaks are normalised to `0.0..=1.0` so the UI can scale them without knowing the codec.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Buckets used for the seek bar: one peak per ~2 px on a 1920-wide window.
pub const DEFAULT_BUCKETS: usize = 900;

/// Frames per peak bucket while decoding. 2048 frames is ~43 ms at 48 kHz — fine enough
/// to show quiet gaps, coarse enough to keep a two-hour film's envelope small.
const CHUNK_FRAMES: usize = 2048;

/// Peak envelope of one media file.
#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    /// Peak amplitude per bucket, normalised to `0.0..=1.0`.
    pub peaks: Vec<f32>,
    pub duration_ms: i64,
}

impl Waveform {
    pub fn is_empty(&self) -> bool {
        self.peaks.is_empty()
    }

    /// Peak at a playback position expressed as `0.0..=1.0`.
    pub fn peak_at(&self, fraction: f32) -> f32 {
        if self.peaks.is_empty() {
            return 0.0;
        }
        let index = (fraction.clamp(0.0, 1.0) * (self.peaks.len() - 1) as f32).round() as usize;
        self.peaks[index.min(self.peaks.len() - 1)]
    }

    /// Encode for the on-disk cache: bucket count, duration, then little-endian `f32`s.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.peaks.len() * 4);
        out.extend_from_slice(&(self.peaks.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.duration_ms.to_le_bytes());
        for peak in &self.peaks {
            out.extend_from_slice(&peak.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 12 {
            return None;
        }
        let count = u32::from_le_bytes(bytes[0..4].try_into().ok()?) as usize;
        let duration_ms = i64::from_le_bytes(bytes[4..12].try_into().ok()?);
        if count == 0 || bytes.len() < 12 + count * 4 {
            return None;
        }
        // Indexed rather than `chunks_exact`: the payload length is already validated
        // above, and this keeps the offset arithmetic visible next to the header layout.
        let peaks = (0..count)
            .map(|index| {
                let at = 12 + index * 4;
                f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
            })
            .collect();
        Some(Self { peaks, duration_ms })
    }
}

#[derive(Debug, Error)]
pub enum WaveformError {
    #[error("cannot open media: {0}")]
    Open(String),
    #[error("cannot decode audio: {0}")]
    Decode(String),
    #[error("the file has no audio track")]
    NoAudio,
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// Decode `path` and build a peak envelope with at most `buckets` entries.
///
/// Corrupt packets are skipped rather than aborting the whole waveform; a file with no
/// audio track returns `WaveformError::NoAudio` so the UI can hide the bar.
pub fn analyse(path: &Path, buckets: usize) -> Result<Waveform, WaveformError> {
    let buckets = buckets.max(1);
    let file = fs::File::open(path)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| WaveformError::Open(error.to_string()))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(WaveformError::NoAudio)?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(48_000).max(1);
    let channels = track
        .codec_params
        .channels
        .map(|value| value.count())
        .unwrap_or(1)
        .max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| WaveformError::Decode(error.to_string()))?;

    let samples_per_chunk = CHUNK_FRAMES * channels;
    let mut chunk_peaks: Vec<f32> = Vec::new();
    let mut current_peak = 0.0f32;
    let mut samples_in_chunk = 0usize;
    let mut decoded_frames: u64 = 0;
    let mut sample_buffer: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // End of stream and container switches are normal terminations.
            Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(error) => return Err(WaveformError::Decode(error.to_string())),
        };
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A single corrupt packet should not discard an otherwise good waveform.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(WaveformError::Decode(error.to_string())),
        };

        let spec = *decoded.spec();
        let capacity = decoded.capacity() as u64;
        let mut buffer = match sample_buffer.take() {
            Some(existing) if existing.capacity() >= capacity as usize => existing,
            _ => SampleBuffer::<f32>::new(capacity, spec),
        };
        buffer.copy_interleaved_ref(decoded);
        decoded_frames += buffer.samples().len() as u64 / channels as u64;

        for sample in buffer.samples() {
            let amplitude = sample.abs();
            if amplitude > current_peak {
                current_peak = amplitude;
            }
            samples_in_chunk += 1;
            if samples_in_chunk >= samples_per_chunk {
                chunk_peaks.push(current_peak);
                current_peak = 0.0;
                samples_in_chunk = 0;
            }
        }
        sample_buffer = Some(buffer);
    }

    if samples_in_chunk > 0 {
        chunk_peaks.push(current_peak);
    }
    if chunk_peaks.is_empty() {
        return Err(WaveformError::NoAudio);
    }

    let duration_ms = (decoded_frames as f64 * 1000.0 / sample_rate as f64) as i64;
    Ok(Waveform {
        peaks: downsample(&chunk_peaks, buckets),
        duration_ms,
    })
}

/// Max-pool `peaks` down to at most `buckets` entries, then normalise to `0.0..=1.0`.
pub fn downsample(peaks: &[f32], buckets: usize) -> Vec<f32> {
    let buckets = buckets.max(1);
    if peaks.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(buckets);
    for index in 0..buckets {
        let start = index * peaks.len() / buckets;
        let end = ((index + 1) * peaks.len() / buckets)
            .max(start + 1)
            .min(peaks.len());
        let peak = peaks[start..end].iter().copied().fold(0.0f32, f32::max);
        out.push(peak);
    }
    normalise(&mut out);
    out
}

/// Scale peaks so the loudest bucket is 1.0. Silent files stay flat at 0.0.
fn normalise(peaks: &mut [f32]) {
    let loudest = peaks.iter().copied().fold(0.0f32, f32::max);
    if loudest <= f32::EPSILON {
        return;
    }
    for peak in peaks.iter_mut() {
        *peak = (*peak / loudest).clamp(0.0, 1.0);
    }
}

/// Disk cache for computed waveforms, keyed by path, size and modification time so a
/// re-encoded file gets a fresh envelope.
#[derive(Debug, Clone)]
pub struct WaveformCache {
    root: PathBuf,
}

impl WaveformCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn path_for(&self, media: &Path) -> PathBuf {
        self.root.join(format!("{:016x}.wf", cache_key(media)))
    }

    pub fn get(&self, media: &Path) -> Option<Waveform> {
        let bytes = fs::read(self.path_for(media)).ok()?;
        Waveform::from_bytes(&bytes)
    }

    pub fn put(&self, media: &Path, waveform: &Waveform) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::write(self.path_for(media), waveform.to_bytes())
    }

    /// Cached envelope, or compute and store it. Errors are returned unchanged so the
    /// caller can distinguish "no audio" from "corrupt file".
    pub fn get_or_compute(&self, media: &Path, buckets: usize) -> Result<Waveform, WaveformError> {
        if let Some(cached) = self.get(media) {
            return Ok(cached);
        }
        let waveform = analyse(media, buckets)?;
        let _ = self.put(media, &waveform);
        Ok(waveform)
    }
}

/// FNV-1a over path, file size and modification time.
fn cache_key(media: &Path) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(media.to_string_lossy().as_bytes());
    if let Ok(meta) = fs::metadata(media) {
        feed(&meta.len().to_le_bytes());
        if let Ok(modified) = meta.modified() {
            if let Ok(elapsed) = modified.duration_since(std::time::UNIX_EPOCH) {
                feed(&elapsed.as_secs().to_le_bytes());
            }
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_keeps_the_loudest_peak_in_each_bucket() {
        let peaks = [0.1, 0.9, 0.2, 0.8, 0.05, 0.3];
        let out = downsample(&peaks, 3);
        assert_eq!(out.len(), 3);
        assert!(
            (out[0] - 1.0).abs() < 1e-6,
            "0.9 is the loudest, so it normalises to 1"
        );
        assert!((out[1] - 0.8 / 0.9).abs() < 1e-5);
        assert!((out[2] - 0.3 / 0.9).abs() < 1e-5);
    }

    #[test]
    fn downsample_never_returns_more_buckets_than_requested() {
        let peaks = vec![0.5f32; 10_000];
        assert_eq!(downsample(&peaks, 64).len(), 64);
    }

    #[test]
    fn a_silent_file_stays_flat() {
        let peaks = vec![0.0f32; 100];
        assert!(downsample(&peaks, 10).iter().all(|peak| *peak == 0.0));
    }

    #[test]
    fn peak_at_reads_the_matching_bucket() {
        let waveform = Waveform {
            peaks: vec![0.0, 0.5, 1.0],
            duration_ms: 3_000,
        };
        assert_eq!(waveform.peak_at(0.0), 0.0);
        assert_eq!(waveform.peak_at(0.5), 0.5);
        assert_eq!(waveform.peak_at(1.0), 1.0);
        assert_eq!(waveform.peak_at(9.0), 1.0, "out-of-range clamps");
        let empty = Waveform {
            peaks: Vec::new(),
            duration_ms: 0,
        };
        assert_eq!(empty.peak_at(0.4), 0.0);
    }

    #[test]
    fn cache_bytes_round_trip() {
        let waveform = Waveform {
            peaks: vec![0.0, 0.25, 1.0],
            duration_ms: 123_456,
        };
        let restored = Waveform::from_bytes(&waveform.to_bytes()).expect("decodes");
        assert_eq!(restored, waveform);
        assert!(Waveform::from_bytes(&[]).is_none());
        assert!(Waveform::from_bytes(&[1, 0, 0, 0]).is_none());
    }

    #[test]
    fn analysing_a_video_without_audio_reports_no_audio() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/phase5/Ep.mkv");
        if !path.exists() {
            return;
        }
        match analyse(&path, 64) {
            Err(WaveformError::NoAudio) => {}
            other => panic!("expected NoAudio for a video-only file, got {other:?}"),
        }
    }

    #[test]
    fn analysing_a_generated_wav_produces_a_normalised_envelope() {
        let dir = std::env::temp_dir().join(format!("vfm-waveform-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("tone.wav");
        write_test_wav(&wav, 8_000, 8_000);

        let waveform = analyse(&wav, 100).expect("wav decodes");
        assert_eq!(waveform.peaks.len(), 100);
        assert!(
            (waveform.duration_ms - 1_000).abs() < 50,
            "one second of audio"
        );
        let loudest = waveform.peaks.iter().copied().fold(0.0f32, f32::max);
        assert!((loudest - 1.0).abs() < 1e-5, "the envelope is normalised");
        assert!(waveform.peaks.iter().any(|peak| *peak > 0.5));

        let _ = fs::remove_dir_all(&dir);
    }

    /// Write a minimal 16-bit mono PCM WAV so the decoder path is tested without ffmpeg.
    fn write_test_wav(path: &Path, sample_rate: u32, frames: u32) {
        let mut data = Vec::with_capacity(frames as usize * 2);
        for index in 0..frames {
            let phase = index as f32 / sample_rate as f32 * 440.0 * std::f32::consts::TAU;
            let sample = (phase.sin() * 0.8 * i16::MAX as f32) as i16;
            data.extend_from_slice(&sample.to_le_bytes());
        }
        let mut out = Vec::with_capacity(44 + data.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        out.extend_from_slice(&2u16.to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        fs::write(path, out).unwrap();
    }
}
