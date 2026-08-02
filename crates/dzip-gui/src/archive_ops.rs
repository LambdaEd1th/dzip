use crate::model::{CompressionChoice, DraftFile, DzCompressionOptions, EntryView, LoadedArchive};
#[cfg(any(feature = "web", test))]
use crc32fast::Hasher;
use dzip::{
    Archive, ArchiveBuilder, Compression, DzOptions, EntryId, EntryOptions, MemoryVolumeSource,
    PackOptions, RangeSettings,
};
use std::collections::HashSet;
use std::io::Cursor;
#[cfg(any(feature = "web", test))]
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

pub fn open_archive(
    main_name: String,
    main_bytes: Vec<u8>,
    auxiliary_files: Vec<(String, Vec<u8>)>,
) -> Result<LoadedArchive, String> {
    let main_bytes: Arc<[u8]> = Arc::from(main_bytes);
    let mut auxiliary_files = auxiliary_files;
    auxiliary_files.sort_by_key(|item| item.0.to_lowercase());
    let auxiliary: Vec<(u16, Vec<u8>)> = auxiliary_files
        .into_iter()
        .enumerate()
        .map(|(index, (_, bytes))| ((index + 1) as u16, bytes))
        .collect();

    let archive = Archive::open_with_volumes(
        Cursor::new(main_bytes.clone()),
        MemoryVolumeSource::new(auxiliary.clone()),
    )
    .map_err(|error| format!("无法读取归档：{error}"))?;

    let chunks = archive.index().chunks();
    let mut dz_options = DzCompressionOptions {
        use_combuf: chunks
            .iter()
            .any(|chunk| chunk.flags & dzip::format::CHUNK_COMBUF != 0),
        ..DzCompressionOptions::default()
    };
    if let Some(settings) = archive.index().range_settings() {
        dz_options.combuf_static_tables =
            settings.flags & RangeSettings::USE_COMBUF_STATIC_TABLES != 0;
        dz_options.win_size = settings.win_size;
        dz_options.offset_table_size = settings.offset_table_size;
        dz_options.offset_tables = settings.offset_tables;
        dz_options.offset_contexts = settings.offset_contexts;
        dz_options.ref_length_table_size = settings.ref_length_table_size;
        dz_options.ref_length_tables = settings.ref_length_tables;
        dz_options.ref_offset_table_size = settings.ref_offset_table_size;
        dz_options.ref_offset_tables = settings.ref_offset_tables;
        dz_options.big_min_match = settings.big_min_match;
    }
    let mut unpacked_size = 0u64;
    let entries = archive
        .entries()
        .iter()
        .map(|entry| {
            let size = entry.decompressed_size();
            unpacked_size = unpacked_size.saturating_add(size);
            let packed_size = entry
                .chunk_ids()
                .iter()
                .filter_map(|id| chunks.get(*id as usize))
                .map(|chunk| u64::from(chunk.compressed_length))
                .sum();
            let path = entry.path().to_string_lossy().replace('\\', "/");
            let path_ref = Path::new(&path);
            EntryView {
                id: entry.id().0,
                name: path_ref
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone()),
                folder: path_ref
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "根目录".to_string()),
                path,
                size,
                packed_size,
                compression: entry.compression().to_string(),
                volume: entry.volume(),
                chunks: entry.chunk_ids().len(),
            }
        })
        .collect();

    let source_size = main_bytes.len() as u64
        + auxiliary
            .iter()
            .map(|(_, bytes)| bytes.len() as u64)
            .sum::<u64>();

    Ok(LoadedArchive {
        name: main_name,
        main_bytes,
        auxiliary: Arc::new(auxiliary),
        entries: Arc::new(entries),
        dz_options,
        source_size,
        unpacked_size,
        chunk_count: chunks.len(),
        volume_count: archive.index().volume_files().len() + 1,
    })
}

pub fn read_entries(
    archive: &LoadedArchive,
    entry_ids: &[usize],
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut reader = Archive::open_with_volumes(
        Cursor::new(archive.main_bytes.clone()),
        MemoryVolumeSource::new(archive.auxiliary.iter().cloned()),
    )
    .map_err(|error| format!("无法重新打开归档：{error}"))?;

    let mut files = Vec::with_capacity(entry_ids.len());
    for id in entry_ids {
        let entry = reader
            .entry(EntryId(*id))
            .ok_or_else(|| format!("归档中不存在条目 #{id}"))?;
        let path = entry.path().to_string_lossy().replace('\\', "/");
        let bytes = reader
            .read_entry(EntryId(*id))
            .map_err(|error| format!("解压 {path} 失败：{error}"))?;
        files.push((path, bytes));
    }
    Ok(files)
}

pub fn build_archive(
    files: &[DraftFile],
    archive_name: &str,
    alignment: u32,
    random_access: bool,
    dz_options: DzCompressionOptions,
) -> Result<Vec<u8>, String> {
    if files.is_empty() {
        return Err("请先添加至少一个文件".to_string());
    }

    let settings = RangeSettings {
        win_size: dz_options.win_size,
        flags: u8::from(dz_options.combuf_static_tables),
        offset_table_size: dz_options.offset_table_size,
        offset_tables: dz_options.offset_tables,
        offset_contexts: dz_options.offset_contexts,
        ref_length_table_size: dz_options.ref_length_table_size,
        ref_length_tables: dz_options.ref_length_tables,
        ref_offset_table_size: dz_options.ref_offset_table_size,
        ref_offset_tables: dz_options.ref_offset_tables,
        big_min_match: dz_options.big_min_match,
    }
    .validate()
    .map_err(|error| format!("DZ 参数无效：{error}"))?;

    let mut builder = ArchiveBuilder::with_options(PackOptions {
        volume_names: vec![normalise_archive_name(archive_name)],
        alignment,
        compatibility: dzip::Compatibility::Dzip,
        dz: DzOptions {
            settings,
            max_mem_usage: dz_options.max_mem_usage,
            use_combuf: dz_options.use_combuf,
            preprocess: dz_options.preprocess,
            trim_reference_factor: dz_options.trim_reference_factor,
            max_common_match: if dz_options.max_common_match == 0 {
                usize::MAX
            } else {
                dz_options.max_common_match as usize
            },
        },
    });

    let mut seen_paths = HashSet::with_capacity(files.len());
    for file in files {
        let path = file.path.trim();
        if path.is_empty() {
            return Err("归档内路径不能为空".to_string());
        }
        let comparison_key = path.replace('\\', "/").to_ascii_lowercase();
        if !seen_paths.insert(comparison_key) {
            return Err(format!("归档内路径重复：{path}"));
        }
        let compression = match file.compression {
            CompressionChoice::Dz => Compression::Dz,
            CompressionChoice::Zlib => Compression::Zlib,
            CompressionChoice::Bzip => Compression::Bzip,
            CompressionChoice::Lzma => Compression::Lzma,
            CompressionChoice::Copy => Compression::Copy,
            CompressionChoice::Zero => Compression::Zero,
        };
        if compression == Compression::Zero && file.bytes.iter().any(|byte| *byte != 0) {
            return Err(format!("{path} 不是全零文件，不能使用零填充算法"));
        }
        builder
            .add_bytes(
                path,
                file.bytes.as_ref().to_vec(),
                EntryOptions::new()
                    .compression(compression)
                    .random_access(random_access),
            )
            .map_err(|error| format!("无法添加 {path}：{error}"))?;
    }

    let mut cursor = Cursor::new(Vec::new());
    builder
        .write_to(&mut cursor)
        .map_err(|error| format!("创建归档失败：{error}"))?;
    Ok(cursor.into_inner())
}

pub fn normalise_archive_name(value: &str) -> String {
    let trimmed = value.trim();
    let name = if trimmed.is_empty() {
        "archive"
    } else {
        trimmed
    };
    if name.to_ascii_lowercase().ends_with(".dz") {
        name.to_string()
    } else {
        format!("{name}.dz")
    }
}

#[cfg(any(feature = "web", test))]
pub fn make_store_zip(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    if files.len() > u16::MAX as usize {
        return Err("网页端一次最多导出 65535 个文件".to_string());
    }

    struct CentralEntry {
        name: Vec<u8>,
        crc: u32,
        size: u32,
        offset: u32,
    }

    let mut output = Vec::new();
    let mut central = Vec::with_capacity(files.len());
    for (path, bytes) in files {
        let name = path.replace('\\', "/").into_bytes();
        let size =
            u32::try_from(bytes.len()).map_err(|_| format!("{path} 超过网页 ZIP 的 4 GB 限制"))?;
        let offset = u32::try_from(output.len()).map_err(|_| "导出内容超过 4 GB".to_string())?;
        let mut hasher = Hasher::new();
        hasher.update(bytes);
        let crc = hasher.finalize();

        write_u32(&mut output, 0x0403_4b50)?;
        write_u16(&mut output, 20)?;
        write_u16(&mut output, 0x0800)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u32(&mut output, crc)?;
        write_u32(&mut output, size)?;
        write_u32(&mut output, size)?;
        write_u16(&mut output, name.len() as u16)?;
        write_u16(&mut output, 0)?;
        output.extend_from_slice(&name);
        output.extend_from_slice(bytes);
        central.push(CentralEntry {
            name,
            crc,
            size,
            offset,
        });
    }

    let central_offset =
        u32::try_from(output.len()).map_err(|_| "导出内容超过 4 GB".to_string())?;
    for entry in &central {
        write_u32(&mut output, 0x0201_4b50)?;
        write_u16(&mut output, 20)?;
        write_u16(&mut output, 20)?;
        write_u16(&mut output, 0x0800)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u32(&mut output, entry.crc)?;
        write_u32(&mut output, entry.size)?;
        write_u32(&mut output, entry.size)?;
        write_u16(&mut output, entry.name.len() as u16)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u32(&mut output, 0)?;
        write_u32(&mut output, entry.offset)?;
        output.extend_from_slice(&entry.name);
    }
    let central_size = u32::try_from(output.len())
        .map_err(|_| "导出内容超过 4 GB".to_string())?
        .saturating_sub(central_offset);
    write_u32(&mut output, 0x0605_4b50)?;
    write_u16(&mut output, 0)?;
    write_u16(&mut output, 0)?;
    write_u16(&mut output, central.len() as u16)?;
    write_u16(&mut output, central.len() as u16)?;
    write_u32(&mut output, central_size)?;
    write_u32(&mut output, central_offset)?;
    write_u16(&mut output, 0)?;
    Ok(output)
}

#[cfg(any(feature = "web", test))]
fn write_u16(output: &mut Vec<u8>, value: u16) -> Result<(), String> {
    output
        .write_all(&value.to_le_bytes())
        .map_err(|error| error.to_string())
}

#[cfg(any(feature = "web", test))]
fn write_u32(output: &mut Vec<u8>, value: u32) -> Result<(), String> {
    output
        .write_all(&value.to_le_bytes())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_name_gets_extension_once() {
        assert_eq!(normalise_archive_name("demo"), "demo.dz");
        assert_eq!(normalise_archive_name("DEMO.DZ"), "DEMO.DZ");
        assert_eq!(normalise_archive_name("  "), "archive.dz");
    }

    #[test]
    fn store_zip_has_local_and_end_records() {
        let bytes = make_store_zip(&[("hello.txt".into(), b"hello".to_vec())]).unwrap();
        assert!(bytes.starts_with(&0x0403_4b50u32.to_le_bytes()));
        assert!(
            bytes
                .windows(4)
                .any(|part| part == 0x0605_4b50u32.to_le_bytes())
        );
    }

    #[test]
    fn gui_build_open_and_extract_roundtrip() {
        let draft = vec![
            DraftFile {
                id: 1,
                path: "Data/hello.txt".into(),
                bytes: Arc::from(b"hello from the GUI".as_slice()),
                compression: CompressionChoice::Zlib,
            },
            DraftFile {
                id: 2,
                path: "Data/already-packed.bin".into(),
                bytes: Arc::from(b"stored without compression".as_slice()),
                compression: CompressionChoice::Copy,
            },
        ];
        let bytes = build_archive(
            &draft,
            "gui-test.dz",
            0,
            false,
            DzCompressionOptions::default(),
        )
        .unwrap();
        let archive = open_archive("gui-test.dz".into(), bytes, Vec::new()).unwrap();
        assert_eq!(archive.entries.len(), 2);
        assert_eq!(archive.entries[0].path, "Data/hello.txt");
        assert_eq!(
            CompressionChoice::from_archive_label(&archive.entries[0].compression),
            CompressionChoice::Zlib
        );
        assert_eq!(
            CompressionChoice::from_archive_label(&archive.entries[1].compression),
            CompressionChoice::Copy
        );
        let extracted =
            read_entries(&archive, &[archive.entries[0].id, archive.entries[1].id]).unwrap();
        assert_eq!(extracted[0].1, b"hello from the GUI");
        assert_eq!(extracted[1].1, b"stored without compression");
    }

    #[test]
    fn gui_applies_and_recovers_dz_range_parameters() {
        let draft = vec![DraftFile {
            id: 1,
            path: "Data/repeated.bin".into(),
            bytes: Arc::from(b"native dz payload native dz payload".repeat(32)),
            compression: CompressionChoice::Dz,
        }];
        let options = DzCompressionOptions {
            combuf_static_tables: false,
            win_size: 12,
            offset_table_size: 7,
            offset_tables: 2,
            offset_contexts: 4,
            ref_length_table_size: 6,
            ref_length_tables: 2,
            ref_offset_table_size: 5,
            ref_offset_tables: 2,
            big_min_match: 11,
            ..DzCompressionOptions::default()
        };

        let bytes = build_archive(&draft, "dz-options.dz", 0, false, options).unwrap();
        let archive = open_archive("dz-options.dz".into(), bytes, Vec::new()).unwrap();

        assert!(!archive.dz_options.combuf_static_tables);
        assert_eq!(archive.dz_options.win_size, 12);
        assert_eq!(archive.dz_options.offset_table_size, 7);
        assert_eq!(archive.dz_options.offset_tables, 2);
        assert_eq!(archive.dz_options.offset_contexts, 4);
        assert_eq!(archive.dz_options.ref_length_table_size, 6);
        assert_eq!(archive.dz_options.ref_length_tables, 2);
        assert_eq!(archive.dz_options.ref_offset_table_size, 5);
        assert_eq!(archive.dz_options.ref_offset_tables, 2);
        assert_eq!(archive.dz_options.big_min_match, 11);
    }

    #[test]
    fn gui_passes_dz_memory_limit_to_encoder() {
        let draft = vec![DraftFile {
            id: 1,
            path: "Data/payload.bin".into(),
            bytes: Arc::from(b"payload".repeat(64)),
            compression: CompressionChoice::Dz,
        }];
        let options = DzCompressionOptions {
            max_mem_usage: 0,
            ..DzCompressionOptions::default()
        };

        let error = build_archive(&draft, "memory-limit.dz", 0, false, options).unwrap_err();
        assert!(error.contains("max_mem_usage"));
    }
}
