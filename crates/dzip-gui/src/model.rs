use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePage {
    Browse,
    Editor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryView {
    pub id: usize,
    pub path: String,
    pub name: String,
    pub folder: String,
    pub size: u64,
    pub packed_size: u64,
    pub compression: String,
    pub volume: u16,
    pub chunks: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedArchive {
    pub name: String,
    pub main_bytes: Arc<[u8]>,
    pub auxiliary: Arc<Vec<(u16, Vec<u8>)>>,
    pub entries: Arc<Vec<EntryView>>,
    pub dz_options: DzCompressionOptions,
    pub source_size: u64,
    pub unpacked_size: u64,
    pub chunk_count: usize,
    pub volume_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftFile {
    pub id: u64,
    pub path: String,
    pub bytes: Arc<[u8]>,
    pub compression: CompressionChoice,
    pub volume: u16,
}

/// Archive-wide settings used by the native DZ encoder.
///
/// `max_common_match == 0` represents the engine's unlimited setting. The
/// static-table flag is exposed separately because the other on-disk flag is
/// rejected by dzip.exe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DzCompressionOptions {
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

impl Default for DzCompressionOptions {
    fn default() -> Self {
        Self {
            max_mem_usage: -1,
            use_combuf: false,
            preprocess: true,
            trim_reference_factor: 20,
            max_common_match: 0,
            combuf_static_tables: true,
            win_size: 16,
            offset_table_size: 8,
            offset_tables: 3,
            offset_contexts: 3,
            ref_length_table_size: 7,
            ref_length_tables: 1,
            ref_offset_table_size: 7,
            ref_offset_tables: 3,
            big_min_match: 15,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionChoice {
    Dz,
    Zlib,
    Bzip,
    Lzma,
    Copy,
    Zero,
}

impl CompressionChoice {
    pub const ALL: [Self; 6] = [
        Self::Dz,
        Self::Zlib,
        Self::Bzip,
        Self::Lzma,
        Self::Copy,
        Self::Zero,
    ];

    pub fn from_archive_label(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "zlib" => Self::Zlib,
            "bzip" => Self::Bzip,
            "lzma" => Self::Lzma,
            "copy" | "仅存储" => Self::Copy,
            "zero" | "零填充" => Self::Zero,
            _ => Self::Dz,
        }
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

pub fn ratio_percent(packed: u64, unpacked: u64) -> u64 {
    packed
        .saturating_mul(100)
        .checked_div(unpacked)
        .unwrap_or(0)
}
