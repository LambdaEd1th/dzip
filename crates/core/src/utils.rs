use crate::format::{ChunkFlags, FLAG_MAPPINGS};
use std::borrow::Cow;
use std::io::BufRead;

/// Reads a null-terminated string from a reader.
pub fn read_null_term_string<R: BufRead>(reader: &mut R) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    reader.read_until(0, &mut bytes)?;
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    String::from_utf8(bytes).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Encodes a list of string flags into a u16 bitmask using FLAG_MAPPINGS.
pub fn encode_flags(flags: &[Cow<'_, str>]) -> u16 {
    let mut mask = ChunkFlags::empty();

    for input_flag in flags {
        for (flag_bit, flag_str) in FLAG_MAPPINGS {
            if input_flag.as_ref() == *flag_str {
                mask.insert(*flag_bit);
            }
        }
    }

    mask.bits()
}

/// Decodes a u16 bitmask into a list of string flags using FLAG_MAPPINGS.
pub fn decode_flags(bits: u16) -> Vec<Cow<'static, str>> {
    let flags = ChunkFlags::from_bits_truncate(bits);
    let mut list = Vec::new();

    for (flag_bit, flag_str) in FLAG_MAPPINGS {
        if flags.contains(*flag_bit) {
            list.push(Cow::Borrowed(*flag_str));
        }
    }

    list
}
