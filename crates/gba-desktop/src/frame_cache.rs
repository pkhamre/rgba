//! Avoid redundant host conversion/uploads; never skip guest emulation frames.

#[derive(Default)]
pub(crate) struct FrameCache {
    pixels: Vec<u32>,
    uploads: u64,
}

impl FrameCache {
    pub(crate) fn update(&mut self, pixels: &[u32]) -> bool {
        if self.pixels == pixels {
            return false;
        }
        self.pixels.clear();
        self.pixels.extend_from_slice(pixels);
        self.uploads += 1;
        true
    }

    pub(crate) fn uploads(&self) -> u64 {
        self.uploads
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn uploads_first_frame_and_changes_but_reuses_identical_pixels() {
        let mut cache = super::FrameCache::default();
        let mut pixels = vec![0; 240 * 160];
        assert!(cache.update(&pixels));
        assert!(!cache.update(&pixels));
        pixels[240 * 80 + 120] = 0x123456;
        assert!(cache.update(&pixels));
        assert!(!cache.update(&pixels));
        pixels[240 * 80 + 120] = 0;
        assert!(cache.update(&pixels));
        assert_eq!(cache.uploads(), 3);
    }
}
