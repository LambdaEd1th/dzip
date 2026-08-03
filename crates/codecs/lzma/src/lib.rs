//! # lzma
//!
//! A pure-Rust, single-threaded LZMA encoder retargeted to the **7-zip LZMA SDK
//! 9.20** decisions used by dzip.exe.
//!
//! The emitted stream is a **raw LZMA stream**: no 13-byte `.lzma` file header
//! and, for [`encode`], no end-of-stream marker. The
//! [`encode_with_end_marker`] entry point reproduces `writeEndMark = 1`. The 5
//! decoder property bytes are produced separately by [`decoder_props`].
//!
//! The dzip property set is regression-tested against a recorded SDK 9.20 C
//! oracle, including an input that produces a different stream under SDK 23.01.

mod price;
mod props;
mod rangecoder;
mod state;

mod matchfinder;
mod optimum;

mod encoder;

#[cfg(any(test, feature = "decode"))]
mod decoder;

#[cfg(test)]
mod roundtrip_tests;

pub use props::LzmaProps;

/// Encode `input` into a raw LZMA stream that is byte-identical to
/// `LzmaEnc_MemEncode(..., writeEndMark = 0, ...)` for the same `props`.
///
/// The returned bytes carry **no** `.lzma` header and **no** end marker. Obtain
/// the 5 decoder property bytes with [`decoder_props`].
pub fn encode(input: &[u8], props: &LzmaProps) -> Vec<u8> {
    encoder::encode(input, props)
}

/// Encode a raw LZMA stream with the SDK end marker (`writeEndMark = 1`).
pub fn encode_with_end_marker(input: &[u8], props: &LzmaProps) -> Vec<u8> {
    encoder::encode_with_end_marker(input, props)
}

/// The 5 decoder property bytes for `props`, identical to
/// `LzmaEnc_WriteProperties`.
///
/// Byte 0 packs `(pb*5 + lp)*9 + lc`; bytes 1..5 are the little-endian *aligned*
/// dictionary size (see [`LzmaProps::decoder_props`] — the encoder rounds the
/// dictionary up before writing it, it does not emit the raw `dict_size`).
pub fn decoder_props(props: &LzmaProps) -> [u8; 5] {
    props.decoder_props()
}

/// Decode a raw LZMA stream (no header, no end marker) of known output length.
///
/// A port of `LzmaDec` provided for round-trip self-tests. Available to external
/// consumers only with the `decode` feature enabled.
#[cfg(any(test, feature = "decode"))]
pub use decoder::DecodeError;

#[cfg(any(test, feature = "decode"))]
pub fn decode_raw(input: &[u8], props: &[u8; 5], out_len: usize) -> Result<Vec<u8>, DecodeError> {
    decoder::decode_raw(input, props, out_len)
}
