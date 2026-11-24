//! Diagnostic: what does libVLC report about a file's tracks, and when?
//!
//! Run with the bundled runtime on PATH:
//!     LIBVLC_PATH=vendor/libvlc cargo run -p vlc-wrapper --release --example tracks -- clip.mp4

use std::path::Path;
use std::time::{Duration, Instant};

use vlc_wrapper::Instance;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: tracks <media file>")?;
    let path = Path::new(&path);

    let instance = Instance::new()?;
    let media = instance.media_from_path(path)?;

    let started = Instant::now();
    let immediate = media.tracks();
    println!(
        "immediate tracks: video={:?} audio={}",
        immediate.video.is_some(),
        immediate.audio.len()
    );

    let waited = media.tracks_wait(Duration::from_millis(4000));
    println!(
        "after {:.0} ms: video={:?} audio={}",
        started.elapsed().as_millis(),
        waited.video.as_ref().map(|v| (v.width, v.height)),
        waited.audio.len()
    );

    // And through a live player, which is the path the app actually uses.
    let player = media.player()?;
    player.play()?;
    for step in 1..=10 {
        std::thread::sleep(Duration::from_millis(300));
        let tracks = player.media_tracks();
        println!(
            "player @{}ms: video={:?} size={:?}",
            step * 300,
            tracks.video.as_ref().map(|v| (v.width, v.height)),
            player.video_size()
        );
    }
    player.stop();
    Ok(())
}
