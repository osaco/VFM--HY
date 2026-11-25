//! Display transform for decoded frames: rotation and mirroring.
//!
//! The engine hands every frame to the UI as RGBA8, so a transform that has to change the
//! pixels belongs here: pure math, no unsafe, unit-tested.

/// Clockwise rotation in quarter turns plus optional mirroring.
///
/// The mirror is applied first (in the source frame), then the rotation. That is the order the
/// sidebar controls imply: "flip horizontally" mirrors what the viewer sees before the picture
/// is turned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VideoTransform {
    /// Clockwise quarter turns, normalised to `0..=3`.
    pub quarters: u8,
    /// Mirror left/right.
    pub flip_h: bool,
    /// Mirror top/bottom.
    pub flip_v: bool,
}

impl VideoTransform {
    /// Build a transform from the values the settings and presets store.
    pub fn new(rotation_degrees: i32, flip_h: bool, flip_v: bool) -> Self {
        let quarters = (rotation_degrees / 90).rem_euclid(4) as u8;
        Self {
            quarters,
            flip_h,
            flip_v,
        }
    }

    /// True when the frame can be passed through untouched.
    pub fn is_identity(&self) -> bool {
        self.quarters.is_multiple_of(4) && !self.flip_h && !self.flip_v
    }

    /// Size of the frame after the transform: width and height swap for odd quarter turns.
    pub fn output_size(&self, width: u32, height: u32) -> (u32, u32) {
        if self.quarters % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// Write the transformed frame into `dst`, which must hold `output_size(w, h)` RGBA8 pixels.
    ///
    /// `src` and `dst` may not overlap; callers pass a freshly allocated buffer.
    pub fn apply_into(&self, src: &[u8], width: u32, height: u32, dst: &mut [u8]) {
        let (out_w, out_h) = self.output_size(width, height);
        let (w, h) = (width as i64, height as i64);
        let (ow, oh) = (out_w as i64, out_h as i64);
        debug_assert_eq!(src.len(), (w * h * 4) as usize);
        debug_assert_eq!(dst.len(), (ow * oh * 4) as usize);
        for dy in 0..oh {
            for dx in 0..ow {
                // Where this destination pixel sits in the unrotated, unmirrored source.
                let (mut sx, mut sy) = match self.quarters % 4 {
                    1 => (dy, h - 1 - dx),
                    2 => (w - 1 - dx, h - 1 - dy),
                    3 => (w - 1 - dy, dx),
                    _ => (dx, dy),
                };
                if self.flip_h {
                    sx = w - 1 - sx;
                }
                if self.flip_v {
                    sy = h - 1 - sy;
                }
                let si = ((sy * w + sx) * 4) as usize;
                let di = ((dy * ow + dx) * 4) as usize;
                dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x2 frame whose pixels are numbered left-to-right, top-to-bottom: 1 2 / 3 4.
    fn frame() -> Vec<u8> {
        let mut src = Vec::new();
        for v in 1u8..=4 {
            src.extend_from_slice(&[v, v, v, 255]);
        }
        src
    }

    /// First channel of each pixel, row-major.
    fn values(buf: &[u8]) -> Vec<u8> {
        buf.as_chunks::<4>().0.iter().map(|px| px[0]).collect()
    }

    fn apply(t: VideoTransform) -> Vec<u8> {
        let (ow, oh) = t.output_size(2, 2);
        let mut dst = vec![0u8; (ow * oh * 4) as usize];
        t.apply_into(&frame(), 2, 2, &mut dst);
        dst
    }

    #[test]
    fn identity_is_detected_and_copies_every_pixel() {
        let t = VideoTransform::new(0, false, false);
        assert!(t.is_identity());
        assert_eq!(values(&apply(t)), vec![1, 2, 3, 4]);
    }

    #[test]
    fn a_full_turn_is_identity_again() {
        let t = VideoTransform::new(360, false, false);
        assert_eq!(t.quarters, 0);
        assert!(t.is_identity());
        assert_eq!(values(&apply(t)), vec![1, 2, 3, 4]);
    }

    #[test]
    fn negative_rotation_normalises_clockwise() {
        assert_eq!(VideoTransform::new(-90, false, false).quarters, 3);
    }

    #[test]
    fn odd_quarters_swap_the_output_size() {
        let t = VideoTransform::new(90, false, false);
        assert_eq!(t.output_size(1920, 1080), (1080, 1920));
        assert_eq!(
            VideoTransform::new(180, false, false).output_size(1920, 1080),
            (1920, 1080)
        );
    }

    #[test]
    fn quarter_turn_clockwise_moves_the_bottom_left_corner_to_the_top_left() {
        assert_eq!(
            values(&apply(VideoTransform::new(90, false, false))),
            vec![3, 1, 4, 2]
        );
    }

    #[test]
    fn quarter_turn_counter_clockwise_moves_the_top_right_corner_to_the_top_left() {
        assert_eq!(
            values(&apply(VideoTransform::new(270, false, false))),
            vec![2, 4, 1, 3]
        );
    }

    #[test]
    fn half_turn_reverses_both_axes() {
        assert_eq!(
            values(&apply(VideoTransform::new(180, false, false))),
            vec![4, 3, 2, 1]
        );
    }

    #[test]
    fn two_quarter_turns_equal_a_half_turn() {
        let once = VideoTransform::new(90, false, false);
        let (ow, oh) = once.output_size(2, 2);
        let mut mid = vec![0u8; (ow * oh * 4) as usize];
        once.apply_into(&frame(), 2, 2, &mut mid);
        let mut dst = vec![0u8; mid.len()];
        once.apply_into(&mid, ow, oh, &mut dst);
        assert_eq!(
            values(&dst),
            values(&apply(VideoTransform::new(180, false, false)))
        );
    }

    #[test]
    fn flips_mirror_before_rotating() {
        assert_eq!(
            values(&apply(VideoTransform::new(0, true, false))),
            vec![2, 1, 4, 3]
        );
        assert_eq!(
            values(&apply(VideoTransform::new(0, false, true))),
            vec![3, 4, 1, 2]
        );
        // Mirror left/right, then turn clockwise: 2 1 / 4 3 becomes 4 2 / 3 1.
        assert_eq!(
            values(&apply(VideoTransform::new(90, true, false))),
            vec![4, 2, 3, 1]
        );
    }

    #[test]
    fn a_transform_on_a_wide_frame_keeps_every_byte() {
        let (w, h) = (3u32, 2u32);
        let src: Vec<u8> = (0..(w * h * 4) as u8).collect();
        let t = VideoTransform::new(90, true, true);
        let (ow, oh) = t.output_size(w, h);
        let mut dst = vec![0u8; (ow * oh * 4) as usize];
        t.apply_into(&src, w, h, &mut dst);
        let mut sorted_src = src.clone();
        let mut sorted_dst = dst.clone();
        sorted_src.sort_unstable();
        sorted_dst.sort_unstable();
        assert_eq!(sorted_src, sorted_dst);
    }
}
