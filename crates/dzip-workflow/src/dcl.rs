use crate::error::{Result, WorkflowError};
use crate::model::{BuildEntry, BuildPlan, DzConfig};
use dzip::{ChunkEncoding, DzOptions, RangeSettings};
use dzip_dcl::{DclConfig, FileEntry};
use std::path::{Path, PathBuf};

pub fn build_plan_from_dcl(config: DclConfig) -> Result<BuildPlan> {
    if config.archives.is_empty() {
        return Err(WorkflowError::invalid(
            "configuration contains no output volumes",
        ));
    }
    let volume_count = config.archives.len();
    let options = config.options;
    let dz_options = DzOptions {
        settings: RangeSettings {
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
        }
        .validate()?,
        max_mem_usage: options.max_mem_usage,
        use_combuf: options.use_combuf,
        preprocess: options.preprocess,
        trim_reference_factor: options.trim_reference_factor,
        ..DzOptions::default()
    };

    let mut entries = Vec::with_capacity(config.files.len());
    for entry in config.files {
        let Some(compression) = entry.selected_compression() else {
            continue;
        };
        let bytes = read_source(&entry, &config.dcl_search_dirs)?;
        let (start, end) = entry
            .byte_range(bytes.len())
            .map_err(|error| WorkflowError::invalid(error.to_string()))?;
        let requested_volume = entry.requested_archive_file_index();
        let volume = if requested_volume >= 0 && (requested_volume as usize) < volume_count {
            requested_volume as u16
        } else {
            0
        };
        let mut encoding = ChunkEncoding::from_packer_flags(entry.dcl_flags())?;
        encoding.compression = compression;
        entries.push(BuildEntry {
            path: entry.path.to_string_lossy().into_owned(),
            bytes: bytes[start..end].to_vec(),
            encoding,
            raw_flags: Some(entry.dcl_flags()),
            volume,
        });
    }

    Ok(BuildPlan {
        archive_name: config.archives[0].clone(),
        volume_names: config.archives,
        alignment: config.align,
        dz_options: DzConfig::from_options(&dz_options),
        entries,
    })
}

fn read_source(entry: &FileEntry, search_dirs: &[PathBuf]) -> Result<Vec<u8>> {
    if entry.path.is_absolute() || has_windows_drive_prefix(&entry.path) {
        return read_path(&entry.path);
    }
    let mut last_error = None;
    for directory in search_dirs {
        let source = directory.join(&entry.path);
        match std::fs::read(&source) {
            Ok(bytes) => return Ok(bytes),
            Err(error) => last_error = Some((source, error)),
        }
    }
    let (source, error) = last_error.unwrap_or_else(|| {
        (
            entry.path.clone(),
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "DCL contains no basedir search roots",
            ),
        )
    });
    Err(WorkflowError::Io(std::io::Error::new(
        error.kind(),
        format!("failed to read {}: {error}", source.display()),
    )))
}

fn read_path(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|error| {
        WorkflowError::Io(std::io::Error::new(
            error.kind(),
            format!("failed to read {}: {error}", path.display()),
        ))
    })
}

fn has_windows_drive_prefix(path: &Path) -> bool {
    path.to_string_lossy().as_bytes().get(1) == Some(&b':')
}
