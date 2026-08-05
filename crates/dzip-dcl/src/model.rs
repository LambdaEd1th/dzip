use dzip::{ChunkEncoding, Compression};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug)]
pub enum ConfigError {
    Io {
        context: Option<String>,
        source: io::Error,
    },
    Invalid(String),
}

impl ConfigError {
    pub(super) fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: Some(context.into()),
            source,
        }
    }

    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                context: Some(context),
                source,
            } => write!(formatter, "{context}: {source}"),
            Self::Io {
                context: None,
                source,
            } => source.fmt(formatter),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Invalid(_) => None,
        }
    }
}

impl From<io::Error> for ConfigError {
    fn from(source: io::Error) -> Self {
        Self::Io {
            context: None,
            source,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DclConfig {
    pub archives: Vec<String>,
    pub align: u32,
    pub files: Vec<FileEntry>,
    pub options: GlobalOptions,
    pub dcl_search_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub archive_file_index: i32,
    pub(crate) range: DclRange,
    pub(crate) flags: u16,
}

impl FileEntry {
    pub fn byte_range(&self, total_len: usize) -> Result<(usize, usize)> {
        self.range.resolve(total_len, &self.path)
    }

    pub const fn dcl_flags(&self) -> u16 {
        self.flags
    }

    pub fn selected_compression(&self) -> Option<Compression> {
        ChunkEncoding::from_packer_flags(self.flags)
            .ok()
            .map(|encoding| encoding.compression)
    }

    pub const fn requested_archive_file_index(&self) -> i32 {
        self.archive_file_index
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DclRange {
    pub(super) from: i32,
    pub(super) to: i32,
}

impl DclRange {
    fn resolve(self, total_len: usize, path: &Path) -> Result<(usize, usize)> {
        let resolve_boundary = |raw: i32| -> Result<usize> {
            if raw < 0 {
                let percent = u64::from(raw.unsigned_abs());
                let value = (total_len as u128)
                    .checked_mul(u128::from(percent))
                    .ok_or_else(|| ConfigError::invalid("DCL percentage range overflow"))?
                    / 100;
                usize::try_from(value).map_err(|_| {
                    ConfigError::invalid("DCL percentage range exceeds platform limits")
                })
            } else {
                Ok(raw as usize)
            }
        };

        let start = resolve_boundary(self.from)?;
        let end = resolve_boundary(self.to)?;
        if start > end || end > total_len {
            return Err(ConfigError::invalid(format!(
                "DCL byte range {}..{} is outside {} ({} bytes)",
                start,
                end,
                path.display(),
                total_len
            )));
        }
        Ok((start, end))
    }
}

#[derive(Debug, Clone)]
pub struct GlobalOptions {
    pub method: String,
    pub is_not_default: bool,
    pub max_mem_usage: i32,
    pub use_combuf: bool,
    pub preprocess: bool,
    pub trim_reference_factor: i32,
    pub win_size: u8,
    pub flags: u8,
    pub offset_table_size: u8,
    pub offset_tables: u8,
    pub offset_contexts: u8,
    pub ref_length_table_size: u8,
    pub ref_length_tables: u8,
    pub ref_offset_table_size: u8,
    pub ref_offset_tables: u8,
    pub big_min_match: u8,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        Self {
            method: "dz".to_string(),
            is_not_default: true,
            max_mem_usage: -1,
            use_combuf: false,
            preprocess: true,
            trim_reference_factor: 20,
            win_size: 16,
            flags: 1,
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
