//! On-disk Dzip format structures and constants.

mod chunk;
mod settings;

pub use chunk::{
    CHUNK_BZIP, CHUNK_COMBUF, CHUNK_COPYCOMP, CHUNK_DZ, CHUNK_JPEG, CHUNK_LZMA, CHUNK_MP3,
    CHUNK_RANDOMACCESS, CHUNK_ZERO, CHUNK_ZLIB, Chunk,
};
pub use settings::{ArchiveSettings, ChunkSettings, RangeSettings};
