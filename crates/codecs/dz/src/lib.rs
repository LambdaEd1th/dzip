//! Native Dzip range/LZ and archive-wide common-buffer codec.

mod archive;
mod chunk;
mod error;
mod matchfinder;
mod model;
mod range;
mod settings;

pub use archive::common::DzCommonBuffer;
pub use archive::{DzEncoderOptions, EncodedDzArchive, compress_archive};
pub use chunk::{compress_chunk, decompress_chunk, decompress_chunk_with_common_buffer};
pub use error::{DzError, Result};
pub use settings::RangeSettings;

pub(crate) use DzError as DzipError;

pub(crate) const END_SYMBOL: usize = 513;
pub(crate) const MIN_MATCH: usize = 2;
pub(crate) const MAX_MATCH: usize = 258;

#[cfg(test)]
mod tests;
