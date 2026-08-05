use dzip::{ChunkEncoding, Compression};
use dzip_workflow::{ArchiveHandle, EditableSegment, EntrySummary, SessionId};
use std::sync::Arc;

pub use dzip::Compression as CompressionChoice;
pub use dzip_workflow::DzConfig as DzCompressionOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePage {
    Browse,
    Editor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryView {
    pub id: usize,
    pub path: String,
    pub name: String,
    pub folder: String,
    pub size: u64,
    pub packed_size: Option<u64>,
    pub compression: Compression,
    pub volume: u16,
    pub chunks: usize,
}

impl From<EntrySummary> for EntryView {
    fn from(entry: EntrySummary) -> Self {
        Self {
            id: entry.id,
            path: entry.path,
            name: entry.name,
            folder: entry.folder,
            size: entry.size,
            packed_size: entry.packed_size,
            compression: entry.compression,
            volume: entry.volume,
            chunks: entry.chunks,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedArchive {
    pub session_id: SessionId,
    pub name: String,
    pub entries: Arc<Vec<EntryView>>,
    pub dz_options: DzCompressionOptions,
    pub source_size: u64,
    pub source_complete: bool,
    pub unpacked_size: u64,
    pub chunk_count: usize,
    pub volume_count: usize,
    pub loaded_volume_count: usize,
}

impl From<ArchiveHandle> for LoadedArchive {
    fn from(handle: ArchiveHandle) -> Self {
        let summary = handle.summary;
        Self {
            session_id: handle.session_id,
            name: summary.name,
            entries: Arc::new(summary.entries.into_iter().map(Into::into).collect()),
            dz_options: summary.dz_options,
            source_size: summary.source_size,
            source_complete: summary.source_complete,
            unpacked_size: summary.unpacked_size,
            chunk_count: summary.chunk_count,
            volume_count: summary.volume_count,
            loaded_volume_count: summary.loaded_volume_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftFile {
    pub id: u64,
    pub path: String,
    pub bytes: Arc<[u8]>,
    pub compression: CompressionChoice,
    pub volume: u16,
    /// Original chunk boundaries and flags. They are retained until the user
    /// changes this file's compression or volume.
    pub preserved_segments: Option<Vec<EditableSegment>>,
}

impl DraftFile {
    pub fn simple(
        id: u64,
        path: impl Into<String>,
        bytes: impl Into<Arc<[u8]>>,
        compression: Compression,
        volume: u16,
    ) -> Self {
        Self {
            id,
            path: path.into(),
            bytes: bytes.into(),
            compression,
            volume,
            preserved_segments: None,
        }
    }

    pub fn build_entries(&self, random_access: bool) -> Vec<dzip_workflow::BuildEntry> {
        if let Some(segments) = &self.preserved_segments {
            let exact_length = segments
                .iter()
                .try_fold(0usize, |total, segment| total.checked_add(segment.length));
            if exact_length != Some(self.bytes.len()) {
                return self.single_build_entry(random_access);
            }
            let mut cursor = 0usize;
            let mut entries = Vec::with_capacity(segments.len());
            for segment in segments {
                let end = cursor + segment.length;
                entries.push(dzip_workflow::BuildEntry {
                    path: self.path.clone(),
                    bytes: self.bytes[cursor..end].to_vec(),
                    encoding: segment.encoding,
                    raw_flags: Some(segment.raw_flags),
                    volume: segment.volume,
                });
                cursor = end;
            }
            return entries;
        }
        self.single_build_entry(random_access)
    }

    fn single_build_entry(&self, random_access: bool) -> Vec<dzip_workflow::BuildEntry> {
        vec![dzip_workflow::BuildEntry {
            path: self.path.clone(),
            bytes: self.bytes.as_ref().to_vec(),
            encoding: ChunkEncoding {
                compression: self.compression,
                random_access,
                common_buffer: false,
                content_hint: None,
                unknown_flags: 0,
            },
            raw_flags: None,
            volume: self.volume,
        }]
    }

    pub fn replace_compression(&mut self, compression: Compression) {
        self.compression = compression;
        self.preserved_segments = None;
    }

    pub fn replace_volume(&mut self, volume: u16) {
        self.volume = volume;
        self.preserved_segments = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserved_segments_are_used_only_when_their_lengths_are_exact() {
        let mut file = DraftFile::simple(1, "mixed.bin", b"abcdef".as_slice(), Compression::Dz, 0);
        file.preserved_segments = Some(vec![
            EditableSegment {
                length: 2,
                encoding: ChunkEncoding {
                    compression: Compression::Copy,
                    random_access: true,
                    common_buffer: false,
                    content_hint: None,
                    unknown_flags: 0,
                },
                raw_flags: dzip::format::CHUNK_COPYCOMP | dzip::format::CHUNK_RANDOMACCESS,
                volume: 1,
            },
            EditableSegment {
                length: 4,
                encoding: ChunkEncoding {
                    compression: Compression::Lzma,
                    random_access: false,
                    common_buffer: false,
                    content_hint: None,
                    unknown_flags: 0,
                },
                raw_flags: dzip::format::CHUNK_LZMA,
                volume: 2,
            },
        ]);
        let entries = file.build_entries(false);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].bytes, b"ab");
        assert_eq!(entries[1].bytes, b"cdef");

        file.preserved_segments.as_mut().unwrap()[1].length = 5;
        let fallback = file.build_entries(false);
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].bytes, b"abcdef");
        assert_eq!(fallback[0].encoding.compression, Compression::Dz);
    }
}
