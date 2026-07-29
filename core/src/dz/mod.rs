mod range;

use crate::format::RangeSettings;
use crate::{DzipError, Result};
use range::{AdaptiveModel, RangeDecoder, RangeEncoder};
use std::collections::HashMap;

const END_SYMBOL: usize = 513;
const MIN_MATCH: usize = 2;
const MAX_MATCH: usize = 258;

struct DzModels {
    top: AdaptiveModel,
    offsets: Vec<Vec<AdaptiveModel>>,
    reference_lengths: Vec<AdaptiveModel>,
    reference_offsets: Vec<AdaptiveModel>,
    reference_sign: AdaptiveModel,
    reference_base: AdaptiveModel,
    reference_bases: [i64; 8],
}

impl DzModels {
    fn new(settings: RangeSettings, has_combuf: bool) -> Result<Self> {
        let top_symbols = if has_combuf {
            514usize + (1usize << settings.ref_length_table_size)
        } else {
            514
        };
        let mut offsets = Vec::with_capacity(usize::from(settings.offset_contexts));
        for _ in 0..settings.offset_contexts {
            let mut context = Vec::with_capacity(usize::from(settings.offset_tables));
            for table in 0..settings.offset_tables {
                context.push(AdaptiveModel::new_with_increment(
                    1usize << settings.offset_table_size,
                    32 + 4 * u16::from(table),
                )?);
            }
            offsets.push(context);
        }
        let mut reference_lengths = Vec::with_capacity(usize::from(settings.ref_length_tables));
        for _ in 0..settings.ref_length_tables {
            reference_lengths.push(AdaptiveModel::new_with_increment(
                1usize << settings.ref_length_table_size,
                32,
            )?);
        }
        let mut reference_offsets = Vec::with_capacity(usize::from(settings.ref_offset_tables));
        for _ in 0..settings.ref_offset_tables {
            reference_offsets.push(AdaptiveModel::new_with_increment(
                1usize << settings.ref_offset_table_size,
                32,
            )?);
        }
        Ok(Self {
            top: AdaptiveModel::new(top_symbols)?,
            offsets,
            reference_lengths,
            reference_offsets,
            reference_sign: AdaptiveModel::new(2)?,
            reference_base: AdaptiveModel::new(8)?,
            reference_bases: [0; 8],
        })
    }
}

#[derive(Clone)]
struct CommonModels {
    top: AdaptiveModel,
    offsets: Vec<Vec<AdaptiveModel>>,
}

impl CommonModels {
    fn uniform(settings: RangeSettings) -> Result<Self> {
        let mut offsets = Vec::with_capacity(usize::from(settings.offset_contexts));
        for _ in 0..settings.offset_contexts {
            let mut context = Vec::with_capacity(usize::from(settings.offset_tables));
            for table in 0..settings.offset_tables {
                context.push(AdaptiveModel::new_with_increment(
                    1usize << settings.offset_table_size,
                    32 + 4 * u16::from(table),
                )?);
            }
            offsets.push(context);
        }
        Ok(Self {
            top: AdaptiveModel::new(514)?,
            offsets,
        })
    }

    fn from_static_prefix(prefix: &[u8], settings: RangeSettings) -> Result<Self> {
        let offset_symbols = 1usize << settings.offset_table_size;
        let expected = settings.combuf_static_prefix_size();
        if prefix.len() < expected {
            return Err(DzipError::InvalidDz(format!(
                "COMBUF static table is {} bytes, expected {}",
                prefix.len(),
                expected
            )));
        }
        let top = AdaptiveModel::from_normalized(&prefix[..514])?;
        let mut cursor = 514usize;
        let mut offsets = Vec::with_capacity(usize::from(settings.offset_contexts));
        for _ in 0..settings.offset_contexts {
            let mut context = Vec::with_capacity(usize::from(settings.offset_tables));
            for table in 0..settings.offset_tables {
                let end = cursor + offset_symbols;
                context.push(AdaptiveModel::from_normalized_with_increment(
                    &prefix[cursor..end],
                    32 + 4 * u16::from(table),
                )?);
                cursor = end;
            }
            offsets.push(context);
        }
        Ok(Self { top, offsets })
    }
}

#[derive(Clone, Debug)]
pub struct DzCommonBuffer {
    chunks: Vec<Vec<u8>>,
    payload_starts: Vec<usize>,
    prefix_size: usize,
    settings: RangeSettings,
}

impl DzCommonBuffer {
    pub fn new(settings: RangeSettings, chunks: Vec<Vec<u8>>) -> Result<Self> {
        let settings = settings.validate()?;
        if chunks.is_empty() {
            return Err(DzipError::InvalidDz(
                "a common buffer must contain at least one chunk".to_string(),
            ));
        }
        let prefix_size = settings.combuf_static_prefix_size();
        let mut payload_starts = Vec::with_capacity(chunks.len() + 1);
        payload_starts.push(0);
        for chunk in &chunks {
            if chunk.len() < prefix_size {
                if chunk.is_empty() {
                    payload_starts.push(*payload_starts.last().unwrap());
                    continue;
                }
                return Err(DzipError::InvalidDz(format!(
                    "COMBUF chunk is {} bytes, smaller than its {} byte static prefix",
                    chunk.len(),
                    prefix_size
                )));
            }
            let next = payload_starts
                .last()
                .copied()
                .unwrap_or(0usize)
                .checked_add(chunk.len() - prefix_size)
                .ok_or_else(|| DzipError::InvalidDz("COMBUF size overflow".to_string()))?;
            payload_starts.push(next);
        }
        Ok(Self {
            chunks,
            payload_starts,
            prefix_size,
            settings,
        })
    }

    pub fn payload_len(&self) -> usize {
        self.payload_starts.last().copied().unwrap_or(0)
    }

    fn decode_at(&self, absolute_offset: usize, length: usize) -> Result<(Vec<u8>, usize)> {
        let chunk_index = self
            .payload_starts
            .windows(2)
            .position(|range| absolute_offset >= range[0] && absolute_offset < range[1])
            .ok_or_else(|| {
                DzipError::InvalidDz(format!(
                    "COMBUF offset {} exceeds {} bytes",
                    absolute_offset,
                    self.payload_len()
                ))
            })?;
        let local_offset = absolute_offset - self.payload_starts[chunk_index];
        let chunk = &self.chunks[chunk_index];
        let payload_start = self.prefix_size + local_offset;
        let payload = chunk.get(payload_start..).ok_or_else(|| {
            DzipError::InvalidDz("COMBUF reference starts outside its chunk".to_string())
        })?;
        let initial_models = if self.prefix_size == 0 {
            CommonModels::uniform(self.settings)?
        } else {
            CommonModels::from_static_prefix(&chunk[..self.prefix_size], self.settings)?
        };
        decode_common_payload(payload, length, self.settings, initial_models)
    }
}

pub fn decompress_chunk(
    input: &[u8],
    expected_size: usize,
    settings: RangeSettings,
) -> Result<Vec<u8>> {
    decompress_chunk_with_common_buffer(input, expected_size, settings, None)
}

pub fn decompress_chunk_with_common_buffer(
    input: &[u8],
    expected_size: usize,
    settings: RangeSettings,
    common_buffer: Option<&DzCommonBuffer>,
) -> Result<Vec<u8>> {
    let settings = settings.validate()?;
    let mut decoder = RangeDecoder::new(input)?;
    let mut models = DzModels::new(settings, common_buffer.is_some())?;
    let mut recent_offsets = [0usize; 4];
    let mut output = Vec::with_capacity(expected_size);

    loop {
        let symbol = decoder.decode(&mut models.top)?;
        match symbol {
            0..=255 => {
                if output.len() >= expected_size {
                    return Err(DzipError::InvalidDz(
                        "literal follows the expected end of a DZ chunk".to_string(),
                    ));
                }
                output.push(symbol as u8);
            }
            256..=512 => {
                let length = symbol - 254;
                let context = usize::min(length - MIN_MATCH, models.offsets.len() - 1);
                let code = decode_grouped(
                    &mut decoder,
                    &mut models.offsets[context],
                    settings.offset_table_size,
                )?;
                let distance = decode_recent_distance(code, &mut recent_offsets)?;
                copy_match(&mut output, distance, length, expected_size)?;
            }
            END_SYMBOL => {
                if output.len() != expected_size {
                    return Err(DzipError::InvalidDz(format!(
                        "DZ stream ended after {} of {} bytes",
                        output.len(),
                        expected_size
                    )));
                }
                break;
            }
            _ => {
                validate_common_settings(settings, true)?;
                let common_buffer = common_buffer.ok_or_else(|| {
                    DzipError::InvalidDz(
                        "common-buffer reference without a common buffer".to_string(),
                    )
                })?;
                let length = decode_reference_length(
                    symbol,
                    &mut decoder,
                    &mut models.reference_lengths,
                    settings.ref_length_table_size,
                )?;
                if output.len().saturating_add(length) > expected_size {
                    return Err(DzipError::InvalidDz(format!(
                        "COMBUF reference of {} bytes exceeds expected output size {}",
                        length, expected_size
                    )));
                }
                let base_index = decoder.decode(&mut models.reference_base)?;
                let negative = decoder.decode(&mut models.reference_sign)? != 0;
                let magnitude = decode_grouped(
                    &mut decoder,
                    &mut models.reference_offsets,
                    settings.ref_offset_table_size,
                )?;
                let delta = if negative {
                    -1i64
                        - i64::try_from(magnitude).map_err(|_| {
                            DzipError::InvalidDz("COMBUF offset delta overflow".to_string())
                        })?
                } else {
                    i64::try_from(magnitude).map_err(|_| {
                        DzipError::InvalidDz("COMBUF offset delta overflow".to_string())
                    })?
                };
                let target = models.reference_bases[base_index]
                    .checked_add(delta)
                    .and_then(|value| value.checked_sub(3))
                    .ok_or_else(|| DzipError::InvalidDz("COMBUF offset overflow".to_string()))?;
                let target = usize::try_from(target).map_err(|_| {
                    DzipError::InvalidDz(format!("negative COMBUF offset {}", target))
                })?;
                let (common_data, consumed) = common_buffer.decode_at(target, length)?;
                output.extend_from_slice(&common_data);
                models.reference_bases[base_index] = i64::try_from(target.saturating_add(consumed))
                    .map_err(|_| DzipError::InvalidDz("COMBUF base offset overflow".to_string()))?;
            }
        }
    }
    Ok(output)
}

fn decode_reference_length(
    first_symbol: usize,
    decoder: &mut RangeDecoder<'_>,
    models: &mut [AdaptiveModel],
    bits: u8,
) -> Result<usize> {
    let first_group = first_symbol.checked_sub(514).ok_or_else(|| {
        DzipError::InvalidDz(format!("invalid COMBUF reference symbol {}", first_symbol))
    })?;
    let grouped = decode_grouped_from_first(decoder, models, bits, first_group)?;
    grouped
        .checked_add(15)
        .ok_or_else(|| DzipError::InvalidDz("COMBUF reference length overflow".to_string()))
}

fn decode_grouped_from_first(
    decoder: &mut RangeDecoder<'_>,
    models: &mut [AdaptiveModel],
    bits: u8,
    first_group: usize,
) -> Result<usize> {
    let continuation = 1usize << (bits - 1);
    let payload_mask = continuation - 1;
    let mut value = first_group & payload_mask;
    if first_group & continuation == 0 {
        return Ok(value);
    }
    if models.is_empty() {
        return Err(DzipError::InvalidDz(
            "continued grouped integer has no continuation model".to_string(),
        ));
    }
    let mut shift = u32::from(bits - 1);
    let mut table = 0usize;
    loop {
        let group = decoder.decode(&mut models[table])?;
        value |= (group & payload_mask)
            .checked_shl(shift)
            .ok_or_else(|| DzipError::InvalidDz("grouped integer overflow".to_string()))?;
        if group & continuation == 0 {
            return Ok(value);
        }
        shift = shift
            .checked_add(u32::from(bits - 1))
            .ok_or_else(|| DzipError::InvalidDz("grouped integer overflow".to_string()))?;
        table = usize::min(table + 1, models.len() - 1);
        if shift >= usize::BITS {
            return Err(DzipError::InvalidDz(
                "grouped integer is too large".to_string(),
            ));
        }
    }
}

fn decode_common_payload(
    payload: &[u8],
    expected_size: usize,
    settings: RangeSettings,
    initial_models: CommonModels,
) -> Result<(Vec<u8>, usize)> {
    let mut stream_offset = 0usize;
    let mut decoder = RangeDecoder::new(payload)?;
    let mut models = initial_models.clone();
    let mut recent_offsets = [0usize; 4];
    let mut output = Vec::with_capacity(expected_size);

    while output.len() < expected_size {
        let symbol = decoder.decode(&mut models.top)?;
        match symbol {
            0..=255 => output.push(symbol as u8),
            256..=512 => {
                let full_length = symbol - 254;
                let context = usize::min(full_length - MIN_MATCH, models.offsets.len() - 1);
                let code = decode_grouped(
                    &mut decoder,
                    &mut models.offsets[context],
                    settings.offset_table_size,
                )?;
                let distance = decode_recent_distance(code, &mut recent_offsets)?;
                let length = usize::min(full_length, expected_size - output.len());
                copy_match(&mut output, distance, length, expected_size)?;
            }
            END_SYMBOL => {
                stream_offset = stream_offset
                    .checked_add(decoder.consumed())
                    .ok_or_else(|| DzipError::InvalidDz("COMBUF offset overflow".to_string()))?;
                decoder = RangeDecoder::new(payload.get(stream_offset..).ok_or_else(|| {
                    DzipError::InvalidDz("COMBUF segment ends outside its chunk".to_string())
                })?)?;
                models = initial_models.clone();
                recent_offsets = [0; 4];
            }
            _ => {
                return Err(DzipError::InvalidDz(format!(
                    "invalid COMBUF token {}",
                    symbol
                )));
            }
        }
    }
    let consumed = stream_offset
        .checked_add(decoder.consumed())
        .ok_or_else(|| DzipError::InvalidDz("COMBUF offset overflow".to_string()))?;
    Ok((output, consumed))
}

pub fn compress_chunk(input: &[u8], settings: RangeSettings) -> Result<Vec<u8>> {
    compress_chunk_with_references(input, settings, false, &[])
}

#[derive(Clone, Debug)]
struct CommonSegment {
    raw: Vec<u8>,
    encoded: Vec<u8>,
    target: usize,
    consumed: usize,
}

#[derive(Clone, Copy, Debug)]
struct CommonReference {
    position: usize,
    length: usize,
    segment: usize,
}

#[derive(Clone, Debug)]
pub struct DzEncoderOptions {
    pub settings: RangeSettings,
    pub use_combuf: bool,
    pub preprocess: bool,
    pub trim_reference_factor: i32,
    pub max_common_match: usize,
}

impl Default for DzEncoderOptions {
    fn default() -> Self {
        Self {
            settings: RangeSettings::default(),
            use_combuf: false,
            preprocess: true,
            trim_reference_factor: 20,
            max_common_match: 4096,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EncodedDzArchive {
    pub chunks: Vec<Vec<u8>>,
    pub common_buffer: Option<Vec<u8>>,
}

pub fn compress_archive(
    inputs: &[Vec<u8>],
    options: &DzEncoderOptions,
) -> Result<EncodedDzArchive> {
    let settings = options.settings.validate()?;
    if !options.use_combuf || inputs.len() < 2 {
        let chunks = inputs
            .iter()
            .map(|input| compress_chunk(input, settings))
            .collect::<Result<Vec<_>>>()?;
        return Ok(EncodedDzArchive {
            chunks,
            common_buffer: None,
        });
    }

    let (mut segments, references) = find_common_references(
        inputs,
        usize::from(settings.big_min_match.max(15)),
        options.max_common_match.max(15),
        options.trim_reference_factor,
    );

    if segments.is_empty() {
        let chunks = inputs
            .iter()
            .map(|input| compress_chunk_with_references(input, settings, true, &[]))
            .collect::<Result<Vec<_>>>()?;
        return Ok(EncodedDzArchive {
            chunks,
            // dzip 1.1.3 emits a zero-length placeholder when COMBUF is enabled
            // but duplicate selection produced no references.
            common_buffer: Some(Vec::new()),
        });
    }
    validate_common_settings(settings, true)?;

    let prefix_size = settings.combuf_static_prefix_size();
    let mut common_bytes = if prefix_size == 0 {
        Vec::new()
    } else {
        vec![1u8; prefix_size]
    };
    let mut payload_offset = 0usize;
    for segment in &mut segments {
        segment.target = payload_offset;
        segment.encoded = compress_chunk(&segment.raw, settings)?;
        payload_offset = payload_offset
            .checked_add(segment.encoded.len())
            .ok_or_else(|| DzipError::InvalidDz("COMBUF size overflow".to_string()))?;
        common_bytes.extend_from_slice(&segment.encoded);
    }

    let common_decoder = DzCommonBuffer::new(settings, vec![common_bytes.clone()])?;
    for segment in &mut segments {
        let (decoded, consumed) = common_decoder.decode_at(segment.target, segment.raw.len())?;
        if decoded != segment.raw {
            return Err(DzipError::InvalidDz(
                "internal COMBUF validation failed".to_string(),
            ));
        }
        segment.consumed = consumed;
    }

    let chunks = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let resolved: Vec<_> = references[index]
                .iter()
                .map(|reference| ResolvedReference {
                    position: reference.position,
                    length: reference.length,
                    target: segments[reference.segment].target,
                    consumed: segments[reference.segment].consumed,
                })
                .collect();
            compress_chunk_with_references(input, settings, true, &resolved)
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(EncodedDzArchive {
        chunks,
        common_buffer: Some(common_bytes),
    })
}

fn validate_common_settings(settings: RangeSettings, use_combuf: bool) -> Result<()> {
    if !use_combuf {
        return Ok(());
    }
    if settings.flags & RangeSettings::USE_COMBUF_STATIC_TABLES == 0 {
        return Err(DzipError::InvalidDz(
            "dzip 1.1.3 requires COMBUF static tables when common references are enabled"
                .to_string(),
        ));
    }
    if settings.ref_length_table_size == 0
        || settings.ref_offset_table_size == 0
        || settings.ref_length_tables == 0
        || settings.ref_offset_tables == 0
    {
        return Err(DzipError::InvalidDz(
            "COMBUF references require length and offset continuation models".to_string(),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct ResolvedReference {
    position: usize,
    length: usize,
    target: usize,
    consumed: usize,
}

fn compress_chunk_with_references(
    input: &[u8],
    settings: RangeSettings,
    has_combuf: bool,
    references: &[ResolvedReference],
) -> Result<Vec<u8>> {
    let settings = settings.validate()?;
    let mut encoder = RangeEncoder::new();
    let mut models = DzModels::new(settings, has_combuf)?;
    let mut recent_offsets = [0usize; 4];
    let window = 1usize
        .checked_shl(u32::from(settings.win_size))
        .unwrap_or(usize::MAX);
    let mut matcher = LzMatcher::new(window);
    let mut position = 0usize;
    let mut reference_index = 0usize;

    while position < input.len() {
        if let Some(reference) = references.get(reference_index)
            && reference.position == position
        {
            encode_common_reference(&mut encoder, &mut models, settings, *reference)?;
            for inserted in position..position + reference.length {
                matcher.insert(input, inserted);
            }
            position += reference.length;
            reference_index += 1;
            continue;
        }

        let next_reference = references
            .get(reference_index)
            .map(|reference| reference.position)
            .unwrap_or(input.len());
        let maximum = usize::min(MAX_MATCH, next_reference.saturating_sub(position));
        let (length, distance) = matcher.find(input, position, maximum);
        if length >= MIN_MATCH {
            encoder.encode(&mut models.top, length.saturating_add(254))?;
            let code = encode_recent_distance(distance, &mut recent_offsets);
            let context = usize::min(length - MIN_MATCH, models.offsets.len() - 1);
            encode_grouped(
                &mut encoder,
                &mut models.offsets[context],
                settings.offset_table_size,
                code,
            )?;
            for inserted in position..position + length {
                matcher.insert(input, inserted);
            }
            position += length;
        } else {
            encoder.encode(&mut models.top, usize::from(input[position]))?;
            matcher.insert(input, position);
            position += 1;
        }
    }
    encoder.encode(&mut models.top, END_SYMBOL)?;
    Ok(encoder.finish())
}

fn encode_common_reference(
    encoder: &mut RangeEncoder,
    models: &mut DzModels,
    settings: RangeSettings,
    reference: ResolvedReference,
) -> Result<()> {
    let value = reference.length.checked_sub(15).ok_or_else(|| {
        DzipError::InvalidDz(format!(
            "COMBUF reference length {} is shorter than 15",
            reference.length
        ))
    })?;
    encode_grouped_with_first_in_top(
        encoder,
        &mut models.top,
        &mut models.reference_lengths,
        settings.ref_length_table_size,
        value,
    )?;

    let target = i64::try_from(reference.target)
        .map_err(|_| DzipError::InvalidDz("COMBUF target overflow".to_string()))?;
    let (base_index, delta) = models
        .reference_bases
        .iter()
        .enumerate()
        .filter_map(|(index, &base)| {
            target
                .checked_sub(base)
                .and_then(|value| value.checked_add(3))
                .map(|delta| (index, delta))
        })
        .min_by_key(|(_, delta)| delta.unsigned_abs())
        .ok_or_else(|| DzipError::InvalidDz("COMBUF target overflow".to_string()))?;
    encoder.encode(&mut models.reference_base, base_index)?;
    let (negative, magnitude) = if delta < 0 {
        (
            1usize,
            usize::try_from(-1i64 - delta)
                .map_err(|_| DzipError::InvalidDz("COMBUF delta overflow".to_string()))?,
        )
    } else {
        (
            0usize,
            usize::try_from(delta)
                .map_err(|_| DzipError::InvalidDz("COMBUF delta overflow".to_string()))?,
        )
    };
    encoder.encode(&mut models.reference_sign, negative)?;
    encode_grouped(
        encoder,
        &mut models.reference_offsets,
        settings.ref_offset_table_size,
        magnitude,
    )?;
    models.reference_bases[base_index] =
        i64::try_from(reference.target.saturating_add(reference.consumed))
            .map_err(|_| DzipError::InvalidDz("COMBUF base overflow".to_string()))?;
    Ok(())
}

fn encode_grouped_with_first_in_top(
    encoder: &mut RangeEncoder,
    top: &mut AdaptiveModel,
    continuation_models: &mut [AdaptiveModel],
    bits: u8,
    mut value: usize,
) -> Result<()> {
    let continuation = 1usize << (bits - 1);
    let payload_mask = continuation - 1;
    let payload = value & payload_mask;
    value >>= bits - 1;
    let first_group = payload | if value != 0 { continuation } else { 0 };
    encoder.encode(top, 514 + first_group)?;
    if value == 0 {
        return Ok(());
    }
    if continuation_models.is_empty() {
        return Err(DzipError::InvalidDz(
            "continued COMBUF length has no model".to_string(),
        ));
    }
    let mut table = 0usize;
    loop {
        let payload = value & payload_mask;
        value >>= bits - 1;
        let group = payload | if value != 0 { continuation } else { 0 };
        encoder.encode(&mut continuation_models[table], group)?;
        if value == 0 {
            return Ok(());
        }
        table = usize::min(table + 1, continuation_models.len() - 1);
    }
}

fn decode_grouped(
    decoder: &mut RangeDecoder<'_>,
    models: &mut [AdaptiveModel],
    bits: u8,
) -> Result<usize> {
    let continuation = 1usize << (bits - 1);
    let payload_mask = continuation - 1;
    let mut value = 0usize;
    let mut shift = 0u32;
    let mut table = 0usize;
    loop {
        let group = decoder.decode(&mut models[table])?;
        value |= (group & payload_mask)
            .checked_shl(shift)
            .ok_or_else(|| DzipError::InvalidDz("grouped integer overflow".to_string()))?;
        if group & continuation == 0 {
            return Ok(value);
        }
        shift = shift
            .checked_add(u32::from(bits - 1))
            .ok_or_else(|| DzipError::InvalidDz("grouped integer overflow".to_string()))?;
        table = usize::min(table + 1, models.len() - 1);
        if shift >= usize::BITS {
            return Err(DzipError::InvalidDz(
                "grouped integer is too large".to_string(),
            ));
        }
    }
}

fn encode_grouped(
    encoder: &mut RangeEncoder,
    models: &mut [AdaptiveModel],
    bits: u8,
    mut value: usize,
) -> Result<()> {
    let continuation = 1usize << (bits - 1);
    let payload_mask = continuation - 1;
    let mut table = 0usize;
    loop {
        let payload = value & payload_mask;
        value >>= bits - 1;
        let group = payload | if value != 0 { continuation } else { 0 };
        encoder.encode(&mut models[table], group)?;
        if value == 0 {
            return Ok(());
        }
        table = usize::min(table + 1, models.len() - 1);
    }
}

fn decode_recent_distance(code: usize, recent: &mut [usize; 4]) -> Result<usize> {
    if code < recent.len() {
        let distance = recent[code];
        recent.swap(0, code);
        if distance == 0 {
            return Err(DzipError::InvalidDz(
                "DZ stream used an uninitialized recent distance".to_string(),
            ));
        }
        Ok(distance)
    } else {
        let distance = code - 3;
        recent.copy_within(0..3, 1);
        recent[0] = distance;
        Ok(distance)
    }
}

fn encode_recent_distance(distance: usize, recent: &mut [usize; 4]) -> usize {
    if let Some(index) = recent.iter().position(|&candidate| candidate == distance) {
        recent.swap(0, index);
        index
    } else {
        recent.copy_within(0..3, 1);
        recent[0] = distance;
        distance + 3
    }
}

fn copy_match(
    output: &mut Vec<u8>,
    distance: usize,
    length: usize,
    expected_size: usize,
) -> Result<()> {
    if distance == 0 || distance > output.len() {
        return Err(DzipError::InvalidDz(format!(
            "invalid LZ distance {} at output position {}",
            distance,
            output.len()
        )));
    }
    if output.len().saturating_add(length) > expected_size {
        return Err(DzipError::InvalidDz(format!(
            "LZ match exceeds expected output size {}",
            expected_size
        )));
    }
    for _ in 0..length {
        let value = output[output.len() - distance];
        output.push(value);
    }
    Ok(())
}

struct LzMatcher {
    window: usize,
    chains: HashMap<u32, Vec<usize>>,
}

impl LzMatcher {
    fn new(window: usize) -> Self {
        Self {
            window,
            chains: HashMap::new(),
        }
    }

    fn find(&self, input: &[u8], position: usize, maximum: usize) -> (usize, usize) {
        if maximum < MIN_MATCH || position + MIN_MATCH > input.len() {
            return (0, 0);
        }
        let Some(key) = local_match_key(input, position) else {
            return (0, 0);
        };
        let Some(candidates) = self.chains.get(&key) else {
            return (0, 0);
        };
        let maximum = usize::min(maximum, input.len() - position);
        let minimum_position = position.saturating_sub(self.window);
        let mut best_length = 0usize;
        let mut best_distance = 0usize;
        for &candidate in candidates.iter().rev().take(128) {
            if candidate < minimum_position || candidate >= position {
                continue;
            }
            let mut length = MIN_MATCH;
            while length < maximum && input[candidate + length] == input[position + length] {
                length += 1;
            }
            if length > best_length {
                best_length = length;
                best_distance = position - candidate;
                if length == maximum {
                    break;
                }
            }
        }
        (best_length, best_distance)
    }

    fn insert(&mut self, input: &[u8], position: usize) {
        let Some(key) = local_match_key(input, position) else {
            return;
        };
        let chain = self.chains.entry(key).or_default();
        chain.push(position);
        if chain.len() > 256 {
            chain.remove(0);
        }
    }
}

fn local_match_key(input: &[u8], position: usize) -> Option<u32> {
    let bytes = input.get(position..position + MIN_MATCH)?;
    Some(u32::from(bytes[0]) | (u32::from(bytes[1]) << 8))
}

fn common_match_key(input: &[u8], position: usize) -> Option<u64> {
    let bytes: [u8; 8] = input.get(position..position + 8)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn find_common_references(
    inputs: &[Vec<u8>],
    minimum_match: usize,
    maximum_match: usize,
    trim_reference_factor: i32,
) -> (Vec<CommonSegment>, Vec<Vec<CommonReference>>) {
    let minimum_match = minimum_match.saturating_add(trim_reference_factor.max(0) as usize / 20);
    let mut prior: HashMap<u64, Vec<(usize, usize)>> = HashMap::new();
    let mut segment_index: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut segments: Vec<CommonSegment> = Vec::new();
    let mut references = vec![Vec::new(); inputs.len()];

    for (chunk_index, input) in inputs.iter().enumerate() {
        let mut position = 0usize;
        while position + minimum_match <= input.len() {
            let Some(key) = common_match_key(input, position) else {
                break;
            };

            let mut selected_segment = segment_index.get(&key).and_then(|indices| {
                indices
                    .iter()
                    .copied()
                    .find(|&index| input[position..].starts_with(segments[index].raw.as_slice()))
            });

            if selected_segment.is_none() {
                let mut best = None;
                if let Some(candidates) = prior.get(&key) {
                    for &(source_chunk, source_position) in candidates.iter().rev().take(32) {
                        let source = &inputs[source_chunk];
                        let maximum = usize::min(
                            maximum_match,
                            usize::min(input.len() - position, source.len() - source_position),
                        );
                        let mut length = 8usize;
                        while length < maximum
                            && input[position + length] == source[source_position + length]
                        {
                            length += 1;
                        }
                        if length >= minimum_match
                            && best.is_none_or(|(_, _, best_length)| length > best_length)
                        {
                            best = Some((source_chunk, source_position, length));
                        }
                    }
                }
                if let Some((source_chunk, source_position, length)) = best {
                    let raw =
                        inputs[source_chunk][source_position..source_position + length].to_vec();
                    let index = segments.len();
                    segments.push(CommonSegment {
                        raw,
                        encoded: Vec::new(),
                        target: 0,
                        consumed: 0,
                    });
                    segment_index.entry(key).or_default().push(index);
                    selected_segment = Some(index);
                }
            }

            if let Some(segment) = selected_segment {
                let length = segments[segment].raw.len();
                references[chunk_index].push(CommonReference {
                    position,
                    length,
                    segment,
                });
                position += length;
            } else {
                position += 1;
            }
        }

        for position in 0..input.len().saturating_sub(7) {
            let key = common_match_key(input, position).expect("length checked");
            let occurrences = prior.entry(key).or_default();
            occurrences.push((chunk_index, position));
            if occurrences.len() > 32 {
                occurrences.remove(0);
            }
        }
    }

    (segments, references)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dz_round_trip_literals_and_matches() {
        let data = b"the quick brown fox jumps over the quick brown fox\n".repeat(32);
        let settings = RangeSettings::default();
        let compressed = compress_chunk(&data, settings).unwrap();
        let decompressed = decompress_chunk(&compressed, data.len(), settings).unwrap();
        assert_eq!(decompressed, data);
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn dz_round_trip_small_inputs() {
        let settings = RangeSettings::default();
        for data in [
            Vec::new(),
            vec![0],
            vec![0, 0],
            vec![0, 1, 2],
            vec![255; 32],
        ] {
            let compressed = compress_chunk(&data, settings).unwrap();
            let decompressed = decompress_chunk(&compressed, data.len(), settings).unwrap();
            assert_eq!(decompressed, data);
        }
    }

    #[test]
    fn dz_without_common_buffer_allows_zero_reference_settings() {
        let settings = RangeSettings {
            ref_length_table_size: 0,
            ref_length_tables: 0,
            ref_offset_table_size: 0,
            ref_offset_tables: 0,
            ..RangeSettings::default()
        };
        let data = b"local matches still work without external-reference models".repeat(8);
        let compressed = compress_chunk(&data, settings).unwrap();
        let decompressed = decompress_chunk(&compressed, data.len(), settings).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn dz_archive_round_trip_with_common_buffer() {
        let shared = b"a cross-file sequence that is deliberately longer than fifteen bytes";
        let inputs = vec![
            [b"first:".as_slice(), shared, b":end".as_slice()].concat(),
            [b"second:".as_slice(), shared, b":done".as_slice()].concat(),
            [b"third:".as_slice(), shared, b":fin".as_slice()].concat(),
        ];
        let options = DzEncoderOptions {
            use_combuf: true,
            ..DzEncoderOptions::default()
        };
        let encoded = compress_archive(&inputs, &options).unwrap();
        let common_bytes = encoded.common_buffer.as_ref().unwrap();
        assert!(!common_bytes.is_empty());
        let common = DzCommonBuffer::new(options.settings, vec![common_bytes.clone()]).unwrap();

        for (input, compressed) in inputs.iter().zip(&encoded.chunks) {
            let decoded = decompress_chunk_with_common_buffer(
                compressed,
                input.len(),
                options.settings,
                Some(&common),
            )
            .unwrap();
            assert_eq!(&decoded, input);
        }
    }
}
