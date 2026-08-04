use alloc::vec::Vec;

use crate::options::{encode_workspace_size, model_workspace_size};
use crate::{DecoderOptions, DzError, EncoderOptions, Result};

#[derive(Debug)]
pub struct Encoder {
    options: EncoderOptions,
    output: Vec<u8>,
}

impl Encoder {
    pub fn new(mut options: EncoderOptions) -> Result<Self> {
        options.settings = options.settings.validate()?;
        Ok(Self {
            options,
            output: Vec::new(),
        })
    }

    pub fn encode<'a>(&'a mut self, input: &[u8]) -> Result<&'a [u8]> {
        check_input(input.len(), self.options.limits.max_input_size)?;
        let workspace = encode_workspace_size(input.len(), self.options.settings)?;
        check_workspace(workspace, self.options.limits.max_workspace_size)?;
        let output = core::mem::take(&mut self.output);
        self.output =
            crate::chunk::compress_chunk_with_output(input, self.options.settings, output)?;
        check_output(self.output.len(), self.options.limits.max_output_size)?;
        Ok(&self.output)
    }

    pub fn output(&self) -> &[u8] {
        &self.output
    }

    pub fn take_output(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.output)
    }
}

#[derive(Debug)]
pub struct Decoder<'a> {
    options: DecoderOptions<'a>,
    output: Vec<u8>,
}

impl<'a> Decoder<'a> {
    pub fn new(mut options: DecoderOptions<'a>) -> Result<Self> {
        options.settings = options.settings.validate()?;
        if options.expected_size > options.limits.max_output_size {
            return Err(DzError::output_limit(
                "DZ declared output exceeds configured limit",
            ));
        }
        let workspace = model_workspace_size(options.settings, options.common_buffer.is_some())?;
        check_workspace(workspace, options.limits.max_workspace_size)?;
        Ok(Self {
            options,
            output: Vec::new(),
        })
    }

    pub fn decode<'b>(&'b mut self, input: &[u8]) -> Result<&'b [u8]> {
        check_input(input.len(), self.options.limits.max_input_size)?;
        let output = core::mem::take(&mut self.output);
        self.output = crate::chunk::decompress_chunk_with_output(
            input,
            self.options.expected_size,
            self.options.settings,
            self.options.common_buffer,
            output,
        )?;
        Ok(&self.output)
    }

    pub fn output(&self) -> &[u8] {
        &self.output
    }

    pub fn take_output(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.output)
    }
}

pub fn encode(input: &[u8], options: &EncoderOptions) -> Result<Vec<u8>> {
    let mut encoder = Encoder::new(*options)?;
    encoder.encode(input)?;
    Ok(encoder.take_output())
}

pub fn decode(input: &[u8], options: &DecoderOptions<'_>) -> Result<Vec<u8>> {
    let mut decoder = Decoder::new(*options)?;
    decoder.decode(input)?;
    Ok(decoder.take_output())
}

fn check_input(actual: usize, limit: usize) -> Result<()> {
    if actual > limit {
        return Err(DzError::input_limit("DZ input exceeds configured limit"));
    }
    Ok(())
}

fn check_output(actual: usize, limit: usize) -> Result<()> {
    if actual > limit {
        return Err(DzError::output_limit("DZ output exceeds configured limit"));
    }
    Ok(())
}

fn check_workspace(actual: usize, limit: usize) -> Result<()> {
    if actual > limit {
        return Err(DzError::workspace_limit(
            "DZ workspace exceeds configured limit",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorKind, RangeSettings, ResourceLimits};

    #[test]
    fn reusable_api_preserves_stream_bytes_and_enforces_limits() {
        let input = b"DZ facade".repeat(200);
        let settings = RangeSettings::default();
        let expected = crate::compress_chunk(&input, settings).unwrap();
        let mut encoder = Encoder::new(EncoderOptions::default()).unwrap();
        assert_eq!(encoder.encode(&input).unwrap(), expected);

        let mut decoder = Decoder::new(DecoderOptions::new(settings, input.len())).unwrap();
        assert_eq!(decoder.decode(&expected).unwrap(), input);

        let error = Decoder::new(DecoderOptions {
            limits: ResourceLimits {
                max_output_size: 1,
                ..ResourceLimits::UNLIMITED
            },
            ..DecoderOptions::new(settings, input.len())
        })
        .err()
        .unwrap();
        assert_eq!(error.kind(), ErrorKind::OutputLimitExceeded);

        let mut encoder = Encoder::new(EncoderOptions {
            limits: ResourceLimits {
                max_workspace_size: 0,
                ..ResourceLimits::UNLIMITED
            },
            ..EncoderOptions::default()
        })
        .unwrap();
        assert_eq!(
            encoder.encode(&input).unwrap_err().kind(),
            ErrorKind::WorkspaceLimitExceeded
        );
    }
}
