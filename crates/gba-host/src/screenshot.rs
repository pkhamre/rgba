//! Screenshot output without image-library dependencies.

use std::fs::OpenOptions;
use std::io::{self, BufWriter};
use std::path::Path;

/// Writes 240×160 RGB pixels as a binary PPM. Existing files are never replaced.
pub fn write_ppm(path: &Path, pixels: &[u32]) -> Result<(), io::Error> {
    use std::io::Write;
    if pixels.len() != 240 * 160 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "screenshot requires 240x160 pixels",
        ));
    }
    let mut file = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);
    file.write_all(b"P6\n240 160\n255\n")?;
    for pixel in pixels {
        file.write_all(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8])?;
    }
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::write_ppm;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn writes_rgb_pixels_exclusively_and_checks_dimensions() {
        let path = std::env::temp_dir().join(format!(
            "gba-ppm-{}-{}.ppm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(write_ppm(&path, &[]).is_err());
        assert!(!path.exists());
        let mut pixels = vec![0; 240 * 160];
        pixels[0] = 0x123456;
        write_ppm(&path, &pixels).expect("new screenshot");
        let bytes = fs::read(&path).expect("screenshot");
        assert!(bytes.starts_with(b"P6\n240 160\n255\n\x12\x34\x56"));
        assert!(write_ppm(&path, &pixels).is_err(), "must not overwrite");
        assert_eq!(fs::read(&path).expect("original screenshot"), bytes);
        fs::remove_file(path).expect("remove screenshot");
    }
}
