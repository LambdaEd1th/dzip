//! Adapter for the optional native DZ range/LZ codec.

#[cfg(all(not(feature = "dz"), feature = "encode"))]
use super::{Codec, CodecError};
use crate::{RangeSettings, Result};

#[cfg(feature = "dz")]
pub use dz::DzCommonBuffer;

/// Archive-wide native-DZ encoder and COMBUF analysis settings.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DzOptions {
    pub settings: RangeSettings,
    pub max_mem_usage: i32,
    pub use_combuf: bool,
    pub preprocess: bool,
    pub trim_reference_factor: i32,
    pub max_common_match: usize,
}

impl Default for DzOptions {
    fn default() -> Self {
        Self {
            settings: RangeSettings::default(),
            max_mem_usage: -1,
            use_combuf: false,
            preprocess: true,
            trim_reference_factor: 20,
            max_common_match: usize::MAX,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DzDecodeContext {
    pub settings: RangeSettings,
    #[cfg(feature = "dz")]
    common_buffer: Option<DzCommonBuffer>,
}

impl DzDecodeContext {
    pub(crate) fn from_encoded_chunks(
        settings: RangeSettings,
        chunks: Vec<Vec<u8>>,
    ) -> Result<Self> {
        #[cfg(feature = "dz")]
        {
            let common_buffer = if chunks.is_empty() {
                None
            } else {
                Some(DzCommonBuffer::new(settings.into(), chunks)?)
            };
            Ok(Self {
                settings,
                common_buffer,
            })
        }
        #[cfg(not(feature = "dz"))]
        {
            let _ = chunks;
            Ok(Self { settings })
        }
    }

    pub const fn has_common_buffer(&self) -> bool {
        #[cfg(feature = "dz")]
        {
            self.common_buffer.is_some()
        }
        #[cfg(not(feature = "dz"))]
        {
            false
        }
    }
}

#[cfg(feature = "encode")]
pub(crate) struct EncodedDzArchive {
    pub(crate) chunks: Vec<Vec<u8>>,
    pub(crate) common_buffer: Option<Vec<u8>>,
}

#[cfg(feature = "dz")]
pub(crate) fn encode(input: &[u8], settings: RangeSettings) -> Result<Vec<u8>> {
    dz::compress_chunk(input, settings.into()).map_err(Into::into)
}

#[cfg(feature = "dz")]
pub(crate) fn decode(
    input: &[u8],
    expected_length: usize,
    context: &DzDecodeContext,
) -> Result<Vec<u8>> {
    dz::decompress_chunk_with_common_buffer(
        input,
        expected_length,
        context.settings.into(),
        context.common_buffer.as_ref(),
    )
    .map_err(Into::into)
}

#[cfg(feature = "encode")]
pub(crate) fn encode_archive(inputs: &[Vec<u8>], options: &DzOptions) -> Result<EncodedDzArchive> {
    #[cfg(feature = "dz")]
    {
        let encoded = dz::compress_archive(inputs, &options.to_engine())?;
        Ok(EncodedDzArchive {
            chunks: encoded.chunks,
            common_buffer: encoded.common_buffer,
        })
    }
    #[cfg(not(feature = "dz"))]
    {
        let _ = (inputs, options);
        Err(CodecError::Unavailable { codec: Codec::Dz }.into())
    }
}

#[cfg(all(feature = "dz", feature = "encode"))]
impl DzOptions {
    fn to_engine(&self) -> dz::DzEncoderOptions {
        dz::DzEncoderOptions {
            settings: self.settings.into(),
            max_mem_usage: self.max_mem_usage,
            use_combuf: self.use_combuf,
            preprocess: self.preprocess,
            trim_reference_factor: self.trim_reference_factor,
            max_common_match: self.max_common_match,
        }
    }
}

#[cfg(feature = "dz")]
impl From<RangeSettings> for dz::RangeSettings {
    fn from(settings: RangeSettings) -> Self {
        Self {
            win_size: settings.win_size,
            flags: settings.flags,
            offset_table_size: settings.offset_table_size,
            offset_tables: settings.offset_tables,
            offset_contexts: settings.offset_contexts,
            ref_length_table_size: settings.ref_length_table_size,
            ref_length_tables: settings.ref_length_tables,
            ref_offset_table_size: settings.ref_offset_table_size,
            ref_offset_tables: settings.ref_offset_tables,
            big_min_match: settings.big_min_match,
        }
    }
}
