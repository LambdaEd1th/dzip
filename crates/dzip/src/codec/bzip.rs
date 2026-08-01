use super::{Codec, CodecError};
use crate::Result;

/// Reproduces dzip.exe's libbzip2 settings: blockSize100k=1, verbosity=0,
/// workFactor=30.
pub(crate) fn encode(input: &[u8]) -> Result<Vec<u8>> {
    // dzip's archive layer does not invoke the external coder for empty
    // chunks; it stores a zero-length physical stream with the codec flag.
    if input.is_empty() {
        return Ok(Vec::new());
    }
    u32::try_from(input.len()).map_err(|_| codec_error("input exceeds 32-bit limit"))?;
    // The public libbzip2 API specifies source + 1% + 600 bytes.
    let capacity = input
        .len()
        .checked_add(input.len().div_ceil(100))
        .and_then(|length| length.checked_add(601))
        .ok_or_else(|| codec_error("compressed-size bound overflow"))?;
    u32::try_from(capacity)
        .map_err(|_| codec_error("compressed-size bound exceeds 32-bit limit"))?;
    let mut output = vec![0; capacity];

    let output_length = bzip::compress_buffer(input, &mut output, 1, 0, 30)
        .map_err(|code| codec_error(&format!("compression failed with code {code}")))?;
    output.truncate(output_length);
    Ok(output)
}

pub(crate) fn decode(input: &[u8], expected_length: usize) -> Result<Vec<u8>> {
    if input.is_empty() && expected_length == 0 {
        return Ok(Vec::new());
    }
    u32::try_from(input.len()).map_err(|_| codec_error("input exceeds 32-bit limit"))?;
    let capacity = expected_length
        .checked_add(1)
        .ok_or_else(|| codec_error("decompressed-size bound overflow"))?;
    u32::try_from(capacity)
        .map_err(|_| codec_error("decompressed-size bound exceeds 32-bit limit"))?;
    let mut output = vec![0; capacity];

    let written = bzip::decompress_buffer(input, &mut output, 0, 0)
        .map_err(|code| codec_error(&format!("decompression failed with code {code}")))?;
    if written != expected_length {
        return Err(CodecError::LengthMismatch {
            codec: Codec::Bzip,
            expected: expected_length,
            actual: written,
        }
        .into());
    }
    output.truncate(written);
    Ok(output)
}

fn codec_error(message: &str) -> crate::DzipError {
    CodecError::invalid(Codec::Bzip, message).into()
}
