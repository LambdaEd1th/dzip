//! Pure-Rust reproduction of the LZMA stream emitted by dzip 1.1.3.
//!
//! dzip initializes the LZMA SDK 9.20 level-5 defaults, overrides the
//! dictionary to 64 KiB, and enables the end marker. The vendored encoder is
//! kept byte-exact with that SDK version for these properties.

use super::{Codec, CodecError};
use crate::Result;
use lzma::{LzmaProps, decode_raw, decoder_props, encode_with_end_marker};

pub(crate) fn encode(input: &[u8]) -> Result<Vec<u8>> {
    let properties = LzmaProps {
        lc: 3,
        lp: 0,
        pb: 2,
        dict_size: 0x10000,
        fb: 32,
        mc: 32,
    };
    let raw = encode_with_end_marker(input, &properties);

    let mut output = Vec::with_capacity(13 + raw.len());
    output.extend_from_slice(&decoder_props(&properties));
    output.extend_from_slice(&(input.len() as u64).to_le_bytes());
    output.extend_from_slice(&raw);
    Ok(output)
}

pub(crate) fn decode(input: &[u8], expected_length: usize) -> Result<Vec<u8>> {
    let header = input
        .get(..13)
        .ok_or_else(|| codec_error("truncated LZMA-alone header"))?;
    let properties: [u8; 5] = header[..5]
        .try_into()
        .expect("the five-byte properties slice has a fixed length");
    let declared_length = u64::from_le_bytes(
        header[5..13]
            .try_into()
            .expect("the eight-byte length slice has a fixed length"),
    );
    let expected_u64 = u64::try_from(expected_length)
        .map_err(|_| codec_error("expected output length does not fit u64"))?;
    if declared_length != u64::MAX && declared_length != expected_u64 {
        return Err(codec_error(&format!(
            "decompressed length mismatch: chunk expects {expected_length}, header declares {declared_length}"
        )));
    }

    decode_raw(&input[13..], &properties, expected_length)
        .map_err(|error| codec_error(&error.to_string()))
}

fn codec_error(message: &str) -> crate::DzipError {
    CodecError::invalid(Codec::Lzma, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn empty_stream_matches_sdk_920() {
        assert_eq!(
            encode(&[]).unwrap(),
            [
                0x5d, 0x00, 0x00, 0x01, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x83, 0xff, 0xfb, 0xff,
                0xff, 0xc0, 0x00, 0x00, 0x00,
            ]
        );
    }

    #[test]
    fn high_entropy_stream_matches_sdk_920_oracle() {
        // Carrying the PRNG state through these lengths reproduces an oracle
        // input that distinguishes SDK 9.20's parser from SDK 23.01.
        let mut state = 0x9e37_79b9u32;
        let mut fixture = Vec::new();
        for length in [
            2usize, 3, 4, 7, 8, 15, 16, 31, 32, 63, 64, 255, 256, 1023, 4096, 32_767, 65_535,
        ] {
            fixture = (0..length)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state as u8
                })
                .collect();
        }

        let stream = encode(&fixture).unwrap();
        assert_eq!(stream.len(), 66_477);
        let digest = Sha256::digest(stream);
        assert_eq!(
            &digest[..],
            &[
                0xa1, 0x9b, 0xb5, 0x1d, 0x97, 0xae, 0xda, 0xdc, 0xe4, 0x76, 0x72, 0x94, 0xfa, 0xe1,
                0xe6, 0xe5, 0x14, 0xc7, 0xd7, 0x25, 0x7e, 0x3c, 0xa6, 0xdb, 0x89, 0x40, 0x3b, 0xde,
                0x20, 0x41, 0x7e, 0xf7,
            ]
        );
    }

    #[test]
    fn lzma_alone_round_trip_uses_local_decoder() {
        for input in [
            Vec::new(),
            b"local LZMA decoder".repeat(100),
            (0..=255).cycle().take(100_000).collect(),
        ] {
            let encoded = encode(&input).unwrap();
            assert_eq!(decode(&encoded, input.len()).unwrap(), input);
        }
    }

    #[test]
    fn malformed_lzma_alone_streams_return_errors() {
        assert!(decode(&[], 0).is_err());

        let mut mismatched = encode(b"payload").unwrap();
        mismatched[5..13].copy_from_slice(&8u64.to_le_bytes());
        assert!(decode(&mismatched, 7).is_err());

        let mut truncated = encode(b"payload").unwrap();
        truncated.truncate(15);
        assert!(decode(&truncated, 7).is_err());
    }
}
