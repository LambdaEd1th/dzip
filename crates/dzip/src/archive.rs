//! High-level archive reading, indexing, and safe extraction.

use crate::codec::{ChunkEncoding, Compression, DzDecodeContext};
use crate::format::{ArchiveSettings, CHUNK_COMBUF, CHUNK_DZ, Chunk, RangeSettings};
use crate::path::{resolve_relative_path, to_archive_format};
use crate::reader::{DzipReader, VolumeSource, correct_chunk_sizes};
use crate::volume::FileSystemVolumeManager;
use crate::{DzipError, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

pub use crate::options::{Compatibility, ReadLimits, ReadOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryId(pub usize);

#[derive(Debug, Clone)]
pub struct Entry {
    id: EntryId,
    path: PathBuf,
    chunk_ids: Vec<u16>,
    decompressed_size: u64,
    compression: Compression,
    volume: u16,
}

impl Entry {
    pub const fn id(&self) -> EntryId {
        self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn chunk_ids(&self) -> &[u16] {
        &self.chunk_ids
    }

    pub const fn decompressed_size(&self) -> u64 {
        self.decompressed_size
    }

    pub const fn compression(&self) -> Compression {
        self.compression
    }

    pub const fn volume(&self) -> u16 {
        self.volume
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveIndex {
    archive_settings: ArchiveSettings,
    entries: Vec<Entry>,
    chunks: Vec<Chunk>,
    volume_files: Vec<String>,
    range_settings: Option<RangeSettings>,
}

impl ArchiveIndex {
    pub const fn archive_settings(&self) -> &ArchiveSettings {
        &self.archive_settings
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    pub fn volume_files(&self) -> &[String] {
        &self.volume_files
    }

    pub const fn range_settings(&self) -> Option<RangeSettings> {
        self.range_settings
    }

    pub fn entry(&self, id: EntryId) -> Option<&Entry> {
        self.entries.get(id.0)
    }

    pub fn find(&self, path: impl AsRef<Path>) -> Option<&Entry> {
        let needle = to_archive_format(path.as_ref());
        self.entries
            .iter()
            .find(|entry| to_archive_format(&entry.path).eq_ignore_ascii_case(&needle))
    }
}

pub struct Archive<R: Read + Seek, V: VolumeSource> {
    reader: DzipReader<R>,
    volumes: V,
    index: ArchiveIndex,
    dz_context: Option<DzDecodeContext>,
}

struct ParsedMetadata {
    settings: ArchiveSettings,
    strings: Vec<String>,
    map: Vec<(u16, Vec<u16>)>,
    chunks: Vec<Chunk>,
    volume_files: Vec<String>,
    range_settings: Option<RangeSettings>,
}

impl<R: Read + Seek, V: VolumeSource> Archive<R, V> {
    pub fn open_with_volumes(reader: R, volumes: V) -> Result<Self> {
        Self::open_with_options(reader, volumes, ReadOptions::default())
    }

    pub fn open_with_options(reader: R, volumes: V, options: ReadOptions) -> Result<Self> {
        let mut reader = DzipReader::new(reader);
        let parsed = parse_metadata(&mut reader, &options)?;
        finish_open(reader, volumes, parsed, &options)
    }

    pub fn index(&self) -> &ArchiveIndex {
        &self.index
    }

    pub fn entries(&self) -> &[Entry] {
        self.index.entries()
    }

    pub fn entry(&self, id: EntryId) -> Option<&Entry> {
        self.index.entry(id)
    }

    pub fn find_entry(&self, path: impl AsRef<Path>) -> Option<&Entry> {
        self.index.find(path)
    }

    pub fn read_entry(&mut self, id: EntryId) -> Result<Vec<u8>> {
        let expected = self
            .entry(id)
            .ok_or_else(|| DzipError::EntryNotFound(id.0.to_string()))?
            .decompressed_size;
        let capacity = usize::try_from(expected).map_err(|_| DzipError::LimitExceeded {
            resource: "entry size",
            limit: usize::MAX as u64,
            actual: expected,
        })?;
        let mut output = Vec::with_capacity(capacity);
        self.read_entry_to(id, &mut output)?;
        Ok(output)
    }

    pub fn read_entry_by_path(&mut self, path: impl AsRef<Path>) -> Result<Vec<u8>> {
        let path = path.as_ref();
        let id = self
            .find_entry(path)
            .map(Entry::id)
            .ok_or_else(|| DzipError::EntryNotFound(path.display().to_string()))?;
        self.read_entry(id)
    }

    pub fn read_entry_to<W: Write>(&mut self, id: EntryId, mut output: W) -> Result<u64> {
        let entry = self
            .entry(id)
            .ok_or_else(|| DzipError::EntryNotFound(id.0.to_string()))?;
        let expected = entry.decompressed_size;
        let codec = entry.compression;
        let chunk_ids = entry.chunk_ids.clone();
        let mut written = 0u64;
        for chunk_id in chunk_ids {
            let chunk = *self
                .index
                .chunks
                .get(chunk_id as usize)
                .ok_or_else(|| invalid_archive(format!("invalid chunk index {chunk_id}")))?;
            let bytes = self.reader.read_chunk_data_with_context(
                &chunk,
                &mut self.volumes,
                self.dz_context.as_ref(),
            )?;
            output.write_all(&bytes)?;
            written = written
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| invalid_archive("entry output size overflow"))?;
        }
        if written != expected {
            return Err(invalid_archive(format!(
                "{codec} entry length mismatch: expected {expected}, got {written}"
            )));
        }
        Ok(written)
    }
}

impl Archive<File, FileSystemVolumeManager> {
    pub fn open_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_path_with_options(path, ReadOptions::default())
    }

    pub fn open_path_with_options(path: impl AsRef<Path>, options: ReadOptions) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let mut reader = DzipReader::new(file);
        let parsed = parse_metadata(&mut reader, &options)?;
        let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let volumes =
            FileSystemVolumeManager::new(base_dir.to_path_buf(), parsed.volume_files.clone());
        finish_open(reader, volumes, parsed, &options)
    }
}

fn parse_metadata<R: Read + Seek>(
    reader: &mut DzipReader<R>,
    options: &ReadOptions,
) -> Result<ParsedMetadata> {
    let settings = reader.read_archive_settings()?;
    check_limit(
        "entry count",
        options.limits.max_entries as u64,
        settings.num_user_files as u64,
    )?;
    let strings_count = usize::from(settings.num_user_files)
        .checked_add(usize::from(settings.num_directories))
        .and_then(|count| count.checked_sub(1))
        .ok_or_else(|| invalid_archive("invalid metadata string count"))?;
    let strings = reader.read_strings_with_limits(
        strings_count,
        options.limits.max_string_length,
        options.limits.max_metadata_bytes,
    )?;
    let map = reader.read_file_chunk_map_with_limits(
        settings.num_user_files as usize,
        options.limits.max_chunks,
        options.limits.max_chunk_references,
    )?;
    let chunk_settings = reader.read_chunk_settings()?;
    check_limit(
        "chunk count",
        options.limits.max_chunks as u64,
        chunk_settings.num_chunks as u64,
    )?;
    if chunk_settings.num_archive_files == 0 {
        return Err(invalid_archive("archive declares zero volumes"));
    }
    let chunks = reader.read_chunks(chunk_settings.num_chunks as usize)?;
    let volume_files =
        reader.read_file_list(chunk_settings.num_archive_files.saturating_sub(1) as usize)?;
    let range_settings = if chunks.iter().any(|chunk| chunk.flags & CHUNK_DZ != 0) {
        Some(reader.read_global_settings()?.validate()?)
    } else {
        None
    };
    Ok(ParsedMetadata {
        settings,
        strings,
        map,
        chunks,
        volume_files,
        range_settings,
    })
}

fn finish_open<R: Read + Seek, V: VolumeSource>(
    mut reader: DzipReader<R>,
    mut volumes: V,
    mut parsed: ParsedMetadata,
    options: &ReadOptions,
) -> Result<Archive<R, V>> {
    let mut sizes = HashMap::new();
    sizes.insert(0, reader.stream_len()?);
    for id in 1..=parsed.volume_files.len() {
        let id = u16::try_from(id).map_err(|_| invalid_archive("volume ID overflow"))?;
        if let Some(length) = volumes.volume_len(id)? {
            sizes.insert(id, length);
        }
    }

    match options.compatibility {
        Compatibility::Dzip => correct_chunk_sizes(&mut parsed.chunks, &sizes),
        Compatibility::Strict => validate_chunk_bounds(&parsed.chunks, &sizes)?,
    }

    let index = build_index(&parsed, options)?;
    let dz_context = if let Some(settings) = parsed.range_settings {
        Some(reader.load_dz_context(&parsed.chunks, settings, &mut volumes)?)
    } else {
        None
    };

    Ok(Archive {
        reader,
        volumes,
        index,
        dz_context,
    })
}

fn build_index(parsed: &ParsedMetadata, options: &ReadOptions) -> Result<ArchiveIndex> {
    let file_count = parsed.settings.num_user_files as usize;
    let mut entries = Vec::with_capacity(parsed.map.len());
    let mut total_output = 0u64;

    for (index, (directory_id, chunk_ids)) in parsed.map.iter().enumerate() {
        let file_name = parsed
            .strings
            .get(index)
            .ok_or_else(|| invalid_archive(format!("missing file name at index {index}")))?;
        let mut archive_path = String::new();
        if *directory_id > 0 {
            let directory_index = file_count
                .checked_add(*directory_id as usize - 1)
                .ok_or_else(|| invalid_archive("directory index overflow"))?;
            let directory = parsed.strings.get(directory_index).ok_or_else(|| {
                invalid_archive(format!(
                    "invalid directory ID {directory_id} for {file_name}"
                ))
            })?;
            archive_path.push_str(directory);
            if !archive_path.ends_with('/') && !archive_path.ends_with('\\') {
                archive_path.push('\\');
            }
        }
        archive_path.push_str(file_name);
        let path = resolve_relative_path(&archive_path)?;

        let mut decompressed_size = 0u64;
        let mut compression = Compression::Copy;
        let mut volume = 0;
        for (part, chunk_id) in chunk_ids.iter().enumerate() {
            let chunk = parsed
                .chunks
                .get(*chunk_id as usize)
                .ok_or_else(|| invalid_archive(format!("invalid chunk index {chunk_id}")))?;
            if chunk.flags & CHUNK_COMBUF != 0 {
                return Err(invalid_archive(format!(
                    "entry {} references a COMBUF chunk",
                    path.display()
                )));
            }
            if chunk.file as usize > parsed.volume_files.len() {
                return Err(DzipError::VolumeNotFound(chunk.file));
            }
            let encoding = ChunkEncoding::from_flags(chunk.flags)?;
            if options.compatibility == Compatibility::Strict && encoding.unknown_flags != 0 {
                return Err(invalid_archive(format!(
                    "chunk {chunk_id} uses unknown flags {:#x}",
                    encoding.unknown_flags
                )));
            }
            if part == 0 {
                compression = encoding.compression;
                volume = chunk.file;
            }
            decompressed_size = decompressed_size
                .checked_add(chunk.decompressed_length as u64)
                .ok_or_else(|| invalid_archive("entry size overflow"))?;
        }
        check_limit(
            "entry size",
            options.limits.max_entry_size,
            decompressed_size,
        )?;
        total_output = total_output
            .checked_add(decompressed_size)
            .ok_or_else(|| invalid_archive("total output size overflow"))?;

        entries.push(Entry {
            id: EntryId(index),
            path,
            chunk_ids: chunk_ids.clone(),
            decompressed_size,
            compression,
            volume,
        });
    }
    check_limit(
        "total output size",
        options.limits.max_total_output,
        total_output,
    )?;

    Ok(ArchiveIndex {
        archive_settings: parsed.settings,
        entries,
        chunks: parsed.chunks.clone(),
        volume_files: parsed.volume_files.clone(),
        range_settings: parsed.range_settings,
    })
}

fn validate_chunk_bounds(chunks: &[Chunk], sizes: &HashMap<u16, u64>) -> Result<()> {
    for (index, chunk) in chunks.iter().enumerate() {
        if chunk.flags & crate::format::CHUNK_ZERO != 0 {
            continue;
        }
        let volume_size = sizes
            .get(&chunk.file)
            .ok_or(DzipError::VolumeNotFound(chunk.file))?;
        let end = u64::from(chunk.offset)
            .checked_add(u64::from(chunk.compressed_length))
            .ok_or_else(|| invalid_archive(format!("chunk {index} range overflow")))?;
        if end > *volume_size {
            return Err(invalid_archive(format!(
                "chunk {index} ends at {end}, beyond volume {} size {volume_size}",
                chunk.file
            )));
        }
    }
    Ok(())
}

fn check_limit(resource: &'static str, limit: u64, actual: u64) -> Result<()> {
    if actual > limit {
        return Err(DzipError::LimitExceeded {
            resource,
            limit,
            actual,
        });
    }
    Ok(())
}

fn invalid_archive(message: impl Into<String>) -> DzipError {
    DzipError::InvalidArchive(message.into())
}
