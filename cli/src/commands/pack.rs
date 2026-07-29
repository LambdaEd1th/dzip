use crate::config;
use dzip_core::dz::{DzEncoderOptions, compress_archive};
use dzip_core::format::{
    ArchiveSettings, CHUNK_COMBUF, CHUNK_DZ, CHUNK_LZMA, Chunk, ChunkSettings, RangeSettings,
};
use dzip_core::{CompressionMethod, Result, compress_data};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, info};
use rayon::prelude::*;
use std::io::{Seek, SeekFrom, Write};

fn pad_writer_to_alignment<W: Write + Seek>(writer: &mut W, alignment: u32) -> std::io::Result<()> {
    if alignment <= 1 {
        return Ok(());
    }

    let position = writer.stream_position()?;
    let alignment = u64::from(alignment);
    let padding = (alignment - (position % alignment)) % alignment;
    if padding != 0 {
        writer.write_all(&vec![0u8; padding as usize])?;
    }

    Ok(())
}

pub fn pack_archive(input_path: &str, output_dir: &str) -> Result<()> {
    let config_path = std::path::Path::new(input_path);
    info!("Parsing config file: {}", config_path.display());
    let mut config = config::parse_config(config_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // If base_dir is "." (default), make it relative to the config file's directory
    #[allow(clippy::collapsible_if)]
    if config.base_dir == std::path::Path::new(".") {
        if let Some(parent) = config_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            config.base_dir = parent.to_path_buf();
        }
    }

    std::fs::create_dir_all(output_dir)?;

    struct LogicalFile {
        path: std::path::PathBuf,
        segment_indices: Vec<usize>,
    }

    let mut logical_files = Vec::new();
    let mut logical_file_map = std::collections::HashMap::new();
    let mut segment_to_logical = Vec::with_capacity(config.files.len());

    for (segment_index, entry) in config.files.iter().enumerate() {
        let archive_path = dzip_core::path::to_archive_format(&entry.path);
        let logical_index = if let Some(&existing_index) = logical_file_map.get(&archive_path) {
            existing_index
        } else {
            let new_index = logical_files.len();
            logical_files.push(LogicalFile {
                path: entry.path.clone(),
                segment_indices: Vec::new(),
            });
            logical_file_map.insert(archive_path, new_index);
            new_index
        };

        logical_files[logical_index]
            .segment_indices
            .push(segment_index);
        segment_to_logical.push(logical_index);
    }

    // --- Prepare Metadata ---
    // 1. Strings: User Files + Unique Directories
    // Note: Dzip strings table contains filenames (basename) and directory paths.
    // The exact structure is: [List of User Filenames], [List of Directory Paths].
    // Wait, the format in unpacking:
    // strings = reader.read_strings(settings.num_user_files + settings.num_directories - 1)?
    // And map points to dir_index.
    // So strings table is: [file1_name, file2_name, ..., dir1_path, dir2_path, ...].
    // Note root dir is implicit/empty and usually not in strings table?
    // Unpacker: `dir_index = num_user_files + (dir_id - 1)`.
    // If dir_id=1, index = num_user_files.
    // So yes, strings list is [Files..., Dir1, Dir2...].

    // Collect File Names
    let mut file_names = Vec::new();
    for logical_file in &logical_files {
        // Use filename component
        if let Some(name) = logical_file.path.file_name() {
            file_names.push(name.to_string_lossy().to_string());
        } else {
            return Err(
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid file path").into(),
            );
        }
    }

    // Collect Unique Directories and assign IDs
    let mut directories = Vec::new();
    let mut dir_map = std::collections::HashMap::new(); // path -> dir_id (1-based)

    // Directory ID 0 is Root.
    // We need to map each file to a dir_id.
    let mut file_dir_ids = Vec::new();

    for logical_file in &logical_files {
        let parent = logical_file
            .path
            .parent()
            .unwrap_or(std::path::Path::new(""));
        // Force Windows-style backslashes as requested using core utility
        let parent_str = dzip_core::path::to_archive_format(parent);

        if parent_str.is_empty() || parent_str == "." {
            file_dir_ids.push(0u16);
        } else {
            // Check if known
            if let Some(&id) = dir_map.get(&parent_str) {
                file_dir_ids.push(id);
            } else {
                // New directory
                // Directories list stores paths.
                directories.push(parent_str.clone());
                let id = directories.len() as u16; // 1-based
                dir_map.insert(parent_str, id);
                file_dir_ids.push(id);
            }
        }
    }

    let num_user_files = logical_files.len() as u16;
    let num_directories = (directories.len() + 1) as u16; // +1 for Root?
    // Unpacker: `strings_count = num_user_files + num_directories - 1`.
    // So strings count = files + dirs.
    // Strings array = [Files..., Dirs...].
    // Root dir is NOT in strings.

    let mut all_strings = file_names;
    all_strings.extend(directories);

    // --- Open Volumes ---
    if config.archives.is_empty() {
        return Err(
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "No archives specified").into(),
        );
    }

    let mut writers = std::collections::HashMap::new();
    for (i, name) in config.archives.iter().enumerate() {
        let path = std::path::Path::new(output_dir).join(name);
        info!("Opening volume {}: {}", i, path.display());
        let f = std::fs::File::create(&path)?;
        writers.insert(i as u16, f);
    }

    struct PreparedFile {
        logical_file_index: usize,
        archive_id: u16,
        data: Vec<u8>,
        method: CompressionMethod,
    }

    // Read and slice inputs in parallel. DZ compression itself is archive-scoped
    // because it may build a cross-file common buffer.
    info!("Reading source chunks in parallel...");
    let pb = ProgressBar::new(config.files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );

    let prepared_files: Vec<PreparedFile> = config
        .files
        .par_iter()
        .enumerate()
        .map(|(i, entry)| {
            let full_path = entry
                .source_base_dir
                .as_ref()
                .unwrap_or(&config.base_dir)
                .join(&entry.path);
            debug!("Processing file {}: {}", i, full_path.display());
            pb.set_message(format!("Compressing {}", entry.path.display()));

            let raw_data = std::fs::read(&full_path).map_err(|e| {
                dzip_core::DzipError::Io(std::io::Error::other(format!(
                    "Failed to read {}: {}",
                    full_path.display(),
                    e
                )))
            })?;
            let (start, end) = entry.byte_range(raw_data.len()).map_err(|error| {
                dzip_core::DzipError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    error.to_string(),
                ))
            })?;
            let sliced_data = raw_data[start..end].to_vec();
            pb.inc(1);
            Ok(PreparedFile {
                logical_file_index: segment_to_logical[i],
                archive_id: entry.archive_file_index,
                data: sliced_data,
                method: entry.compression,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    pb.finish_with_message("Input read complete");

    let range_settings = config
        .options
        .as_ref()
        .map(|options| RangeSettings {
            win_size: options.win_size,
            flags: options.flags,
            offset_table_size: options.offset_table_size,
            offset_tables: options.offset_tables,
            offset_contexts: options.offset_contexts,
            ref_length_table_size: options.ref_length_table_size,
            ref_length_tables: options.ref_length_tables,
            ref_offset_table_size: options.ref_offset_table_size,
            ref_offset_tables: options.ref_offset_tables,
            big_min_match: options.big_min_match,
        })
        .unwrap_or_default()
        .validate()?;
    let dz_inputs: Vec<Vec<u8>> = prepared_files
        .iter()
        .filter(|file| file.method == CompressionMethod::Dz)
        .map(|file| file.data.clone())
        .collect();
    let dz_options = config.options.clone().unwrap_or_default();
    let encoded_dz = if dz_inputs.is_empty() {
        None
    } else {
        info!("Compressing {} DZ chunks...", dz_inputs.len());
        Some(compress_archive(
            &dz_inputs,
            &DzEncoderOptions {
                settings: range_settings,
                use_combuf: dz_options.use_combuf,
                preprocess: dz_options.preprocess,
                trim_reference_factor: dz_options.trim_reference_factor,
                ..DzEncoderOptions::default()
            },
        )?)
    };

    let mut next_dz_index = 0usize;
    let dz_indices: Vec<Option<usize>> = prepared_files
        .iter()
        .map(|file| {
            if file.method == CompressionMethod::Dz {
                let index = next_dz_index;
                next_dz_index += 1;
                Some(index)
            } else {
                None
            }
        })
        .collect();
    let processed_files: Vec<_> = prepared_files
        .into_par_iter()
        .enumerate()
        .map(|(file_index, file)| {
            let original_len = file.data.len();
            let (flags, compressed) = if let Some(dz_index) = dz_indices[file_index] {
                let encoded = encoded_dz
                    .as_ref()
                    .and_then(|archive| archive.chunks.get(dz_index))
                    .ok_or_else(|| {
                        dzip_core::DzipError::InvalidDz(
                            "missing archive-scoped DZ result".to_string(),
                        )
                    })?
                    .clone();
                (CHUNK_DZ, encoded)
            } else {
                compress_data(&file.data, file.method)?
            };
            Ok((
                file.logical_file_index,
                file.archive_id,
                compressed,
                original_len,
                flags,
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    let has_dz = !dz_inputs.is_empty();
    let common_buffer = encoded_dz.and_then(|archive| archive.common_buffer);
    let total_chunks = processed_files.len() + usize::from(common_buffer.is_some());
    let num_chunks = u16::try_from(total_chunks).map_err(|_| {
        dzip_core::DzipError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "archive contains more than 65535 chunks",
        ))
    })?;

    // Calculate the final header after archive-scoped compression has decided
    // whether a CHUNK_COMBUF entry is required.
    let mut header_size = 9u64;
    header_size += all_strings
        .iter()
        .map(|value| value.len() as u64 + 1)
        .sum::<u64>();
    header_size += logical_files
        .iter()
        .map(|logical_file| 4 + (logical_file.segment_indices.len() as u64) * 2)
        .sum::<u64>();
    header_size += 4 + u64::from(num_chunks) * 16;
    header_size += config.archives[1..]
        .iter()
        .map(|value| value.len() as u64 + 1)
        .sum::<u64>();
    if has_dz {
        header_size += 10;
    }
    writers
        .get_mut(&0)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Volume 0 missing"))?
        .seek(SeekFrom::Start(header_size))?;

    // --- Process Files and Write Chunks ---
    let mut chunks = Vec::with_capacity(total_chunks);
    let mut logical_chunk_ids = vec![Vec::new(); logical_files.len()];
    let alignment = config.align.unwrap_or(0);

    // Sequential Write Phase
    info!("Writing compressed chunks to volumes...");
    for (logical_file_index, archive_id, compressed_data, original_len, flags) in
        processed_files.into_iter()
    {
        let chunk_id = chunks.len() as u16;

        let writer = writers.get_mut(&archive_id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Archive volume {} not found in config", archive_id),
            )
        })?;

        pad_writer_to_alignment(writer, alignment)?;
        let offset = writer.stream_position()? as u32;
        writer.write_all(&compressed_data)?;

        // dzip.exe 1.1.3 stores the uncompressed size in both length
        // fields for LZMA chunks. The physical stream length is recovered
        // from the next chunk offset (or EOF) when the archive is read.
        let compressed_length = if flags & CHUNK_LZMA != 0 {
            original_len
        } else {
            compressed_data.len()
        };
        chunks.push(Chunk {
            offset,
            compressed_length: compressed_length as u32,
            decompressed_length: original_len as u32,
            flags,
            file: archive_id,
        });

        logical_chunk_ids[logical_file_index].push(chunk_id);
    }

    if let Some(common_data) = common_buffer {
        let writer = writers
            .get_mut(&0)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Volume 0 missing"))?;
        pad_writer_to_alignment(writer, alignment)?;
        let offset = writer.stream_position()? as u32;
        writer.write_all(&common_data)?;
        chunks.push(Chunk {
            offset,
            compressed_length: common_data.len() as u32,
            decompressed_length: 0,
            flags: CHUNK_COMBUF,
            file: 0,
        });
    }

    let chunk_map: Vec<(u16, Vec<u16>)> = logical_chunk_ids
        .into_iter()
        .enumerate()
        .map(|(logical_index, chunk_ids)| (file_dir_ids[logical_index], chunk_ids))
        .collect();

    // --- Write Header ---
    info!("Writing header to Volume 0...");
    let main_writer = writers
        .get_mut(&0)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Volume 0 missing"))?;

    main_writer.seek(SeekFrom::Start(0))?;

    // We need DzipWriter
    struct SimpleWriter<'a, W: Write + Seek>(&'a mut W);
    impl<'a, W: Write + Seek> Write for SimpleWriter<'a, W> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }
    impl<'a, W: Write + Seek> Seek for SimpleWriter<'a, W> {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(pos)
        }
    }

    let mut dzip_writer = dzip_core::writer::DzipWriter::new(SimpleWriter(main_writer));

    // ... rest of header writing ...

    dzip_writer.write_archive_settings(&ArchiveSettings {
        header: 0x5A525444, // DTRZ
        num_user_files,
        num_directories,
        version: 0,
    })?;

    // ...

    dzip_writer.write_strings(&all_strings)?;
    dzip_writer.write_file_chunk_map(&chunk_map)?;

    // ...

    let num_archive_files = config.archives.len() as u16;

    dzip_writer.write_chunk_settings(&ChunkSettings {
        num_archive_files,
        num_chunks: chunks.len() as u16,
    })?;

    dzip_writer.write_chunks(&chunks)?;

    // Write Auxiliary File List
    if config.archives.len() > 1 {
        let aux_files = &config.archives[1..];
        dzip_writer.write_strings(aux_files)?;
    }

    if has_dz {
        dzip_writer.write_global_settings(&range_settings)?;
    }

    info!("Pack complete.");
    Ok(())
}
