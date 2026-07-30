//! Low-level binary-format API for inspection and reverse-engineering tools.
//!
//! Most applications should use [`crate::Archive`] and
//! [`crate::ArchiveBuilder`] instead.

pub use crate::format::{
    ArchiveSettings, CHUNK_BZIP, CHUNK_COMBUF, CHUNK_COPYCOMP, CHUNK_DZ, CHUNK_JPEG, CHUNK_LZMA,
    CHUNK_MP3, CHUNK_RANDOMACCESS, CHUNK_ZERO, CHUNK_ZLIB, Chunk, ChunkSettings, RangeSettings,
};
pub use crate::reader::{DzDecodeContext, DzipReader, ReadSeek, VolumeSource, correct_chunk_sizes};
pub use crate::writer::DzipWriter;

/// Direct access to the native DZ engine for format-analysis tools.
#[cfg(feature = "dz")]
pub mod dz {
    pub use dz_rs::*;
}

#[cfg(feature = "encode")]
pub use crate::writer::compress_data;
