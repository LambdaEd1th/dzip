//! High-level deterministic Dzip archive creation.

use crate::codec::{ChunkEncoding, Compression, ContentHint};
use crate::format::*;
use crate::path::{sanitize_path, to_archive_format};
use crate::writer::DzipWriter;
use crate::{DzipError, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub use crate::options::{EntryOptions, PackOptions};

pub trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

pub trait VolumeSink {
    fn open_volume(&mut self, id: u16, name: &str) -> Result<&mut dyn WriteSeek>;
}

pub struct FileSystemVolumeSink {
    output_dir: PathBuf,
    files: HashMap<u16, File>,
}

impl FileSystemVolumeSink {
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            files: HashMap::new(),
        }
    }
}

impl VolumeSink for FileSystemVolumeSink {
    fn open_volume(&mut self, id: u16, name: &str) -> Result<&mut dyn WriteSeek> {
        if !self.files.contains_key(&id) {
            let relative = sanitize_path(Path::new(name))?;
            let path = self.output_dir.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let file = File::create(path)?;
            self.files.insert(id, file);
        }
        Ok(self
            .files
            .get_mut(&id)
            .expect("volume was inserted before lookup"))
    }
}

#[derive(Default)]
pub struct MemoryVolumeSink {
    volumes: HashMap<u16, Cursor<Vec<u8>>>,
    names: HashMap<u16, String>,
}

impl MemoryVolumeSink {
    pub fn volume(&self, id: u16) -> Option<&[u8]> {
        self.volumes
            .get(&id)
            .map(|writer| writer.get_ref().as_slice())
    }

    pub fn name(&self, id: u16) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }

    pub fn into_volumes(self) -> HashMap<u16, Vec<u8>> {
        self.volumes
            .into_iter()
            .map(|(id, writer)| (id, writer.into_inner()))
            .collect()
    }
}

impl VolumeSink for MemoryVolumeSink {
    fn open_volume(&mut self, id: u16, name: &str) -> Result<&mut dyn WriteSeek> {
        self.names.entry(id).or_insert_with(|| name.to_string());
        Ok(self.volumes.entry(id).or_default())
    }
}

struct SingleVolumeSink<'a, W> {
    writer: &'a mut W,
}

impl<W: Write + Seek> VolumeSink for SingleVolumeSink<'_, W> {
    fn open_volume(&mut self, id: u16, _name: &str) -> Result<&mut dyn WriteSeek> {
        if id != 0 {
            return Err(invalid_input(
                "single-writer output cannot contain auxiliary volumes",
            ));
        }
        Ok(self.writer)
    }
}

enum EntrySource {
    Bytes(Vec<u8>),
    Path(PathBuf),
}

struct BuilderEntry {
    archive_path: PathBuf,
    source: EntrySource,
    options: EntryOptions,
}

pub struct ArchiveBuilder {
    options: PackOptions,
    entries: Vec<BuilderEntry>,
}

impl Default for ArchiveBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ArchiveBuilder {
    pub fn new() -> Self {
        Self {
            options: PackOptions::default(),
            entries: Vec::new(),
        }
    }

    pub fn with_options(options: PackOptions) -> Self {
        Self {
            options,
            entries: Vec::new(),
        }
    }

    pub fn options(&self) -> &PackOptions {
        &self.options
    }

    pub fn options_mut(&mut self) -> &mut PackOptions {
        &mut self.options
    }

    pub fn add_bytes(
        &mut self,
        archive_path: impl AsRef<Path>,
        bytes: impl Into<Vec<u8>>,
        options: EntryOptions,
    ) -> Result<&mut Self> {
        let archive_path = validate_archive_path(archive_path.as_ref())?;
        self.entries.push(BuilderEntry {
            archive_path,
            source: EntrySource::Bytes(bytes.into()),
            options,
        });
        Ok(self)
    }

    pub fn add_path(
        &mut self,
        archive_path: impl AsRef<Path>,
        source_path: impl Into<PathBuf>,
        options: EntryOptions,
    ) -> Result<&mut Self> {
        let archive_path = validate_archive_path(archive_path.as_ref())?;
        self.entries.push(BuilderEntry {
            archive_path,
            source: EntrySource::Path(source_path.into()),
            options,
        });
        Ok(self)
    }

    pub fn write_to<W: Write + Seek>(&self, writer: &mut W) -> Result<BuildReport> {
        if self.options.volume_names.len() != 1
            || self.entries.iter().any(|entry| entry.options.volume != 0)
        {
            return Err(invalid_input(
                "write_to supports one volume; use write_to_sink for split archives",
            ));
        }
        let mut sink = SingleVolumeSink { writer };
        self.write_to_sink(&mut sink)
    }

    pub fn write_to_path(&self, path: impl AsRef<Path>) -> Result<BuildReport> {
        let path = path.as_ref();
        let output_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let main_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid_input("output path has no UTF-8 file name"))?;
        let mut options = self.options.clone();
        if options.volume_names.is_empty() {
            options.volume_names.push(main_name.to_string());
        } else {
            options.volume_names[0] = main_name.to_string();
        }
        let prepared = self.prepare(&options)?;
        let mut sink = FileSystemVolumeSink::new(output_dir);
        write_prepared(&prepared, &options, &mut sink)
    }

    pub fn write_to_directory(&self, output_dir: impl AsRef<Path>) -> Result<BuildReport> {
        let mut sink = FileSystemVolumeSink::new(output_dir.as_ref());
        self.write_to_sink(&mut sink)
    }

    pub fn write_to_sink<S: VolumeSink>(&self, sink: &mut S) -> Result<BuildReport> {
        let prepared = self.prepare(&self.options)?;
        write_prepared(&prepared, &self.options, sink)
    }

    fn prepare(&self, options: &PackOptions) -> Result<Vec<PreparedEntry>> {
        if options.volume_names.is_empty() {
            return Err(invalid_input("at least one output volume is required"));
        }
        if options.volume_names.len() > u16::MAX as usize {
            return Err(invalid_input("more than 65535 output volumes"));
        }

        let read_entry = |entry: &BuilderEntry| -> Result<PreparedEntry> {
            if entry.options.volume as usize >= options.volume_names.len() {
                return Err(DzipError::VolumeNotFound(entry.options.volume));
            }
            let data = match &entry.source {
                EntrySource::Bytes(bytes) => bytes.clone(),
                EntrySource::Path(path) => std::fs::read(path).map_err(|error| {
                    DzipError::Io(std::io::Error::new(
                        error.kind(),
                        format!("failed to read {}: {error}", path.display()),
                    ))
                })?,
            };
            effective_compression(entry.options)?;
            Ok(PreparedEntry {
                archive_path: entry.archive_path.clone(),
                data,
                options: entry.options,
            })
        };

        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            self.entries.par_iter().map(read_entry).collect()
        }
        #[cfg(not(feature = "parallel"))]
        {
            self.entries.iter().map(read_entry).collect()
        }
    }
}

struct PreparedEntry {
    archive_path: PathBuf,
    data: Vec<u8>,
    options: EntryOptions,
}

struct LogicalFile {
    path: PathBuf,
    segment_indices: Vec<usize>,
}

struct ProcessedEntry {
    logical_index: usize,
    volume: u16,
    compressed: Vec<u8>,
    original_len: usize,
    flags: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuildReport {
    pub entries: usize,
    pub chunks: usize,
    pub input_bytes: u64,
    pub stored_bytes: u64,
    pub volumes: usize,
}

fn write_prepared<S: VolumeSink>(
    prepared: &[PreparedEntry],
    options: &PackOptions,
    sink: &mut S,
) -> Result<BuildReport> {
    let compressions = prepared
        .iter()
        .map(|entry| effective_compression(entry.options))
        .collect::<Result<Vec<_>>>()?;
    let (logical_files, segment_to_logical) = group_logical_files(prepared);
    let (all_strings, file_directory_ids, directory_count) = build_string_table(&logical_files)?;
    let file_count = checked_u16(logical_files.len(), "user file count")?;
    let volume_count = checked_u16(options.volume_names.len(), "archive volume count")?;

    for (id, name) in options.volume_names.iter().enumerate() {
        let id = checked_u16(id, "archive volume index")?;
        sink.open_volume(id, name)?;
    }

    let dz_inputs: Vec<Vec<u8>> = prepared
        .iter()
        .zip(&compressions)
        .filter(|(_, compression)| **compression == Compression::Dz)
        .map(|(entry, _)| entry.data.clone())
        .collect();
    let encoded_dz = if dz_inputs.is_empty() {
        None
    } else {
        Some(crate::codec::dz::encode_archive(&dz_inputs, &options.dz)?)
    };
    let mut next_dz = 0usize;
    let dz_indices = compressions
        .iter()
        .map(|compression| {
            if *compression == Compression::Dz {
                let index = next_dz;
                next_dz += 1;
                Some(index)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let process_entry = |(index, entry): (usize, &PreparedEntry)| -> Result<ProcessedEntry> {
        let compression = compressions[index];
        let compressed = if compression == Compression::Dz {
            encoded_dz
                .as_ref()
                .and_then(|archive| archive.chunks.get(dz_indices[index]?))
                .ok_or_else(|| invalid_input("missing archive-scoped DZ result"))?
                .clone()
        } else {
            crate::codec::encode(compression, &entry.data, options.dz.settings)?
        };
        let flags = entry.options.raw_flags.unwrap_or_else(|| {
            let mut flags = compression.flag();
            if entry.options.random_access {
                flags |= CHUNK_RANDOMACCESS;
            }
            flags |= match entry.options.content_hint {
                Some(ContentHint::Mp3) => CHUNK_MP3,
                Some(ContentHint::Jpeg) => CHUNK_JPEG,
                Some(ContentHint::Mp3AndJpeg) => CHUNK_MP3 | CHUNK_JPEG,
                None => 0,
            };
            flags
        });
        Ok(ProcessedEntry {
            logical_index: segment_to_logical[index],
            volume: entry.options.volume,
            compressed,
            original_len: entry.data.len(),
            flags,
        })
    };
    #[cfg(feature = "parallel")]
    let processed = {
        use rayon::prelude::*;
        prepared
            .par_iter()
            .enumerate()
            .map(process_entry)
            .collect::<Result<Vec<_>>>()?
    };
    #[cfg(not(feature = "parallel"))]
    let processed = prepared
        .iter()
        .enumerate()
        .map(process_entry)
        .collect::<Result<Vec<_>>>()?;

    let has_dz = !dz_inputs.is_empty();
    let common_buffer = encoded_dz.and_then(|archive| archive.common_buffer);
    let chunk_count = processed
        .len()
        .checked_add(usize::from(common_buffer.is_some()))
        .ok_or_else(|| invalid_input("chunk count overflow"))?;
    let chunk_count_u16 = checked_u16(chunk_count, "chunk count")?;
    let header_size = calculate_header_size(
        &all_strings,
        &logical_files,
        chunk_count_u16,
        &options.volume_names,
        has_dz,
    )?;
    sink.open_volume(0, &options.volume_names[0])?
        .seek(SeekFrom::Start(header_size))?;

    // dzip.exe applies `align` to the beginning of each volume's payload
    // region. Chunks within that region remain tightly packed.
    for (id, name) in options.volume_names.iter().enumerate() {
        let id = checked_u16(id, "archive volume index")?;
        pad_writer_to_alignment(sink.open_volume(id, name)?, options.alignment)?;
    }

    let mut chunks = vec![
        Chunk {
            offset: 0,
            compressed_length: 0,
            decompressed_length: 0,
            flags: 0,
            file: 0,
        };
        processed.len()
    ];
    let mut logical_chunk_ids = vec![Vec::new(); logical_files.len()];
    for (index, entry) in processed.iter().enumerate() {
        logical_chunk_ids[entry.logical_index].push(checked_u16(index, "chunk index")?);
    }

    let mut write_order: Vec<usize> = (0..processed.len()).collect();
    write_order.sort_by_key(|index| chunk_write_rank(processed[*index].flags));
    for &index in write_order
        .iter()
        .filter(|index| processed[**index].flags & CHUNK_DZ == 0)
    {
        write_chunk(index, &processed, &mut chunks, options, sink)?;
    }

    if let Some(common) = common_buffer {
        let writer = sink.open_volume(0, &options.volume_names[0])?;
        let offset = checked_u32(writer.stream_position()? as usize, "COMBUF offset")?;
        writer.write_all(&common)?;
        chunks.push(Chunk {
            offset,
            compressed_length: checked_u32(common.len(), "COMBUF compressed length")?,
            decompressed_length: 0,
            flags: CHUNK_COMBUF,
            file: 0,
        });
    }

    for &index in write_order
        .iter()
        .filter(|index| processed[**index].flags & CHUNK_DZ != 0)
    {
        write_chunk(index, &processed, &mut chunks, options, sink)?;
    }

    let chunk_map: Vec<_> = logical_chunk_ids
        .into_iter()
        .enumerate()
        .map(|(index, chunks)| (file_directory_ids[index], chunks))
        .collect();
    let main = sink.open_volume(0, &options.volume_names[0])?;
    main.seek(SeekFrom::Start(0))?;
    let mut writer = DzipWriter::new(main);
    writer.write_archive_settings(&ArchiveSettings {
        header: 0x5A52_5444,
        num_user_files: file_count,
        num_directories: directory_count,
        version: 0,
    })?;
    writer.write_strings(&all_strings)?;
    writer.write_file_chunk_map(&chunk_map)?;
    writer.write_chunk_settings(&ChunkSettings {
        num_archive_files: volume_count,
        num_chunks: chunk_count_u16,
    })?;
    writer.write_chunks(&chunks)?;
    writer.write_strings(&options.volume_names[1..])?;
    if has_dz {
        writer.write_global_settings(&options.dz.settings)?;
    }

    let input_bytes = prepared.iter().try_fold(0u64, |total, entry| {
        total
            .checked_add(entry.data.len() as u64)
            .ok_or_else(|| invalid_input("input byte count overflow"))
    })?;
    let stored_bytes = processed.iter().try_fold(0u64, |total, entry| {
        total
            .checked_add(entry.compressed.len() as u64)
            .ok_or_else(|| invalid_input("stored byte count overflow"))
    })?;
    Ok(BuildReport {
        entries: logical_files.len(),
        chunks: chunk_count,
        input_bytes,
        stored_bytes,
        volumes: options.volume_names.len(),
    })
}

fn effective_compression(options: EntryOptions) -> Result<Compression> {
    match options.raw_flags {
        Some(flags) => Ok(ChunkEncoding::from_flags(flags)?.compression),
        None => Ok(options.compression),
    }
}

fn write_chunk<S: VolumeSink>(
    index: usize,
    processed: &[ProcessedEntry],
    chunks: &mut [Chunk],
    options: &PackOptions,
    sink: &mut S,
) -> Result<()> {
    let entry = &processed[index];
    let name = options
        .volume_names
        .get(entry.volume as usize)
        .ok_or(DzipError::VolumeNotFound(entry.volume))?;
    let writer = sink.open_volume(entry.volume, name)?;
    let offset_u64 = writer.stream_position()?;
    let offset = u32::try_from(offset_u64)
        .map_err(|_| invalid_input("chunk offset exceeds the 32-bit format field"))?;
    writer.write_all(&entry.compressed)?;
    let compressed_length = if entry.flags & (CHUNK_ZLIB | CHUNK_BZIP | CHUNK_LZMA) != 0 {
        entry.original_len
    } else {
        entry.compressed.len()
    };
    chunks[index] = Chunk {
        offset,
        compressed_length: checked_u32(compressed_length, "chunk compressed length")?,
        decompressed_length: checked_u32(entry.original_len, "chunk decompressed length")?,
        flags: entry.flags,
        file: entry.volume,
    };
    Ok(())
}

fn group_logical_files(prepared: &[PreparedEntry]) -> (Vec<LogicalFile>, Vec<usize>) {
    let mut logical_files = Vec::<LogicalFile>::new();
    let mut lookup = HashMap::new();
    let mut segment_to_logical = Vec::with_capacity(prepared.len());
    for (segment_index, entry) in prepared.iter().enumerate() {
        let key = to_archive_format(&entry.archive_path).to_ascii_lowercase();
        let logical_index = if let Some(index) = lookup.get(&key).copied() {
            index
        } else {
            let index = logical_files.len();
            logical_files.push(LogicalFile {
                path: entry.archive_path.clone(),
                segment_indices: Vec::new(),
            });
            lookup.insert(key, index);
            index
        };
        logical_files[logical_index]
            .segment_indices
            .push(segment_index);
        segment_to_logical.push(logical_index);
    }
    (logical_files, segment_to_logical)
}

fn build_string_table(logical: &[LogicalFile]) -> Result<(Vec<String>, Vec<u16>, u16)> {
    let mut file_names = Vec::with_capacity(logical.len());
    let mut directories = Vec::new();
    let mut directory_lookup = HashMap::new();
    let mut directory_ids = Vec::with_capacity(logical.len());
    for file in logical {
        let file_name = file
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid_input(format!("invalid UTF-8 path {}", file.path.display())))?;
        file_names.push(file_name.to_string());
        let parent = file.path.parent().unwrap_or_else(|| Path::new(""));
        let parent = to_archive_format(parent);
        if parent.is_empty() || parent == "." {
            directory_ids.push(0);
        } else {
            let key = parent.to_ascii_lowercase();
            let id = if let Some(id) = directory_lookup.get(&key).copied() {
                id
            } else {
                directories.push(parent);
                let id = checked_u16(directories.len(), "directory count")?;
                directory_lookup.insert(key, id);
                id
            };
            directory_ids.push(id);
        }
    }
    let directory_count = checked_u16(
        directories
            .len()
            .checked_add(1)
            .ok_or_else(|| invalid_input("directory count overflow"))?,
        "directory count",
    )?;
    file_names.extend(directories);
    Ok((file_names, directory_ids, directory_count))
}

fn calculate_header_size(
    strings: &[String],
    logical: &[LogicalFile],
    chunk_count: u16,
    volume_names: &[String],
    has_dz: bool,
) -> Result<u64> {
    let mut size = 9u64;
    for string in strings {
        size = size
            .checked_add(string.len() as u64 + 1)
            .ok_or_else(|| invalid_input("header size overflow"))?;
    }
    for file in logical {
        size = size
            .checked_add(4 + file.segment_indices.len() as u64 * 2)
            .ok_or_else(|| invalid_input("header size overflow"))?;
    }
    size = size
        .checked_add(4 + u64::from(chunk_count) * 16)
        .ok_or_else(|| invalid_input("header size overflow"))?;
    for name in &volume_names[1..] {
        size = size
            .checked_add(name.len() as u64 + 1)
            .ok_or_else(|| invalid_input("header size overflow"))?;
    }
    if has_dz {
        size = size
            .checked_add(10)
            .ok_or_else(|| invalid_input("header size overflow"))?;
    }
    Ok(size)
}

fn validate_archive_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(invalid_input("archive path must not be empty"));
    }
    if path.to_str().is_none() {
        return Err(invalid_input(format!(
            "archive path is not UTF-8: {}",
            path.display()
        )));
    }
    sanitize_path(path)
}

fn pad_writer_to_alignment(writer: &mut dyn WriteSeek, alignment: u32) -> std::io::Result<()> {
    if alignment <= 1 {
        return Ok(());
    }
    let position = writer.stream_position()?;
    let alignment = u64::from(alignment);
    let padding = (alignment - position % alignment) % alignment;
    if padding != 0 {
        writer.write_all(&vec![0; padding as usize])?;
    }
    Ok(())
}

fn chunk_write_rank(flags: u16) -> u8 {
    if flags & CHUNK_ZERO != 0 {
        0
    } else if flags & CHUNK_BZIP != 0 {
        1
    } else if flags & CHUNK_COPYCOMP != 0 {
        2
    } else if flags & CHUNK_ZLIB != 0 {
        3
    } else if flags & CHUNK_LZMA != 0 {
        4
    } else if flags & CHUNK_DZ != 0 {
        5
    } else {
        6
    }
}

fn checked_u16(value: usize, label: &'static str) -> Result<u16> {
    u16::try_from(value).map_err(|_| invalid_input(format!("{label} exceeds 65535")))
}

fn checked_u32(value: usize, label: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| invalid_input(format!("{label} exceeds 32-bit limit")))
}

fn invalid_input(message: impl Into<String>) -> DzipError {
    DzipError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}
