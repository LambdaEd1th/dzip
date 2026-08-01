use super::{Codec, CodecError};
use crate::Result;
use zlib::{Deflate, DeflateFlush, Inflate, InflateFlush, Status};

const GZIP_HEADER: [u8; 10] = [
    0x1f, 0x8b, 8, 0, // magic, deflate, no optional fields
    0, 0, 0, 0, // timestamp
    0, 11, // default compression, Windows/NTFS
];

/// Reproduces dzip.exe's zlib 1.1.3 writer: a gzip header followed by a raw
/// DEFLATE stream, deliberately without the normal CRC32/ISIZE trailer.
pub(crate) fn encode(input: &[u8]) -> Result<Vec<u8>> {
    // dzip's archive layer does not invoke the external coder for empty
    // chunks; it stores a zero-length physical stream with the codec flag.
    if input.is_empty() {
        return Ok(Vec::new());
    }
    if input.len() > u32::MAX as usize {
        return Err(codec_error("input exceeds zlib 1.1.3's 32-bit limit"));
    }

    // zlib 1.1.3 documents source + 0.1% + 12 bytes as sufficient for a
    // one-shot stream. Add one extra byte for integer rounding.
    let deflate_bound = input
        .len()
        .checked_add(input.len().div_ceil(1000))
        .and_then(|length| length.checked_add(13))
        .ok_or_else(|| codec_error("compressed-size bound overflow"))?;
    let mut raw = vec![0; deflate_bound];
    let mut stream = Deflate::new(6, false, 15);
    let status = stream
        .compress(input, &mut raw, DeflateFlush::Finish)
        .map_err(|error| codec_error(error.as_str()))?;
    if status != Status::StreamEnd {
        return Err(codec_error("output buffer was unexpectedly too small"));
    }
    let written = usize::try_from(stream.total_out())
        .map_err(|_| codec_error("compressed size does not fit usize"))?;
    raw.truncate(written);

    let mut output = Vec::with_capacity(GZIP_HEADER.len() + raw.len());
    output.extend_from_slice(&GZIP_HEADER);
    output.extend_from_slice(&raw);
    Ok(output)
}

/// Accepts both dzip's truncated-gzip framing and ordinary RFC 1950 zlib
/// streams, matching the two paths in the original decoder.
pub(crate) fn decode(input: &[u8], expected_length: usize) -> Result<Vec<u8>> {
    if input.is_empty() && expected_length == 0 {
        return Ok(Vec::new());
    }
    if input.len() > u32::MAX as usize || expected_length > u32::MAX as usize {
        return Err(codec_error("stream exceeds zlib 1.1.3's 32-bit limit"));
    }

    let (payload, zlib_header) = if input.starts_with(&[0x1f, 0x8b]) {
        (&input[gzip_payload_offset(input)?..], false)
    } else {
        (input, true)
    };

    // The spare byte both allows an empty stream to make progress and detects
    // an archive header that understates the decompressed size.
    let capacity = expected_length
        .checked_add(1)
        .ok_or_else(|| codec_error("decompressed-size bound overflow"))?;
    let mut output = vec![0; capacity];
    let mut stream = Inflate::new(zlib_header, 15);
    let status = stream
        .decompress(payload, &mut output, InflateFlush::Finish)
        .map_err(|error| codec_error(error.as_str()))?;
    if status != Status::StreamEnd {
        return Err(codec_error("truncated or incomplete deflate stream"));
    }
    let written = usize::try_from(stream.total_out())
        .map_err(|_| codec_error("decompressed size does not fit usize"))?;
    if written != expected_length {
        return Err(CodecError::LengthMismatch {
            codec: Codec::Zlib,
            expected: expected_length,
            actual: written,
        }
        .into());
    }
    output.truncate(written);
    Ok(output)
}

fn gzip_payload_offset(input: &[u8]) -> Result<usize> {
    if input.len() < GZIP_HEADER.len() {
        return Err(codec_error("truncated gzip header"));
    }
    if input[2] != 8 {
        return Err(codec_error("gzip stream does not use DEFLATE"));
    }

    let flags = input[3];
    if flags & 0xe0 != 0 {
        return Err(codec_error("gzip header uses reserved flags"));
    }

    let mut offset = GZIP_HEADER.len();
    if flags & 0x04 != 0 {
        let length_bytes = input
            .get(offset..offset + 2)
            .ok_or_else(|| codec_error("truncated gzip extra-field length"))?;
        let length = usize::from(u16::from_le_bytes([length_bytes[0], length_bytes[1]]));
        offset = offset
            .checked_add(2)
            .and_then(|value| value.checked_add(length))
            .filter(|&value| value <= input.len())
            .ok_or_else(|| codec_error("truncated gzip extra field"))?;
    }
    if flags & 0x08 != 0 {
        offset = skip_c_string(input, offset, "file name")?;
    }
    if flags & 0x10 != 0 {
        offset = skip_c_string(input, offset, "comment")?;
    }
    if flags & 0x02 != 0 {
        offset = offset
            .checked_add(2)
            .filter(|&value| value <= input.len())
            .ok_or_else(|| codec_error("truncated gzip header checksum"))?;
    }
    Ok(offset)
}

fn skip_c_string(input: &[u8], offset: usize, field: &str) -> Result<usize> {
    let relative_end = input
        .get(offset..)
        .and_then(|bytes| bytes.iter().position(|&byte| byte == 0))
        .ok_or_else(|| codec_error(&format!("unterminated gzip {field}")))?;
    Ok(offset + relative_end + 1)
}

fn codec_error(message: &str) -> crate::DzipError {
    CodecError::invalid(Codec::Zlib, message).into()
}
