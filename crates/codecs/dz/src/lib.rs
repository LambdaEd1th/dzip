//! Native Dzip range/LZ and archive-wide common-buffer codec.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;

mod archive;
mod chunk;
mod codec;
mod error;
mod matchfinder;
mod model;
mod options;
mod range;
mod settings;

pub use archive::common::DzCommonBuffer;
pub use archive::{DzEncoderOptions, EncodedDzArchive, compress_archive};
pub use chunk::{compress_chunk, decompress_chunk, decompress_chunk_with_common_buffer};
pub use codec::{Decoder, Encoder, decode, encode};
pub use error::{DzError, ErrorKind, Result};
pub use options::{DecoderOptions, EncoderOptions, ResourceLimits, model_workspace_size};
pub use settings::RangeSettings;

pub(crate) use DzError as DzipError;

pub(crate) const END_SYMBOL: usize = 513;
pub(crate) const MIN_MATCH: usize = 2;
pub(crate) const MAX_MATCH: usize = 258;

#[cfg(test)]
mod tests;
