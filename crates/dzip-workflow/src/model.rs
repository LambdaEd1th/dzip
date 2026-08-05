use dzip::{ChunkEncoding, Compression, DzOptions, RangeSettings};
#[cfg(feature = "protocol")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "protocol", serde(transparent))]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct NamedBytes {
    pub name: String,
    #[cfg_attr(feature = "protocol", serde(with = "serde_bytes"))]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct SegmentSummary {
    pub decoded_start: u64,
    pub decoded_end: u64,
    pub encoding: ChunkEncoding,
    pub raw_flags: u16,
    pub volume: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct EntrySummary {
    pub id: usize,
    pub path: String,
    pub name: String,
    pub folder: String,
    pub size: u64,
    /// Exact physical size once every referenced volume has been resolved.
    pub packed_size: Option<u64>,
    pub compression: Compression,
    pub volume: u16,
    pub chunks: usize,
    pub segments: Vec<SegmentSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct ArchiveSummary {
    pub name: String,
    pub entries: Vec<EntrySummary>,
    pub dz_options: DzConfig,
    pub source_size: u64,
    /// Whether `source_size` includes every declared auxiliary volume.
    pub source_complete: bool,
    pub unpacked_size: u64,
    pub chunk_count: usize,
    pub volume_count: usize,
    /// Main volume plus the auxiliary volumes currently available to the session.
    pub loaded_volume_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct ArchiveHandle {
    pub session_id: SessionId,
    pub summary: ArchiveSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct ExtractedFile {
    pub path: String,
    #[cfg_attr(feature = "protocol", serde(with = "serde_bytes"))]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct EditableSegment {
    pub length: usize,
    pub encoding: ChunkEncoding,
    pub raw_flags: u16,
    pub volume: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct EditableEntry {
    pub id: usize,
    pub path: String,
    #[cfg_attr(feature = "protocol", serde(with = "serde_bytes"))]
    pub bytes: Vec<u8>,
    pub segments: Vec<EditableSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct BuildEntry {
    pub path: String,
    #[cfg_attr(feature = "protocol", serde(with = "serde_bytes"))]
    pub bytes: Vec<u8>,
    pub encoding: ChunkEncoding,
    /// Exact compatibility flags when the frontend must preserve combined DCL
    /// coder bits. Ordinary callers should leave this as `None`.
    #[cfg_attr(feature = "protocol", serde(default))]
    pub raw_flags: Option<u16>,
    pub volume: u16,
}

impl BuildEntry {
    pub fn new(path: impl Into<String>, bytes: Vec<u8>, compression: Compression) -> Self {
        Self {
            path: path.into(),
            bytes,
            encoding: ChunkEncoding {
                compression,
                random_access: false,
                common_buffer: false,
                content_hint: None,
                unknown_flags: 0,
            },
            raw_flags: None,
            volume: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct BuildPlan {
    pub archive_name: String,
    /// Exact volume names. An empty list asks the workflow to derive names
    /// from `archive_name` and the highest entry volume.
    pub volume_names: Vec<String>,
    pub alignment: u32,
    pub dz_options: DzConfig,
    pub entries: Vec<BuildEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct BuiltArchive {
    pub volumes: Vec<NamedBytes>,
    pub archive: ArchiveHandle,
}

/// Stable, frontend-safe representation of native DZ settings.
///
/// `max_common_match == 0` represents the encoder's unlimited value and keeps
/// the protocol portable to JavaScript, where `usize::MAX` is not exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "protocol", derive(Serialize, Deserialize))]
pub struct DzConfig {
    pub max_mem_usage: i32,
    pub use_combuf: bool,
    pub preprocess: bool,
    pub trim_reference_factor: i32,
    pub max_common_match: u32,
    pub combuf_static_tables: bool,
    pub win_size: u8,
    pub offset_table_size: u8,
    pub offset_tables: u8,
    pub offset_contexts: u8,
    pub ref_length_table_size: u8,
    pub ref_length_tables: u8,
    pub ref_offset_table_size: u8,
    pub ref_offset_tables: u8,
    pub big_min_match: u8,
}

impl Default for DzConfig {
    fn default() -> Self {
        Self::from_options(&DzOptions::default())
    }
}

impl DzConfig {
    pub fn from_options(options: &DzOptions) -> Self {
        let settings = options.settings;
        Self {
            max_mem_usage: options.max_mem_usage,
            use_combuf: options.use_combuf,
            preprocess: options.preprocess,
            trim_reference_factor: options.trim_reference_factor,
            max_common_match: u32::try_from(options.max_common_match).unwrap_or(0),
            combuf_static_tables: settings.flags & RangeSettings::USE_COMBUF_STATIC_TABLES != 0,
            win_size: settings.win_size,
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

    pub fn to_options(self) -> dzip::Result<DzOptions> {
        let settings = RangeSettings {
            win_size: self.win_size,
            flags: u8::from(self.combuf_static_tables),
            offset_table_size: self.offset_table_size,
            offset_tables: self.offset_tables,
            offset_contexts: self.offset_contexts,
            ref_length_table_size: self.ref_length_table_size,
            ref_length_tables: self.ref_length_tables,
            ref_offset_table_size: self.ref_offset_table_size,
            ref_offset_tables: self.ref_offset_tables,
            big_min_match: self.big_min_match,
        }
        .validate()?;
        Ok(DzOptions {
            settings,
            max_mem_usage: self.max_mem_usage,
            use_combuf: self.use_combuf,
            preprocess: self.preprocess,
            trim_reference_factor: self.trim_reference_factor,
            max_common_match: if self.max_common_match == 0 {
                usize::MAX
            } else {
                self.max_common_match as usize
            },
        })
    }

    /// Repair persisted UI values using the same limits enforced by the core
    /// encoder. This keeps preference migration out of individual frontends.
    pub fn sanitized(mut self) -> Self {
        let defaults = Self::default();
        if self.win_size > 30 {
            self.win_size = defaults.win_size;
        }
        if !(1..=15).contains(&self.offset_table_size) {
            self.offset_table_size = defaults.offset_table_size;
        }
        if self.offset_tables == 0 {
            self.offset_tables = defaults.offset_tables;
        }
        if !(1..=8).contains(&self.offset_contexts) {
            self.offset_contexts = defaults.offset_contexts;
        }
        let minimum = u8::from(self.use_combuf);
        if !(minimum..=15).contains(&self.ref_length_table_size) {
            self.ref_length_table_size = defaults.ref_length_table_size;
        }
        if self.ref_length_tables < minimum {
            self.ref_length_tables = defaults.ref_length_tables;
        }
        if !(minimum..=15).contains(&self.ref_offset_table_size) {
            self.ref_offset_table_size = defaults.ref_offset_table_size;
        }
        if self.ref_offset_tables < minimum {
            self.ref_offset_tables = defaults.ref_offset_tables;
        }
        if self.big_min_match < minimum {
            self.big_min_match = defaults.big_min_match;
        }
        if self.use_combuf {
            self.combuf_static_tables = true;
        }
        self
    }
}
