//! Exclusive, bounded-size stereo PCM WAV capture for deterministic audio checks.

use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

/// Streams stereo signed-16 PCM and finalizes its RIFF header explicitly.
pub struct WavWriter {
    file: BufWriter<File>,
    rate: u32,
    bytes: u32,
}

impl WavWriter {
    /// Creates a new file; never replaces an existing artifact.
    pub fn create(path: &Path, rate: u32) -> io::Result<Self> {
        if rate == 0 || rate > u32::MAX / 4 {
            return Err(io::Error::other("invalid PCM rate"));
        }
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut writer = Self {
            file: BufWriter::new(file),
            rate,
            bytes: 0,
        };
        writer.header()?;
        Ok(writer)
    }

    /// Appends left/right sample frames; rejects RIFF files exceeding 4 GiB.
    pub fn write(&mut self, samples: &[[i16; 2]]) -> io::Result<()> {
        let added = u32::try_from(samples.len())
            .ok()
            .and_then(|count| count.checked_mul(4));
        let total = added
            .and_then(|count| self.bytes.checked_add(count))
            .filter(|total| *total <= u32::MAX - 36)
            .ok_or_else(|| io::Error::other("WAV capture exceeds RIFF size limit"))?;
        for frame in samples {
            for sample in frame {
                self.file.write_all(&sample.to_le_bytes())?;
            }
        }
        self.bytes = total;
        Ok(())
    }

    fn header(&mut self) -> io::Result<()> {
        self.file.write_all(b"RIFF")?;
        self.file.write_all(&(36 + self.bytes).to_le_bytes())?;
        self.file.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x02\0")?;
        self.file.write_all(&self.rate.to_le_bytes())?;
        self.file.write_all(&(self.rate * 4).to_le_bytes())?;
        self.file.write_all(b"\x04\0\x10\0data")?;
        self.file.write_all(&self.bytes.to_le_bytes())
    }

    /// Flushes output, rewrites the length header, and syncs the file.
    pub fn finish(mut self) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.header()?;
        self.file.flush()?;
        self.file.get_ref().sync_all()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pcm_header_sizes_endianness_and_exclusive_creation() {
        let path = std::env::temp_dir().join(format!("gba-wav-{}.wav", std::process::id()));
        let mut writer = super::WavWriter::create(&path, 32768).expect("new WAV");
        writer.write(&[[0x1234, -2], [0, 32767]]).expect("samples");
        writer.finish().expect("finalize");
        let bytes = std::fs::read(&path).expect("WAV bytes");
        assert_eq!(bytes.len(), 52);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[40..44], &8u32.to_le_bytes());
        assert_eq!(&bytes[44..48], &[0x34, 0x12, 0xfe, 0xff]);
        assert!(super::WavWriter::create(&path, 32768).is_err());
        std::fs::remove_file(path).expect("cleanup");
    }
}
