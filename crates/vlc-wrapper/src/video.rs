//! Double-buffered RV32/BGRA (or RGBA) frame swapper — the ONLY place raw video pixels move.
//!
//! Thread model (documented invariants):
//! * libVLC's video thread calls `lock` -> writes pixels -> `unlock` -> `display`.
//! * A slot is owned by exactly one side at a time, tracked by `Slot::claimed` (CAS).
//!   `lock()` claims a free slot; `unlock()` releases it; `with_front()` claims the front
//!   slot for the Slint UI thread and releases it after the copy. No data race exists.
//! * Callbacks never call any libVLC API (would deadlock libVLC's vout mutex) and never
//!   block on the UI thread; with two slots there is always a free one for the decoder.
//! * All `unsafe` in this module is confined to raw-pointer recovery under an owned claim.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::VlcError;

/// Chroma requested from libVLC. `Rv32Bgra` is BGRA byte order on x86_64 and needs an
/// R/B swap before Slint's `Rgba8Pixel`; `Rgba` is a direct copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba,
    Rv32Bgra,
}

impl PixelFormat {
    pub fn fourcc(self) -> &'static [u8] {
        match self {
            Self::Rgba => b"RGBA\0",
            Self::Rv32Bgra => b"RV32\0",
        }
    }

    pub fn parse(s: &str) -> Result<Self, VlcError> {
        match s.to_ascii_lowercase().as_str() {
            "rgba" => Ok(Self::Rgba),
            "rv32" | "bgra" => Ok(Self::Rv32Bgra),
            _ => Err(VlcError::InvalidChroma(s.to_owned())),
        }
    }

    pub fn needs_swizzle(self) -> bool {
        self == Self::Rv32Bgra
    }
}

#[derive(Debug)]
struct Slot {
    buf: Vec<u8>,
    claimed: AtomicBool,
}

/// Heap-stable: the address handed to libVLC as `opaque` must never move.
#[derive(Debug)]
pub struct FrameSwap {
    slots: [Slot; 2],
    front: AtomicUsize,
    front_ready: AtomicBool,
    write_slot: AtomicUsize,
    decoded: AtomicU64,
    presented: AtomicU64,
    dropped: AtomicU64,
    width: u32,
    height: u32,
    format: PixelFormat,
}

impl FrameSwap {
    /// SAFETY: caller must keep the returned Box alive until libVLC callbacks are detached
    /// (MediaPlayer::drop stops playback and releases the player before dropping this).
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Result<Arc<Self>, VlcError> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(VlcError::InvalidDimensions(width, height));
        }
        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or(VlcError::InvalidDimensions(width, height))?;
        let make = || Slot {
            buf: vec![0u8; len],
            claimed: AtomicBool::new(false),
        };
        Ok(Arc::new(Self {
            slots: [make(), make()],
            front: AtomicUsize::new(0),
            front_ready: AtomicBool::new(false),
            write_slot: AtomicUsize::new(0),
            decoded: AtomicU64::new(0),
            presented: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            width,
            height,
            format,
        }))
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// True when a decoded frame is waiting for the UI; lets the presenter coalesce wakeups.
    pub fn has_pending(&self) -> bool {
        self.front_ready.load(Ordering::Acquire)
    }

    /// Present the newest complete frame to the UI. Returns false when no new frame is
    /// pending (the UI coalesces wakeups) or when the decoder already reclaimed that slot.
    pub fn with_front<F: FnOnce(&mut [u8])>(&self, consume: F) -> bool {
        if !self.front_ready.swap(false, Ordering::AcqRel) {
            return false;
        }
        let idx = self.front.load(Ordering::Acquire);
        if self.slots[idx]
            .claimed
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        // SAFETY: the CAS above gives this thread exclusive ownership of slot `idx`;
        // libVLC cannot write to it until we release `claimed` below.
        let buf = unsafe {
            std::slice::from_raw_parts_mut(
                self.slots[idx].buf.as_ptr() as *mut u8,
                self.slots[idx].buf.len(),
            )
        };
        consume(buf);
        self.slots[idx].claimed.store(false, Ordering::Release);
        self.presented.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub fn decoded(&self) -> u64 {
        self.decoded.load(Ordering::Relaxed)
    }
    pub fn presented(&self) -> u64 {
        self.presented.load(Ordering::Relaxed)
    }
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// In-place BGRA -> RGBA byte swap for `PixelFormat::Rv32Bgra`.
pub fn swizzle_rgba(buf: &mut [u8]) {
    debug_assert_eq!(buf.len() % 4, 0);
    for px in buf.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
}

unsafe fn claim_for_decode(s: &FrameSwap, planes: *mut *mut c_void) -> *mut c_void {
    // Never write into a front frame the UI has not consumed yet, even though unlock()
    // released its claim. Without this the decoder can immediately reclaim the just-published
    // slot and the UI's with_front() CAS always fails (observed: 0 presented frames).
    let front = s.front.load(Ordering::Acquire);
    let preferred = if s.front_ready.load(Ordering::Acquire) {
        1 - front
    } else {
        front
    };
    for i in [preferred, 1 - preferred] {
        if s.slots[i]
            .claimed
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            let ptr = s.slots[i].buf.as_ptr() as *mut c_void;
            s.write_slot.store(i, Ordering::Release);
            // SAFETY: libVLC allocates the `planes` array; plane 0 is valid for writing.
            if !planes.is_null() {
                *planes = ptr;
            }
            return ptr;
        }
    }
    // Defensive: with two slots and a single-threaded UI this is unreachable. Counting the
    // miss and returning the write slot keeps libVLC alive without blocking its vout thread.
    let i = s.write_slot.load(Ordering::Acquire);
    s.dropped.fetch_add(1, Ordering::Relaxed);
    let ptr = s.slots[i].buf.as_ptr() as *mut c_void;
    if !planes.is_null() {
        *planes = ptr;
    }
    ptr
}

pub const VIDEO_LOCK_CB: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void =
    lock_cb;
pub const VIDEO_UNLOCK_CB: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *mut c_void) =
    unlock_cb;
pub const VIDEO_DISPLAY_CB: unsafe extern "C" fn(*mut c_void, *mut c_void) = display_cb;

unsafe extern "C" fn lock_cb(opaque: *mut c_void, planes: *mut *mut c_void) -> *mut c_void {
    // SAFETY: `opaque` is the Box<FrameSwap> pointer set by MediaPlayer::enable_video_callbacks,
    // alive until player release. Callbacks never re-enter libVLC.
    let s = &*(opaque as *const FrameSwap);
    claim_for_decode(s, planes)
}

unsafe extern "C" fn unlock_cb(
    opaque: *mut c_void,
    _picture: *mut c_void,
    _planes: *mut *mut c_void,
) {
    let s = &*(opaque as *const FrameSwap);
    let i = s.write_slot.load(Ordering::Acquire);
    s.slots[i].claimed.store(false, Ordering::Release);
}

unsafe extern "C" fn display_cb(opaque: *mut c_void, _picture: *mut c_void) {
    let s = &*(opaque as *const FrameSwap);
    // A second display before the UI consumed the previous front means that frame was lost.
    if s.front_ready.swap(true, Ordering::AcqRel) {
        s.dropped.fetch_add(1, Ordering::Relaxed);
    }
    s.front
        .store(s.write_slot.load(Ordering::Acquire), Ordering::Release);
    s.decoded.fetch_add(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swaps_rb_channels() {
        let mut px = [0x11, 0x22, 0x33, 0xFF, 0x44, 0x55, 0x66, 0xFF];
        swizzle_rgba(&mut px);
        assert_eq!(px, [0x33, 0x22, 0x11, 0xFF, 0x66, 0x55, 0x44, 0xFF]);
    }

    #[test]
    fn fourcc_and_parse() {
        assert_eq!(PixelFormat::Rgba.fourcc(), b"RGBA\0");
        assert_eq!(PixelFormat::Rv32Bgra.fourcc(), b"RV32\0");
        assert_eq!(PixelFormat::parse("bgra").unwrap(), PixelFormat::Rv32Bgra);
        assert!(PixelFormat::parse("nv12").is_err());
    }

    #[test]
    fn rejects_bad_dimensions() {
        assert!(FrameSwap::new(0, 1080, PixelFormat::Rgba).is_err());
        assert!(FrameSwap::new(1920, 1080, PixelFormat::Rgba).is_ok());
    }
}
