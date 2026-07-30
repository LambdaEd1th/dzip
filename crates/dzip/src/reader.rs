use crate::error::{DzipError, Result};
use crate::format::*;
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{BufRead, BufReader, Read, Seek};

pub use crate::codec::DzDecodeContext;

pub struct DzipReader<R: Read + Seek> {
    reader: BufReader<R>,
}

impl<R: Read + Seek> DzipReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
        }
    }

    pub fn read_archive_settings(&mut self) -> Result<ArchiveSettings> {
        let header = self.reader.read_u32::<LittleEndian>()?;
        if header != 0x5A525444 {
            // 'DTRZ' in little endian (ZRTD)
            return Err(DzipError::InvalidHeader);
        }

        let num_user_files = self.reader.read_u16::<LittleEndian>()?;
        let num_directories = self.reader.read_u16::<LittleEndian>()?;
        let version = self.reader.read_u8()?;
        if version != 0 {
            return Err(DzipError::UnsupportedVersion(version));
        }
        if num_directories == 0 {
            return Err(DzipError::InvalidArchive(
                "archive must contain the implicit root directory".to_string(),
            ));
        }

        Ok(ArchiveSettings {
            header,
            num_user_files,
            num_directories,
            version,
        })
    }

    pub fn read_strings(&mut self, count: usize) -> Result<Vec<String>> {
        self.read_strings_with_limit(count, usize::MAX - 1)
    }

    pub fn read_strings_with_limit(
        &mut self,
        count: usize,
        max_string_length: usize,
    ) -> Result<Vec<String>> {
        self.read_strings_with_limits(count, max_string_length, usize::MAX)
    }

    pub fn read_strings_with_limits(
        &mut self,
        count: usize,
        max_string_length: usize,
        max_total_length: usize,
    ) -> Result<Vec<String>> {
        let mut strings = Vec::with_capacity(count);
        let mut total_length = 0usize;
        for _ in 0..count {
            let s = self.read_null_terminated_string(max_string_length)?;
            total_length = total_length
                .checked_add(s.len() + 1)
                .ok_or_else(|| DzipError::InvalidArchive("metadata size overflow".to_string()))?;
            if total_length > max_total_length {
                return Err(DzipError::LimitExceeded {
                    resource: "metadata strings",
                    limit: max_total_length as u64,
                    actual: total_length as u64,
                });
            }
            strings.push(s);
        }
        Ok(strings)
    }

    fn read_null_terminated_string(&mut self, max_length: usize) -> Result<String> {
        let mut bytes = Vec::new();
        let max_with_terminator = max_length.saturating_add(1);
        let read = self
            .reader
            .by_ref()
            .take(max_with_terminator as u64)
            .read_until(0, &mut bytes)?;
        if read == 0 || bytes.last() != Some(&0) {
            return Err(DzipError::InvalidArchive(
                "unterminated or oversized metadata string".to_string(),
            ));
        }
        bytes.pop();
        Ok(String::from_utf8(bytes)?)
    }

    /// Reads the User-File to Chunk-And-Directory list.
    /// Returns a vector of tuples: (Directory ID, List of Chunk IDs).
    pub fn read_file_chunk_map(&mut self, num_files: usize) -> Result<Vec<(u16, Vec<u16>)>> {
        self.read_file_chunk_map_with_limit(num_files, usize::MAX)
    }

    pub fn read_file_chunk_map_with_limit(
        &mut self,
        num_files: usize,
        max_chunks_per_file: usize,
    ) -> Result<Vec<(u16, Vec<u16>)>> {
        self.read_file_chunk_map_with_limits(num_files, max_chunks_per_file, usize::MAX)
    }

    pub fn read_file_chunk_map_with_limits(
        &mut self,
        num_files: usize,
        max_chunks_per_file: usize,
        max_total_chunks: usize,
    ) -> Result<Vec<(u16, Vec<u16>)>> {
        let mut map = Vec::with_capacity(num_files);
        let mut total_chunks = 0usize;
        for _ in 0..num_files {
            let dir_id = self.reader.read_u16::<LittleEndian>()?;
            let mut chunks = Vec::new();
            loop {
                let chunk_id = self.reader.read_u16::<LittleEndian>()?;
                if chunk_id == 0xFFFF {
                    break;
                }
                if chunks.len() >= max_chunks_per_file {
                    return Err(DzipError::LimitExceeded {
                        resource: "chunks per entry",
                        limit: max_chunks_per_file as u64,
                        actual: chunks.len() as u64 + 1,
                    });
                }
                total_chunks = total_chunks.checked_add(1).ok_or_else(|| {
                    DzipError::InvalidArchive("chunk-reference count overflow".to_string())
                })?;
                if total_chunks > max_total_chunks {
                    return Err(DzipError::LimitExceeded {
                        resource: "chunk references",
                        limit: max_total_chunks as u64,
                        actual: total_chunks as u64,
                    });
                }
                chunks.push(chunk_id);
            }
            map.push((dir_id, chunks));
        }
        Ok(map)
    }

    pub fn read_chunk_settings(&mut self) -> Result<ChunkSettings> {
        let num_archive_files = self.reader.read_u16::<LittleEndian>()?;
        let num_chunks = self.reader.read_u16::<LittleEndian>()?;
        Ok(ChunkSettings {
            num_archive_files,
            num_chunks,
        })
    }

    pub fn read_chunks(&mut self, count: usize) -> Result<Vec<Chunk>> {
        let mut chunks = Vec::with_capacity(count);
        for _ in 0..count {
            let offset = self.reader.read_u32::<LittleEndian>()?;
            let compressed_length = self.reader.read_u32::<LittleEndian>()?;
            let decompressed_length = self.reader.read_u32::<LittleEndian>()?;
            let flags = self.reader.read_u16::<LittleEndian>()?;
            let file = self.reader.read_u16::<LittleEndian>()?;
            chunks.push(Chunk {
                offset,
                compressed_length,
                decompressed_length,
                flags,
                file,
            });
        }
        Ok(chunks)
    }

    pub fn read_global_settings(&mut self) -> Result<RangeSettings> {
        let win_size = self.reader.read_u8()?;
        let flags = self.reader.read_u8()?;
        let offset_table_size = self.reader.read_u8()?;
        let offset_tables = self.reader.read_u8()?;
        let offset_contexts = self.reader.read_u8()?;
        let ref_length_table_size = self.reader.read_u8()?;
        let ref_length_tables = self.reader.read_u8()?;
        let ref_offset_table_size = self.reader.read_u8()?;
        let ref_offset_tables = self.reader.read_u8()?;
        let big_min_match = self.reader.read_u8()?;

        Ok(RangeSettings {
            win_size,
            flags,
            offset_table_size,
            offset_tables,
            offset_contexts,
            ref_length_table_size,
            ref_length_tables,
            ref_offset_table_size,
            ref_offset_tables,
            big_min_match,
        })
    }

    pub fn read_file_list(&mut self, num_archive_files: usize) -> Result<Vec<String>> {
        self.read_strings(num_archive_files)
    }

    pub fn position(&mut self) -> std::io::Result<u64> {
        self.reader.stream_position()
    }

    pub fn stream_len(&mut self) -> std::io::Result<u64> {
        let position = self.reader.stream_position()?;
        let length = self.reader.seek(std::io::SeekFrom::End(0))?;
        self.reader.seek(std::io::SeekFrom::Start(position))?;
        Ok(length)
    }

    pub fn read_chunk_data(&mut self, chunk: &Chunk) -> Result<Vec<u8>> {
        Self::decompress_chunk_data(&mut self.reader, chunk, None)
    }

    pub fn read_chunk_data_with_volumes(
        &mut self,
        chunk: &Chunk,
        volume_source: &mut dyn VolumeSource,
    ) -> Result<Vec<u8>> {
        self.read_chunk_data_with_context(chunk, volume_source, None)
    }

    pub fn read_chunk_data_with_context(
        &mut self,
        chunk: &Chunk,
        volume_source: &mut dyn VolumeSource,
        dz_context: Option<&DzDecodeContext>,
    ) -> Result<Vec<u8>> {
        if chunk.file == 0 {
            Self::decompress_chunk_data(&mut self.reader, chunk, dz_context)
        } else {
            let reader = volume_source.open_volume(chunk.file)?;
            Self::decompress_chunk_data(reader, chunk, dz_context)
        }
    }

    pub fn load_dz_context(
        &mut self,
        chunks: &[Chunk],
        settings: RangeSettings,
        volume_source: &mut dyn VolumeSource,
    ) -> Result<DzDecodeContext> {
        let settings = settings.validate()?;
        let common_chunks: Vec<_> = chunks
            .iter()
            .filter(|chunk| chunk.flags & CHUNK_COMBUF != 0)
            .copied()
            .collect();
        if common_chunks.is_empty() {
            return DzDecodeContext::from_encoded_chunks(settings, Vec::new());
        }

        let mut encoded_chunks = Vec::with_capacity(common_chunks.len());
        for chunk in common_chunks {
            let mut data = vec![0u8; chunk.compressed_length as usize];
            if chunk.file == 0 {
                self.reader
                    .seek(std::io::SeekFrom::Start(u64::from(chunk.offset)))?;
                self.reader.read_exact(&mut data)?;
            } else {
                let reader = volume_source.open_volume(chunk.file)?;
                reader.seek(std::io::SeekFrom::Start(u64::from(chunk.offset)))?;
                reader.read_exact(&mut data)?;
            }
            encoded_chunks.push(data);
        }
        DzDecodeContext::from_encoded_chunks(settings, encoded_chunks)
    }

    fn decompress_chunk_data(
        reader: &mut dyn ReadSeek,
        chunk: &Chunk,
        dz_context: Option<&DzDecodeContext>,
    ) -> Result<Vec<u8>> {
        let encoding = crate::codec::ChunkEncoding::from_flags(chunk.flags)?;

        // Zero chunks have no physical stream and may carry a virtual offset.
        if encoding.compression == crate::codec::Compression::Zero {
            return crate::codec::decode(
                encoding,
                &[],
                crate::codec::DecodeContext {
                    expected_len: chunk.decompressed_length as usize,
                    dz: dz_context,
                },
            );
        }

        reader.seek(std::io::SeekFrom::Start(chunk.offset as u64))?;
        let mut buffer = vec![0u8; chunk.compressed_length as usize];
        reader.read_exact(&mut buffer)?;

        crate::codec::decode(
            encoding,
            &buffer,
            crate::codec::DecodeContext {
                expected_len: chunk.decompressed_length as usize,
                dz: dz_context,
            },
        )
    }
}

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

pub trait VolumeSource {
    /// Open the volume with the given index (1-based, corresponding to the file list)
    fn open_volume(&mut self, id: u16) -> Result<&mut dyn ReadSeek>;

    fn volume_len(&mut self, id: u16) -> Result<Option<u64>> {
        let reader = self.open_volume(id)?;
        let position = reader.stream_position()?;
        let length = reader.seek(std::io::SeekFrom::End(0))?;
        reader.seek(std::io::SeekFrom::Start(position))?;
        Ok(Some(length))
    }
}

/// Corrects chunk sizes based on actual file boundaries.
///
/// Some archives (like testnew.dz) have incorrect compressed_length headers (e.g., listing uncompressed size).
/// This function clamps compressed lengths to the available space between chunks or EOF.
///
/// # Arguments
/// * `chunks` - The list of chunks to correct.
/// * `file_sizes` - specific file sizes mapped by file ID (0 for main, 1+ for volumes).
pub fn correct_chunk_sizes(
    chunks: &mut [crate::format::Chunk],
    file_sizes: &std::collections::HashMap<u16, u64>,
) {
    use crate::format::*;
    let mut chunks_by_file: std::collections::HashMap<u16, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, chunk) in chunks.iter().enumerate() {
        // Virtual zero chunks and zero-length COMBUF placeholders occupy no
        // bytes and must not become a boundary for a physical neighbor at the
        // same offset.
        if chunk.flags & CHUNK_ZERO != 0 || chunk.compressed_length == 0 {
            continue;
        }
        chunks_by_file.entry(chunk.file).or_default().push(i);
    }

    for (file_id, mut indices) in chunks_by_file {
        indices.sort_by_key(|&i| chunks[i].offset);

        let file_size = *file_sizes.get(&file_id).unwrap_or(&0);

        for i in 0..indices.len() {
            let idx = indices[i];
            let chunk_offset = chunks[idx].offset as u64;

            // Determine the limit (end of region)
            let limit = if i + 1 < indices.len() {
                chunks[indices[i + 1]].offset as u64
            } else {
                file_size
            };

            let available = limit.saturating_sub(chunk_offset);

            // If header claims more than available, clamp it.
            // BMS Logic: If SIZE == ZSIZE (equal lengths) for compressed chunks, it means
            // the size is unknown/placeholder, so we SHOULD use the available size (next offset - current).
            let is_compressed =
                (chunks[idx].flags & (CHUNK_LZMA | CHUNK_ZLIB | CHUNK_BZIP | CHUNK_DZ)) != 0;
            let equal_sizes = chunks[idx].compressed_length == chunks[idx].decompressed_length;

            if is_compressed && equal_sizes {
                // Always update to available size (whether larger or smaller)
                if chunks[idx].compressed_length != available as u32 {
                    chunks[idx].compressed_length = available as u32;
                }
            } else if (chunks[idx].compressed_length as u64) > available {
                chunks[idx].compressed_length = available as u32;
            }
        }
    }
}
