//! Versioned, checksummed save-state container framing.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{Deserialize, Deserializer, SeqAccess, Visitor};
use sha2::{Digest, Sha256};

const MAX_SEQUENCE_ELEMENTS: usize = 1_048_576;
const MAGIC: &[u8; 8] = b"GBASTATE";
const VERSION: u16 = 1;
const HEADER_SIZE: usize = 8 + 2 + 32 + 4 + 32;
/// Maximum encoded save-state file size accepted by the core.
pub const MAX_SAVE_STATE_SIZE: usize = 4 * 1024 * 1024;

pub(crate) fn deserialize_bounded_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct BoundedVec<T>(PhantomData<T>);

    impl<'de, T> Visitor<'de> for BoundedVec<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "a sequence with at most {MAX_SEQUENCE_ELEMENTS} elements"
            )
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let length = sequence.size_hint().unwrap_or(0);
            if length > MAX_SEQUENCE_ELEMENTS {
                return Err(serde::de::Error::invalid_length(length, &self));
            }
            let mut values = Vec::with_capacity(length);
            while let Some(value) = sequence.next_element()? {
                if values.len() == MAX_SEQUENCE_ELEMENTS {
                    return Err(serde::de::Error::invalid_length(values.len() + 1, &self));
                }
                values.push(value);
            }
            Ok(values)
        }
    }

    deserializer.deserialize_seq(BoundedVec(PhantomData))
}

pub(crate) fn deserialize_bounded_box<'de, D, T>(deserializer: D) -> Result<Box<[T]>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    deserialize_bounded_vec(deserializer).map(Vec::into_boxed_slice)
}

pub(crate) fn deserialize_bounded_deque<'de, D, T>(deserializer: D) -> Result<VecDeque<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    deserialize_bounded_vec(deserializer).map(VecDeque::from)
}

/// Invalid, incompatible, or corrupt emulator save-state data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StateError {
    /// The file is truncated, oversized, malformed, or has an invalid checksum.
    InvalidData,
    /// The file was written by an unsupported save-state version.
    UnsupportedVersion,
    /// The state belongs to a different ROM image.
    DifferentRom,
    /// Saving a machine with a latched execution fault is not supported.
    FaultedMachine,
}

impl fmt::Display for StateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidData => formatter.write_str("invalid or corrupt save state"),
            Self::UnsupportedVersion => formatter.write_str("unsupported save-state version"),
            Self::DifferentRom => formatter.write_str("save state belongs to a different ROM"),
            Self::FaultedMachine => formatter.write_str("cannot save a faulted emulation machine"),
        }
    }
}

impl Error for StateError {}

pub(crate) fn encode(rom_hash: [u8; 32], payload: &[u8]) -> Result<Vec<u8>, StateError> {
    if payload.len() > MAX_SAVE_STATE_SIZE - HEADER_SIZE {
        return Err(StateError::InvalidData);
    }
    let payload_hash = Sha256::digest(payload);
    let mut output = Vec::with_capacity(HEADER_SIZE + payload.len());
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&VERSION.to_le_bytes());
    output.extend_from_slice(&rom_hash);
    output.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    output.extend_from_slice(&payload_hash);
    output.extend_from_slice(payload);
    Ok(output)
}

pub(crate) fn decode<'a>(bytes: &'a [u8], expected_rom: &[u8; 32]) -> Result<&'a [u8], StateError> {
    if bytes.len() < HEADER_SIZE || bytes.len() > MAX_SAVE_STATE_SIZE || &bytes[..8] != MAGIC {
        return Err(StateError::InvalidData);
    }
    let version = u16::from_le_bytes(bytes[8..10].try_into().expect("fixed header slice"));
    if version != VERSION {
        return Err(StateError::UnsupportedVersion);
    }
    if &bytes[10..42] != expected_rom {
        return Err(StateError::DifferentRom);
    }
    let payload_len =
        u32::from_le_bytes(bytes[42..46].try_into().expect("fixed header slice")) as usize;
    if payload_len != bytes.len() - HEADER_SIZE {
        return Err(StateError::InvalidData);
    }
    let payload_hash = Sha256::digest(&bytes[HEADER_SIZE..]);
    if payload_hash.as_slice() != &bytes[46..HEADER_SIZE] {
        return Err(StateError::InvalidData);
    }
    Ok(&bytes[HEADER_SIZE..])
}

#[cfg(test)]
mod tests {
    use super::{StateError, decode, encode};

    #[test]
    fn bounded_sequence_rejects_untrusted_length_before_allocating() {
        #[derive(serde::Deserialize)]
        struct Sequence {
            #[serde(deserialize_with = "super::deserialize_bounded_vec")]
            values: Vec<u8>,
        }

        let bytes = postcard::to_allocvec(&(1_048_577usize,)).expect("oversized length");
        assert!(postcard::from_bytes::<Sequence>(&bytes).is_err());
        let valid = postcard::to_allocvec(&vec![7u8]).expect("bounded sequence");
        assert_eq!(
            postcard::from_bytes::<Sequence>(&valid)
                .expect("decode")
                .values,
            [7]
        );
    }

    #[test]
    fn state_container_checks_identity_version_length_and_checksum() {
        let rom = [0x5a; 32];
        let encoded = encode(rom, b"snapshot payload").expect("encode");
        assert_eq!(decode(&encoded, &rom), Ok(&b"snapshot payload"[..]));
        assert_eq!(decode(&encoded, &[0; 32]), Err(StateError::DifferentRom));
        assert_eq!(
            decode(&encoded[..encoded.len() - 1], &rom),
            Err(StateError::InvalidData)
        );
        assert_eq!(decode(&encoded[..20], &rom), Err(StateError::InvalidData));
        let mut corrupt = encoded.clone();
        *corrupt.last_mut().expect("payload") ^= 1;
        assert_eq!(decode(&corrupt, &rom), Err(StateError::InvalidData));
        let mut future = encoded;
        future[8..10].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(decode(&future, &rom), Err(StateError::UnsupportedVersion));
    }
}
