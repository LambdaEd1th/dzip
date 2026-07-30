//! Dzip compression choices and on-disk chunk-flag interpretation.

use crate::format::{
    CHUNK_BZIP, CHUNK_COMBUF, CHUNK_COPYCOMP, CHUNK_DZ, CHUNK_JPEG, CHUNK_LZMA, CHUNK_MP3,
    CHUNK_RANDOMACCESS, CHUNK_ZERO, CHUNK_ZLIB,
};
use crate::{DzipError, Result};
use std::fmt;
use std::str::FromStr;

/// A compression engine used by Dzip.
///
/// Copy and zero-filled storage are represented by [`Compression`] because
/// they are storage strategies rather than compression codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Codec {
    Bzip,
    Zlib,
    Lzma,
    Dz,
}

impl Codec {
    pub const fn flag(self) -> u16 {
        match self {
            Self::Bzip => CHUNK_BZIP,
            Self::Zlib => CHUNK_ZLIB,
            Self::Lzma => CHUNK_LZMA,
            Self::Dz => CHUNK_DZ,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Bzip => "bzip",
            Self::Zlib => "zlib",
            Self::Lzma => "lzma",
            Self::Dz => "dz",
        }
    }
}

impl fmt::Display for Codec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// How an archive entry is stored.
///
/// DZ encoder settings are archive-wide and live in
/// [`crate::PackOptions::dz`]; keeping them out of this per-entry enum avoids
/// conflicting COMBUF settings inside one archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Compression {
    Copy,
    Zero,
    Bzip,
    Zlib,
    Lzma,
    Dz,
}

impl Compression {
    pub const fn codec(self) -> Option<Codec> {
        match self {
            Self::Copy | Self::Zero => None,
            Self::Bzip => Some(Codec::Bzip),
            Self::Zlib => Some(Codec::Zlib),
            Self::Lzma => Some(Codec::Lzma),
            Self::Dz => Some(Codec::Dz),
        }
    }

    pub const fn flag(self) -> u16 {
        match self {
            Self::Copy => CHUNK_COPYCOMP,
            Self::Zero => CHUNK_ZERO,
            Self::Bzip => CHUNK_BZIP,
            Self::Zlib => CHUNK_ZLIB,
            Self::Lzma => CHUNK_LZMA,
            Self::Dz => CHUNK_DZ,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Zero => "zero",
            Self::Bzip => "bzip",
            Self::Zlib => "zlib",
            Self::Lzma => "lzma",
            Self::Dz => "dz",
        }
    }
}

impl From<Codec> for Compression {
    fn from(codec: Codec) -> Self {
        match codec {
            Codec::Bzip => Self::Bzip,
            Codec::Zlib => Self::Zlib,
            Codec::Lzma => Self::Lzma,
            Codec::Dz => Self::Dz,
        }
    }
}

impl fmt::Display for Compression {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCompressionError {
    value: String,
}

impl fmt::Display for ParseCompressionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unknown Dzip compression strategy: {}",
            self.value
        )
    }
}

impl std::error::Error for ParseCompressionError {}

impl FromStr for Compression {
    type Err = ParseCompressionError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "copy" => Ok(Self::Copy),
            "zero" => Ok(Self::Zero),
            "bzip" | "bzip2" => Ok(Self::Bzip),
            "zlib" => Ok(Self::Zlib),
            "lzma" => Ok(Self::Lzma),
            "dz" => Ok(Self::Dz),
            _ => Err(ParseCompressionError {
                value: value.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ContentHint {
    Mp3,
    Jpeg,
}

/// Fully interpreted on-disk chunk encoding.
///
/// This type intentionally preserves orthogonal Dzip flags instead of
/// collapsing them into a single codec value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkEncoding {
    pub compression: Compression,
    /// Whether the original flags explicitly contained `CHUNK_COPYCOMP`.
    /// Media-hint-only chunks decode as copy without setting this bit.
    pub explicit_copy: bool,
    pub random_access: bool,
    pub common_buffer: bool,
    pub content_hint: Option<ContentHint>,
    pub unknown_flags: u16,
}

impl ChunkEncoding {
    pub const KNOWN_FLAGS: u16 = CHUNK_COMBUF
        | CHUNK_DZ
        | CHUNK_ZLIB
        | CHUNK_BZIP
        | CHUNK_MP3
        | CHUNK_JPEG
        | CHUNK_ZERO
        | CHUNK_COPYCOMP
        | CHUNK_LZMA
        | CHUNK_RANDOMACCESS;

    /// Interpret flags using dzip.exe-compatible precedence: zero,
    /// copy/media, zlib, bzip, LZMA, then native DZ.
    pub fn from_flags(flags: u16) -> Result<Self> {
        let compression = if flags & CHUNK_ZERO != 0 {
            Compression::Zero
        } else if flags & (CHUNK_COPYCOMP | CHUNK_MP3 | CHUNK_JPEG) != 0
            || flags & CHUNK_RANDOMACCESS != 0
                && flags & (CHUNK_LZMA | CHUNK_ZLIB | CHUNK_BZIP | CHUNK_DZ) == 0
        {
            Compression::Copy
        } else if flags & CHUNK_ZLIB != 0 {
            Compression::Zlib
        } else if flags & CHUNK_BZIP != 0 {
            Compression::Bzip
        } else if flags & CHUNK_LZMA != 0 {
            Compression::Lzma
        } else if flags & CHUNK_DZ != 0 {
            Compression::Dz
        } else {
            return Err(DzipError::UnsupportedCompression(flags));
        };

        let content_hint = if flags & CHUNK_MP3 != 0 {
            Some(ContentHint::Mp3)
        } else if flags & CHUNK_JPEG != 0 {
            Some(ContentHint::Jpeg)
        } else {
            None
        };

        Ok(Self {
            compression,
            explicit_copy: flags & CHUNK_COPYCOMP != 0,
            random_access: flags & CHUNK_RANDOMACCESS != 0,
            common_buffer: flags & CHUNK_COMBUF != 0,
            content_hint,
            unknown_flags: flags & !Self::KNOWN_FLAGS,
        })
    }

    pub const fn to_flags(self) -> u16 {
        let compression_flag = if matches!(self.compression, Compression::Copy) {
            if self.explicit_copy {
                CHUNK_COPYCOMP
            } else {
                0
            }
        } else {
            self.compression.flag()
        };
        let mut flags = compression_flag | self.unknown_flags;
        if self.random_access {
            flags |= CHUNK_RANDOMACCESS;
        }
        if self.common_buffer {
            flags |= CHUNK_COMBUF;
        }
        flags |= match self.content_hint {
            Some(ContentHint::Mp3) => CHUNK_MP3,
            Some(ContentHint::Jpeg) => CHUNK_JPEG,
            None => 0,
        };
        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_chunk_flags_round_trip_without_normalization() {
        for flags in [
            CHUNK_COPYCOMP,
            CHUNK_MP3,
            CHUNK_JPEG,
            CHUNK_RANDOMACCESS,
            CHUNK_COPYCOMP | CHUNK_RANDOMACCESS | CHUNK_MP3,
            CHUNK_ZLIB | CHUNK_RANDOMACCESS,
            CHUNK_BZIP,
            CHUNK_LZMA,
            CHUNK_DZ,
            CHUNK_ZERO,
            CHUNK_ZLIB | 0x8000,
        ] {
            assert_eq!(ChunkEncoding::from_flags(flags).unwrap().to_flags(), flags);
        }
    }
}
