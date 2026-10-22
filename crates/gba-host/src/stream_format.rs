//! Framing for unseekable audio-player stdin.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamFormat {
    RawLittleEndian,
    AuBigEndian,
}

impl StreamFormat {
    pub(crate) fn header(self) -> &'static [u8] {
        match self {
            Self::RawLittleEndian => &[],
            // AU: .snd, offset 24, unknown length (-1), PCM16 (3), 32768 Hz, stereo.
            // Unlike RIFF, AU explicitly supports an indefinitely sized stream.
            Self::AuBigEndian => b".snd\0\0\0\x18\xff\xff\xff\xff\0\0\0\x03\0\0\x80\0\0\0\0\x02",
        }
    }

    pub(crate) fn sample(self, value: i16) -> [u8; 2] {
        match self {
            Self::RawLittleEndian => value.to_le_bytes(),
            Self::AuBigEndian => value.to_be_bytes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StreamFormat;

    #[test]
    fn au_header_describes_unbounded_stereo_pcm_without_host_endian_assumptions() {
        let header = StreamFormat::AuBigEndian.header();
        assert_eq!(header.len(), 24);
        assert_eq!(&header[..4], b".snd");
        let fields: Vec<u32> = header[4..]
            .chunks_exact(4)
            .map(|chunk| u32::from_be_bytes(chunk.try_into().expect("four-byte field")))
            .collect();
        assert_eq!(fields, [24, u32::MAX, 3, 32768, 2]);
        assert_eq!(StreamFormat::AuBigEndian.sample(0x1234), [0x12, 0x34]);
        assert_eq!(StreamFormat::AuBigEndian.sample(-2), [0xff, 0xfe]);
        assert!(StreamFormat::RawLittleEndian.header().is_empty());
        assert_eq!(StreamFormat::RawLittleEndian.sample(0x1234), [0x34, 0x12]);
    }
}
