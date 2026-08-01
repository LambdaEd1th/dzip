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
    /// Preserves legacy chunks whose independent MP3 and JPEG bits are both set.
    Mp3AndJpeg,
}

/// Fully interpreted on-disk chunk encoding.
///
/// This type intentionally preserves orthogonal Dzip flags instead of
/// collapsing them into a single codec value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkEncoding {
    pub compression: Compression,
    /// Requests retaining the whole decoded chunk for random seeking.
    ///
    /// The high-level Rust reader already materializes complete chunks, so
    /// this remains metadata there. The original streaming decoder used it to
    /// raise the minimum output-buffer size.
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

    /// Interpret the storage bits using dzip.exe 1.1.3's packer registration
    /// order. MP3, JPEG, and random-access are orthogonal metadata and never
    /// select a codec by themselves.
    ///
    /// Well-formed chunks contain one storage bit. The precedence only matters
    /// for legacy `.dcl` input that combined multiple coder keywords.
    pub fn from_flags(flags: u16) -> Result<Self> {
        let compression = if flags & CHUNK_ZERO != 0 {
            Compression::Zero
        } else if flags & CHUNK_BZIP != 0 {
            Compression::Bzip
        } else if flags & CHUNK_COPYCOMP != 0 {
            Compression::Copy
        } else if flags & CHUNK_ZLIB != 0 {
            Compression::Zlib
        } else if flags & CHUNK_LZMA != 0 {
            Compression::Lzma
        } else if flags & CHUNK_DZ != 0 {
            Compression::Dz
        } else {
            return Err(DzipError::UnsupportedCompression(flags));
        };

        let content_hint = match (flags & CHUNK_MP3 != 0, flags & CHUNK_JPEG != 0) {
            (true, true) => Some(ContentHint::Mp3AndJpeg),
            (true, false) => Some(ContentHint::Mp3),
            (false, true) => Some(ContentHint::Jpeg),
            (false, false) => None,
        };

        Ok(Self {
            compression,
            random_access: flags & CHUNK_RANDOMACCESS != 0,
            common_buffer: flags & CHUNK_COMBUF != 0,
            content_hint,
            unknown_flags: flags & !Self::KNOWN_FLAGS,
        })
    }

    pub const fn to_flags(self) -> u16 {
        let mut flags = self.compression.flag() | self.unknown_flags;
        if self.random_access {
            flags |= CHUNK_RANDOMACCESS;
        }
        if self.common_buffer {
            flags |= CHUNK_COMBUF;
        }
        flags |= match self.content_hint {
            Some(ContentHint::Mp3) => CHUNK_MP3,
            Some(ContentHint::Jpeg) => CHUNK_JPEG,
            Some(ContentHint::Mp3AndJpeg) => CHUNK_MP3 | CHUNK_JPEG,
            None => 0,
        };
        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_flags_round_trip_with_orthogonal_metadata() {
        for flags in [
            CHUNK_COPYCOMP,
            CHUNK_COPYCOMP | CHUNK_RANDOMACCESS | CHUNK_MP3,
            CHUNK_COPYCOMP | CHUNK_MP3 | CHUNK_JPEG,
            CHUNK_ZLIB | CHUNK_RANDOMACCESS,
            CHUNK_ZLIB | CHUNK_JPEG,
            CHUNK_BZIP,
            CHUNK_LZMA,
            CHUNK_DZ | CHUNK_MP3,
            CHUNK_ZERO,
            CHUNK_ZLIB | 0x8000,
        ] {
            assert_eq!(ChunkEncoding::from_flags(flags).unwrap().to_flags(), flags);
        }
    }

    #[test]
    fn media_and_random_access_flags_do_not_select_copy() {
        let dz_mp3 = ChunkEncoding::from_flags(CHUNK_DZ | CHUNK_MP3).unwrap();
        assert_eq!(dz_mp3.compression, Compression::Dz);
        assert_eq!(dz_mp3.content_hint, Some(ContentHint::Mp3));

        let zlib_jpeg =
            ChunkEncoding::from_flags(CHUNK_ZLIB | CHUNK_JPEG | CHUNK_RANDOMACCESS).unwrap();
        assert_eq!(zlib_jpeg.compression, Compression::Zlib);
        assert_eq!(zlib_jpeg.content_hint, Some(ContentHint::Jpeg));
        assert!(zlib_jpeg.random_access);
    }

    #[test]
    fn metadata_only_flags_are_not_decodable_chunks() {
        for flags in [
            CHUNK_MP3,
            CHUNK_JPEG,
            CHUNK_RANDOMACCESS,
            CHUNK_MP3 | CHUNK_JPEG | CHUNK_RANDOMACCESS,
        ] {
            assert!(matches!(
                ChunkEncoding::from_flags(flags),
                Err(DzipError::UnsupportedCompression(value)) if value == flags
            ));
        }
    }

    #[test]
    fn combined_storage_flags_use_original_packer_order() {
        let flags = CHUNK_ZERO | CHUNK_BZIP | CHUNK_COPYCOMP | CHUNK_ZLIB | CHUNK_LZMA | CHUNK_DZ;
        assert_eq!(
            ChunkEncoding::from_flags(flags).unwrap().compression,
            Compression::Zero
        );
        assert_eq!(
            ChunkEncoding::from_flags(flags & !CHUNK_ZERO)
                .unwrap()
                .compression,
            Compression::Bzip
        );
        assert_eq!(
            ChunkEncoding::from_flags(flags & !(CHUNK_ZERO | CHUNK_BZIP))
                .unwrap()
                .compression,
            Compression::Copy
        );
        assert_eq!(
            ChunkEncoding::from_flags(flags & !(CHUNK_ZERO | CHUNK_BZIP | CHUNK_COPYCOMP))
                .unwrap()
                .compression,
            Compression::Zlib
        );
        assert_eq!(
            ChunkEncoding::from_flags(
                flags & !(CHUNK_ZERO | CHUNK_BZIP | CHUNK_COPYCOMP | CHUNK_ZLIB)
            )
            .unwrap()
            .compression,
            Compression::Lzma
        );
    }
}
