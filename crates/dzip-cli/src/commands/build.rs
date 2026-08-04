use crate::config;
use dzip::{ArchiveBuilder, DzOptions, EntryOptions, PackOptions, RangeSettings, Result};
use log::{debug, info};
use std::path::Path;

pub fn build_from_config(input_path: &str, output_dir: &str) -> Result<()> {
    build_from_config_with_commands(input_path, output_dir, &[])
}

pub fn build_from_config_with_commands(
    input_path: &str,
    output_dir: &str,
    commands: &[String],
) -> Result<()> {
    let config_path = Path::new(input_path);
    info!("Parsing config file: {}", config_path.display());
    let parsed = if commands.is_empty() {
        config::parse_config(config_path)
    } else {
        config::parse_config_with_commands(config_path, commands)
    };
    let mut config =
        parsed.map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if !config.is_legacy_dcl
        && config.base_dir == Path::new(".")
        && let Some(parent) = config_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
    {
        config.base_dir = parent.to_path_buf();
    }
    write_config(config, Path::new(output_dir))
}

fn write_config(config: config::DzipConfig, output_dir: &Path) -> Result<()> {
    let builder = archive_builder(config)?;
    let report = builder.write_to_directory(output_dir)?;
    info!(
        "Packed {} files into {} chunks across {} volume(s)",
        report.entries, report.chunks, report.volumes
    );
    Ok(())
}

fn archive_builder(config: config::DzipConfig) -> Result<ArchiveBuilder> {
    if config.archives.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "configuration contains no output volumes",
        )
        .into());
    }

    let volume_count = config.archives.len();
    let is_legacy_dcl = config.is_legacy_dcl;
    let dcl_search_dirs = config.dcl_search_dirs.clone();
    let default_base_dir = config.base_dir.clone();
    let dz = config.options.clone().unwrap_or_default();
    let settings = RangeSettings {
        win_size: dz.win_size,
        flags: dz.flags,
        offset_table_size: dz.offset_table_size,
        offset_tables: dz.offset_tables,
        offset_contexts: dz.offset_contexts,
        ref_length_table_size: dz.ref_length_table_size,
        ref_length_tables: dz.ref_length_tables,
        ref_offset_table_size: dz.ref_offset_table_size,
        ref_offset_tables: dz.ref_offset_tables,
        big_min_match: dz.big_min_match,
    }
    .validate()?;
    let mut builder = ArchiveBuilder::with_options(PackOptions {
        volume_names: config.archives,
        alignment: config.align.unwrap_or(0),
        dz: DzOptions {
            settings,
            max_mem_usage: dz.max_mem_usage,
            use_combuf: dz.use_combuf,
            preprocess: dz.preprocess,
            trim_reference_factor: dz.trim_reference_factor,
            ..DzOptions::default()
        },
    });

    for entry in config.files {
        let Some(compression) = entry.selected_compression() else {
            debug!(
                "Ignoring {} because no registered dzip.exe coder matches flags {:#x}",
                entry.path.display(),
                entry.dcl_flags().unwrap_or(0)
            );
            continue;
        };
        let (source, bytes) =
            read_source(&entry, is_legacy_dcl, &dcl_search_dirs, &default_base_dir)?;
        let (start, end) = entry.byte_range(bytes.len()).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
        })?;
        let requested_volume = entry.requested_archive_file_index();
        let volume = if requested_volume >= 0 && (requested_volume as usize) < volume_count {
            requested_volume as u16
        } else if is_legacy_dcl {
            // dzip.exe silently redirects an out-of-range positive index to
            // the primary archive. Negative values are undefined in the
            // original; using volume zero keeps the compatibility frontend
            // memory-safe.
            0
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "archive volume index {} is outside 0..{}",
                    requested_volume, volume_count
                ),
            )
            .into());
        };
        let mut options = EntryOptions::new().compression(compression).volume(volume);
        if let Some(flags) = entry.dcl_flags() {
            options = options.raw_flags(flags);
        }
        builder.add_bytes(&entry.path, bytes[start..end].to_vec(), options)?;
        debug!("Added {}", source.display());
    }

    Ok(builder)
}

fn read_source(
    entry: &config::FileEntry,
    is_legacy_dcl: bool,
    dcl_search_dirs: &[std::path::PathBuf],
    default_base_dir: &Path,
) -> Result<(std::path::PathBuf, Vec<u8>)> {
    if !is_legacy_dcl {
        let source = entry
            .source_base_dir
            .as_deref()
            .unwrap_or(default_base_dir)
            .join(&entry.path);
        debug!("Reading {}", source.display());
        let bytes = read_path(&source)?;
        return Ok((source, bytes));
    }

    if entry.path.is_absolute() || has_windows_drive_prefix(&entry.path) {
        debug!("Reading {}", entry.path.display());
        return read_path(&entry.path).map(|bytes| (entry.path.clone(), bytes));
    }

    let mut last_error = None;
    for base_dir in dcl_search_dirs {
        let source = base_dir.join(&entry.path);
        debug!("Reading {}", source.display());
        match std::fs::read(&source) {
            Ok(bytes) => return Ok((source, bytes)),
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
    Err(dzip::DzipError::Io(std::io::Error::new(
        error.kind(),
        format!("failed to read {}: {error}", source.display()),
    )))
}

fn read_path(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|error| {
        dzip::DzipError::Io(std::io::Error::new(
            error.kind(),
            format!("failed to read {}: {error}", path.display()),
        ))
    })
}

fn has_windows_drive_prefix(path: &Path) -> bool {
    path.to_string_lossy().as_bytes().get(1) == Some(&b':')
}
