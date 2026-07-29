use anyhow::{Context, Result, bail};
use dzip_core::CompressionMethod;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DzipConfig {
    pub archives: Vec<String>,
    #[serde(default = "default_base_dir")]
    pub base_dir: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<u32>,
    pub files: Vec<FileEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<GlobalOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: PathBuf,
    pub archive_file_index: u16,
    pub compression: CompressionMethod,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub modifiers: String, // e.g., "to 25%"
    #[serde(skip)]
    pub source_base_dir: Option<PathBuf>,
}

impl FileEntry {
    pub fn byte_range(&self, total_len: usize) -> Result<(usize, usize)> {
        let (from_percent, to_percent) = parse_file_modifiers(&self.modifiers)?;
        let start_percent = usize::from(from_percent.unwrap_or(0));
        let end_percent = usize::from(to_percent.unwrap_or(100));

        if start_percent > end_percent {
            bail!(
                "Invalid file modifiers '{}' for {}: from {}% is after to {}%",
                self.modifiers,
                self.path.display(),
                start_percent,
                end_percent
            );
        }

        let start = total_len.saturating_mul(start_percent) / 100;
        let end = total_len.saturating_mul(end_percent) / 100;
        Ok((start.min(total_len), end.min(total_len)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalOptions {
    pub method: String,
    #[serde(alias = "isnotdefault")]
    pub is_not_default: bool,
    pub max_mem_usage: i32,
    pub use_combuf: bool,
    pub preprocess: bool,
    pub trim_reference_factor: i32,
    #[serde(alias = "WinSize")]
    pub win_size: u8,
    #[serde(alias = "Flags")]
    pub flags: u8,
    #[serde(alias = "OffsetTableSize")]
    pub offset_table_size: u8,
    #[serde(alias = "OffsetTables")]
    pub offset_tables: u8,
    #[serde(alias = "OffsetContexts")]
    pub offset_contexts: u8,
    #[serde(alias = "RefLengthTableSize")]
    pub ref_length_table_size: u8,
    #[serde(alias = "RefLengthTables")]
    pub ref_length_tables: u8,
    #[serde(alias = "RefOffsetTableSize")]
    pub ref_offset_table_size: u8,
    #[serde(alias = "RefOffsetTables")]
    pub ref_offset_tables: u8,
    #[serde(alias = "BigMinMatch")]
    pub big_min_match: u8,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        Self {
            method: "dz".to_string(),
            is_not_default: false,
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

fn default_base_dir() -> PathBuf {
    PathBuf::from(".")
}

pub fn parse_config(path: &Path) -> Result<DzipConfig> {
    let content = std::fs::read_to_string(path)?;

    if path.extension().is_some_and(|ext| ext == "toml") {
        return Ok(toml::from_str(&content)?);
    }

    let mut config = DzipConfig {
        archives: Vec::new(),
        base_dir: default_base_dir(),
        align: None,
        files: Vec::new(),
        options: Some(GlobalOptions::default()),
    };

    let mut include_stack = HashSet::new();
    let initial_base_dir = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    parse_legacy_config_file(
        path,
        &mut config,
        &mut include_stack,
        initial_base_dir,
        true,
    )?;

    Ok(config)
}

fn parse_legacy_config_file(
    path: &Path,
    config: &mut DzipConfig,
    include_stack: &mut HashSet<PathBuf>,
    mut current_base_dir: PathBuf,
    is_root: bool,
) -> Result<PathBuf> {
    let canonical_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !include_stack.insert(canonical_path.clone()) {
        bail!("Recursive master include detected at {}", path.display());
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config {}", path.display()))?;
    let config_dir = path.parent().unwrap_or_else(|| Path::new("."));

    for raw_line in content.lines() {
        let line = strip_comments(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0].to_ascii_lowercase().as_str() {
            "archive" => {
                if let Some(name) = parts.get(1) {
                    config.archives.push(name.replace('\\', "/"));
                }
            }
            "align" => {
                let value = parse_value::<u32>(parts.get(1), "align")?;
                config.align = Some(value);
            }
            "master" => {
                let include_name = parts.get(1).context("Missing master file path")?;
                let include_path = resolve_config_file_path(config_dir, include_name)?;
                current_base_dir = parse_legacy_config_file(
                    &include_path,
                    config,
                    include_stack,
                    current_base_dir,
                    false,
                )?;
            }
            "basedir" => {
                let raw_base_dir = parts.get(1).context("Missing basedir path")?;
                let relative_base_dir = dzip_core::path::resolve_relative_path(raw_base_dir)
                    .context("Failed to resolve base dir")?;
                current_base_dir = config_dir.join(&relative_base_dir);
                if is_root {
                    config.base_dir = relative_base_dir;
                }
            }
            "file" => {
                if parts.len() < 4 {
                    bail!(
                        "Invalid file directive '{}': expected file <path> <index> <algo>",
                        line
                    );
                }

                let archive_path = dzip_core::path::resolve_relative_path(parts[1])
                    .context("Failed to resolve file path")?;
                let archive_file_index = parts[2]
                    .parse::<u16>()
                    .context("Failed to parse archive index")?;
                let compression = parts[3]
                    .parse::<CompressionMethod>()
                    .context("Failed to parse compression method")?;

                let modifiers = if parts.len() > 4 {
                    let joined = parts[4..].join(" ");
                    parse_file_modifiers(&joined)?;
                    joined
                } else {
                    String::new()
                };

                config.files.push(FileEntry {
                    path: archive_path,
                    archive_file_index,
                    compression,
                    modifiers,
                    source_base_dir: Some(current_base_dir.clone()),
                });
            }
            "options" => {
                if let Some(method) = parts.get(1) {
                    current_options(config).method = (*method).to_string();
                }
            }
            "isnotdefault" => {
                current_options(config).is_not_default =
                    parse_bool_flag(parts.get(1), "isnotdefault")?;
            }
            "max_mem_usage" => {
                current_options(config).max_mem_usage = parse_value(parts.get(1), "max_mem_usage")?;
            }
            "use_combuf" => {
                current_options(config).use_combuf = parse_bool_flag(parts.get(1), "use_combuf")?;
            }
            "preprocess" => {
                current_options(config).preprocess = parse_bool_flag(parts.get(1), "preprocess")?;
            }
            "trim_reference_factor" => {
                current_options(config).trim_reference_factor =
                    parse_value(parts.get(1), "trim_reference_factor")?;
            }
            "winsize" => {
                current_options(config).win_size = parse_value(parts.get(1), "WinSize")?;
            }
            "flags" => {
                current_options(config).flags = parse_value(parts.get(1), "Flags")?;
            }
            key => match key {
                "offsettablesize" => {
                    current_options(config).offset_table_size =
                        parse_value(parts.get(1), "OffsetTableSize")?;
                }
                "offsettables" => {
                    current_options(config).offset_tables =
                        parse_value(parts.get(1), "OffsetTables")?;
                }
                "offsetcontexts" => {
                    current_options(config).offset_contexts =
                        parse_value(parts.get(1), "OffsetContexts")?;
                }
                "reflengthtablesize" => {
                    current_options(config).ref_length_table_size =
                        parse_value(parts.get(1), "RefLengthTableSize")?;
                }
                "reflengthtables" => {
                    current_options(config).ref_length_tables =
                        parse_value(parts.get(1), "RefLengthTables")?;
                }
                "refoffsettablesize" => {
                    current_options(config).ref_offset_table_size =
                        parse_value(parts.get(1), "RefOffsetTableSize")?;
                }
                "refoffsettables" => {
                    current_options(config).ref_offset_tables =
                        parse_value(parts.get(1), "RefOffsetTables")?;
                }
                "bigminmatch" => {
                    current_options(config).big_min_match =
                        parse_value(parts.get(1), "BigMinMatch")?;
                }
                _ => {}
            },
        }
    }

    include_stack.remove(&canonical_path);
    Ok(current_base_dir)
}

fn current_options(config: &mut DzipConfig) -> &mut GlobalOptions {
    config.options.get_or_insert_with(GlobalOptions::default)
}

fn parse_value<T>(value: Option<&&str>, key: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let raw_value = value.with_context(|| format!("Missing value for {}", key))?;
    raw_value.parse::<T>().map_err(|error| {
        anyhow::anyhow!("Failed to parse {} value '{}': {}", key, raw_value, error)
    })
}

fn parse_bool_flag(value: Option<&&str>, key: &str) -> Result<bool> {
    let raw_value = value.with_context(|| format!("Missing value for {}", key))?;
    match raw_value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!(
            "Failed to parse {} value '{}': expected boolean/0/1",
            key,
            raw_value
        ),
    }
}

fn parse_file_modifiers(modifiers: &str) -> Result<(Option<u8>, Option<u8>)> {
    let mut from_percent = None;
    let mut to_percent = None;
    let mut tokens = modifiers.split_whitespace();

    while let Some(token) = tokens.next() {
        match token.to_ascii_lowercase().as_str() {
            "from" => {
                let value = tokens.next().with_context(|| {
                    format!(
                        "Missing percentage after 'from' in modifiers '{}'",
                        modifiers
                    )
                })?;
                if from_percent
                    .replace(parse_percentage(value, modifiers)?)
                    .is_some()
                {
                    bail!("Duplicate 'from' modifier in '{}'", modifiers);
                }
            }
            "to" => {
                let value = tokens.next().with_context(|| {
                    format!("Missing percentage after 'to' in modifiers '{}'", modifiers)
                })?;
                if to_percent
                    .replace(parse_percentage(value, modifiers)?)
                    .is_some()
                {
                    bail!("Duplicate 'to' modifier in '{}'", modifiers);
                }
            }
            _ => bail!("Unsupported file modifier '{}' in '{}'", token, modifiers),
        }
    }

    Ok((from_percent, to_percent))
}

fn parse_percentage(value: &str, modifiers: &str) -> Result<u8> {
    let trimmed = value.trim_end_matches('%');
    let percent = trimmed.parse::<u8>().with_context(|| {
        format!(
            "Invalid percentage '{}' in modifiers '{}'",
            value, modifiers
        )
    })?;
    if percent > 100 {
        bail!("Percentage '{}' in '{}' exceeds 100", value, modifiers);
    }
    Ok(percent)
}

fn resolve_config_file_path(config_dir: &Path, raw_path: &str) -> Result<PathBuf> {
    let relative_path = dzip_core::path::resolve_relative_path(raw_path)
        .with_context(|| format!("Failed to resolve config path '{}'", raw_path))?;
    Ok(config_dir.join(relative_path))
}

fn strip_comments(line: &str) -> &str {
    line.split('#').next().unwrap_or("")
}
