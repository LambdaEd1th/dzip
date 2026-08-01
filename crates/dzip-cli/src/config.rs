use anyhow::{Context, Result, bail};
use dzip::format::{
    CHUNK_BZIP, CHUNK_COMBUF, CHUNK_COPYCOMP, CHUNK_DZ, CHUNK_JPEG, CHUNK_LZMA, CHUNK_MP3,
    CHUNK_RANDOMACCESS, CHUNK_ZERO, CHUNK_ZLIB,
};
use dzip::{ChunkEncoding, Compression};
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
    #[serde(skip)]
    pub(crate) dcl_search_dirs: Vec<PathBuf>,
    #[serde(skip)]
    pub(crate) is_legacy_dcl: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: PathBuf,
    pub archive_file_index: u16,
    pub compression: Compression,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub modifiers: String, // e.g., "to 25%"
    #[serde(skip)]
    pub source_base_dir: Option<PathBuf>,
    #[serde(skip)]
    pub(crate) dcl_range: Option<DclRange>,
    #[serde(skip)]
    pub(crate) dcl_flags: Option<u16>,
    #[serde(skip)]
    pub(crate) dcl_archive_file_index: Option<i32>,
}

impl FileEntry {
    pub fn byte_range(&self, total_len: usize) -> Result<(usize, usize)> {
        if let Some(range) = self.dcl_range {
            return range.resolve(total_len, &self.path);
        }

        let (from, to) = parse_file_modifiers(&self.modifiers)?;
        let start = from
            .map(|boundary| boundary.resolve(total_len))
            .unwrap_or(0);
        let end = to
            .map(|boundary| boundary.resolve(total_len))
            .unwrap_or(total_len);

        if start > end {
            bail!(
                "Invalid file modifiers '{}' for {}: from {} is after to {}",
                self.modifiers,
                self.path.display(),
                start,
                end
            );
        }

        Ok((start.min(total_len), end.min(total_len)))
    }

    pub const fn dcl_flags(&self) -> Option<u16> {
        self.dcl_flags
    }

    pub fn selected_compression(&self) -> Option<Compression> {
        self.dcl_flags
            .map_or(Some(self.compression), dcl_compression)
    }

    pub const fn requested_archive_file_index(&self) -> i32 {
        match self.dcl_archive_file_index {
            Some(index) => index,
            None => self.archive_file_index as i32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DclRange {
    from: i32,
    to: i32,
}

impl DclRange {
    fn resolve(self, total_len: usize, path: &Path) -> Result<(usize, usize)> {
        let resolve_boundary = |raw: i32| -> Result<usize> {
            if raw < 0 {
                let percent = u64::from(raw.unsigned_abs());
                let value = (total_len as u128)
                    .checked_mul(u128::from(percent))
                    .context("DCL percentage range overflow")?
                    / 100;
                usize::try_from(value).context("DCL percentage range exceeds platform limits")
            } else {
                Ok(raw as usize)
            }
        };

        let start = resolve_boundary(self.from)?;
        let end = resolve_boundary(self.to)?;
        if start > end || end > total_len {
            bail!(
                "DCL byte range {}..{} is outside {} ({} bytes)",
                start,
                end,
                path.display(),
                total_len
            );
        }
        Ok((start, end))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileBoundary {
    Bytes(usize),
    Percent(u8),
}

impl FileBoundary {
    fn resolve(self, total_len: usize) -> usize {
        match self {
            Self::Bytes(bytes) => bytes,
            Self::Percent(percent) => total_len.saturating_mul(usize::from(percent)) / 100,
        }
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

fn default_base_dir() -> PathBuf {
    PathBuf::from(".")
}

pub fn parse_config(path: &Path) -> Result<DzipConfig> {
    parse_config_with_commands(path, &[])
}

pub fn parse_config_with_commands(path: &Path, commands: &[String]) -> Result<DzipConfig> {
    if path.extension().is_some_and(|ext| ext == "toml") {
        let content = std::fs::read_to_string(path)?;
        return Ok(toml::from_str(&content)?);
    }

    let mut config = DzipConfig {
        archives: Vec::new(),
        base_dir: default_base_dir(),
        align: Some(0),
        files: Vec::new(),
        options: Some(GlobalOptions::default()),
        dcl_search_dirs: Vec::new(),
        is_legacy_dcl: true,
    };

    let root_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let root_dir = root_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut parser = DclParser {
        config: &mut config,
        root_dir,
        include_stack: HashSet::new(),
        options_selected: false,
    };
    parser.parse_file(&root_path, true)?;
    for command in commands {
        parser.parse_line(command)?;
    }

    Ok(config)
}

struct DclParser<'a> {
    config: &'a mut DzipConfig,
    root_dir: PathBuf,
    include_stack: HashSet<PathBuf>,
    options_selected: bool,
}

impl DclParser<'_> {
    fn parse_file(&mut self, path: &Path, is_root: bool) -> Result<()> {
        // dzip.exe resets alignment every time a master file is entered.
        self.config.align = Some(0);

        let canonical_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !self.include_stack.insert(canonical_path.clone()) {
            bail!("Recursive master include detected at {}", path.display());
        }

        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if !is_root => {
                log::warn!("Not a valid config file: {} ({error})", path.display());
                self.include_stack.remove(&canonical_path);
                return Ok(());
            }
            Err(error) => {
                self.include_stack.remove(&canonical_path);
                return Err(error)
                    .with_context(|| format!("Failed to read config {}", path.display()));
            }
        };

        for line in dcl_fgets_lines(&bytes) {
            self.parse_line(&line)?;
        }
        self.include_stack.remove(&canonical_path);
        Ok(())
    }

    fn parse_line(&mut self, line: &str) -> Result<()> {
        let parts = tokenize_dcl_line(line);
        if parts.len() < 2 {
            return Ok(());
        }

        match parts[0].to_ascii_lowercase().as_str() {
            "archive" => self.config.archives.push(parts[1].replace('\\', "/")),
            "align" => {
                let value = atoi_compat(&parts[1]);
                self.config.align = Some(u32::try_from(value).unwrap_or(0));
            }
            "master" => {
                let include = PathBuf::from(parts[1].replace('\\', "/"));
                let include = if include.is_absolute() {
                    include
                } else {
                    self.root_dir.join(include)
                };
                self.parse_file(&include, false)?;
            }
            "basedir" => {
                let directory = PathBuf::from(parts[1].replace('\\', "/"));
                let directory = if directory.is_absolute() {
                    directory
                } else {
                    self.root_dir.join(directory)
                };
                self.config.dcl_search_dirs.push(directory);
            }
            "file" if parts.len() >= 3 => self.parse_file_directive(&parts),
            "options" if parts[1].eq_ignore_ascii_case("dz") => {
                self.options_selected = true;
                current_options(self.config).method = "dz".to_string();
            }
            "options" => {}
            key if self.options_selected => {
                set_dcl_option(current_options(self.config), key, &parts[1]);
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_file_directive(&mut self, parts: &[String]) {
        let path = PathBuf::from(parts[1].replace('\\', "/"));
        let dcl_archive_file_index = atoi_compat(&parts[2]);
        let mut flags = 0u16;
        let mut range = DclRange { from: 0, to: -100 };
        let mut pending_boundary = None;

        for part in &parts[3..] {
            if let Some(is_from) = pending_boundary.take() {
                let boundary = strtoul_boundary_compat(part);
                if is_from {
                    range.from = boundary;
                } else {
                    range.to = boundary;
                }
                continue;
            }

            match part.to_ascii_lowercase().as_str() {
                "combuf" => flags |= CHUNK_COMBUF,
                "dz" => flags |= CHUNK_DZ,
                "zlib" => flags |= CHUNK_ZLIB,
                "bzip" => flags |= CHUNK_BZIP,
                "mp3" => flags |= CHUNK_MP3,
                "jpeg" => flags |= CHUNK_JPEG,
                "zero" => flags |= CHUNK_ZERO,
                "copy" => flags |= CHUNK_COPYCOMP,
                "lzma" => flags |= CHUNK_LZMA,
                "random-access" => flags |= CHUNK_RANDOMACCESS,
                "from" => pending_boundary = Some(true),
                "to" => pending_boundary = Some(false),
                _ => {}
            }
        }

        self.config.files.push(FileEntry {
            path,
            archive_file_index: u16::try_from(dcl_archive_file_index).unwrap_or(0),
            compression: dcl_compression(flags).unwrap_or(Compression::Dz),
            modifiers: String::new(),
            source_base_dir: None,
            dcl_range: Some(range),
            dcl_flags: Some(flags),
            dcl_archive_file_index: Some(dcl_archive_file_index),
        });
    }
}

fn current_options(config: &mut DzipConfig) -> &mut GlobalOptions {
    config.options.get_or_insert_with(GlobalOptions::default)
}

fn set_dcl_option(options: &mut GlobalOptions, key: &str, value: &str) {
    let value = atoi_compat(value);
    match key {
        "isnotdefault" => options.is_not_default = value != 0,
        "max_mem_usage" => options.max_mem_usage = value,
        "use_combuf" => options.use_combuf = value != 0,
        "preprocess" => options.preprocess = value != 0,
        "trim_reference_factor" => options.trim_reference_factor = value,
        "winsize" => options.win_size = value as u8,
        "flags" => options.flags = value as u8,
        "offsettablesize" => options.offset_table_size = value as u8,
        "offsettables" => options.offset_tables = value as u8,
        "offsetcontexts" => options.offset_contexts = value as u8,
        "reflengthtablesize" => options.ref_length_table_size = value as u8,
        "reflengthtables" => options.ref_length_tables = value as u8,
        "refoffsettablesize" => options.ref_offset_table_size = value as u8,
        "refoffsettables" => options.ref_offset_tables = value as u8,
        "bigminmatch" => options.big_min_match = value as u8,
        _ => {}
    }
}

fn dcl_compression(flags: u16) -> Option<Compression> {
    ChunkEncoding::from_flags(flags)
        .ok()
        .map(|encoding| encoding.compression)
}

fn atoi_compat(value: &str) -> i32 {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    let negative = match bytes.first() {
        Some(b'-') => {
            index = 1;
            true
        }
        Some(b'+') => {
            index = 1;
            false
        }
        _ => false,
    };
    let mut parsed = 0i64;
    let mut found_digit = false;
    while let Some(byte) = bytes.get(index) {
        if !byte.is_ascii_digit() {
            break;
        }
        found_digit = true;
        parsed = parsed
            .saturating_mul(10)
            .saturating_add(i64::from(byte - b'0'));
        index += 1;
    }
    if !found_digit {
        return 0;
    }
    if negative {
        parsed = -parsed;
    }
    parsed.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn strtoul_boundary_compat(value: &str) -> i32 {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    let negative = match bytes.first() {
        Some(b'-') => {
            index = 1;
            true
        }
        Some(b'+') => {
            index = 1;
            false
        }
        _ => false,
    };
    let mut parsed = 0u64;
    while let Some(byte) = bytes.get(index) {
        if !byte.is_ascii_digit() {
            break;
        }
        parsed = parsed
            .saturating_mul(10)
            .saturating_add(u64::from(byte - b'0'))
            .min(u64::from(u32::MAX));
        index += 1;
    }
    let mut raw = parsed as u32;
    if negative {
        raw = raw.wrapping_neg();
    }
    if bytes.get(index) == Some(&b'%') {
        raw = raw.wrapping_neg();
    }
    raw as i32
}

fn dcl_fgets_lines(bytes: &[u8]) -> Vec<String> {
    let mut result = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let maximum_end = cursor.saturating_add(255).min(bytes.len());
        let end = bytes[cursor..maximum_end]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(maximum_end, |offset| cursor + offset + 1);
        let mut chunk = &bytes[cursor..end];
        if chunk.ends_with(b"\n") {
            chunk = &chunk[..chunk.len() - 1];
        }
        if chunk.ends_with(b"\r") {
            chunk = &chunk[..chunk.len() - 1];
        }
        if let Some(nul) = chunk.iter().position(|byte| *byte == 0) {
            chunk = &chunk[..nul];
        }
        result.push(String::from_utf8_lossy(chunk).into_owned());
        cursor = end;
    }
    result
}

fn tokenize_dcl_line(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut token_started = false;
    let mut characters = line.chars().peekable();

    while let Some(character) = characters.next() {
        if in_quotes && character == '\\' && matches!(characters.peek(), Some('"' | '\\')) {
            current.push(characters.next().expect("peeked character exists"));
            token_started = true;
            continue;
        }
        if character == '"' {
            in_quotes = !in_quotes;
            token_started = true;
            continue;
        }
        if !in_quotes && matches!(character, ' ' | '\t' | '\n') {
            if token_started {
                tokens.push(std::mem::take(&mut current));
                token_started = false;
            }
            continue;
        }
        current.push(character);
        token_started = true;
    }
    if token_started {
        tokens.push(current);
    }
    tokens
}

fn parse_file_modifiers(modifiers: &str) -> Result<(Option<FileBoundary>, Option<FileBoundary>)> {
    let mut from = None;
    let mut to = None;
    let mut tokens = modifiers.split_whitespace();

    while let Some(token) = tokens.next() {
        match token.to_ascii_lowercase().as_str() {
            "from" => {
                let value = tokens.next().with_context(|| {
                    format!("Missing value after 'from' in modifiers '{}'", modifiers)
                })?;
                if from
                    .replace(parse_file_boundary(value, modifiers)?)
                    .is_some()
                {
                    bail!("Duplicate 'from' modifier in '{}'", modifiers);
                }
            }
            "to" => {
                let value = tokens.next().with_context(|| {
                    format!("Missing value after 'to' in modifiers '{}'", modifiers)
                })?;
                if to.replace(parse_file_boundary(value, modifiers)?).is_some() {
                    bail!("Duplicate 'to' modifier in '{}'", modifiers);
                }
            }
            _ => bail!("Unsupported file modifier '{}' in '{}'", token, modifiers),
        }
    }

    Ok((from, to))
}

fn parse_file_boundary(value: &str, modifiers: &str) -> Result<FileBoundary> {
    if let Some(trimmed) = value.strip_suffix('%') {
        let percent = trimmed.parse::<u8>().with_context(|| {
            format!(
                "Invalid percentage '{}' in modifiers '{}'",
                value, modifiers
            )
        })?;
        if percent > 100 {
            bail!("Percentage '{}' in '{}' exceeds 100", value, modifiers);
        }
        Ok(FileBoundary::Percent(percent))
    } else {
        let bytes = value.parse::<usize>().with_context(|| {
            format!(
                "Invalid byte offset '{}' in modifiers '{}'",
                value, modifiers
            )
        })?;
        Ok(FileBoundary::Bytes(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn entry(modifiers: &str) -> FileEntry {
        FileEntry {
            path: PathBuf::from("payload.bin"),
            archive_file_index: 0,
            compression: Compression::Copy,
            modifiers: modifiers.to_string(),
            source_base_dir: None,
            dcl_range: None,
            dcl_flags: None,
            dcl_archive_file_index: None,
        }
    }

    #[test]
    fn bare_slice_values_are_absolute_byte_offsets() {
        assert_eq!(entry("from 25 to 75").byte_range(200).unwrap(), (25, 75));
    }

    #[test]
    fn percent_suffix_selects_percentage_offsets() {
        assert_eq!(entry("from 25% to 75%").byte_range(200).unwrap(), (50, 150));
    }

    #[test]
    fn byte_and_percentage_offsets_can_be_mixed() {
        assert_eq!(entry("from 25 to 75%").byte_range(200).unwrap(), (25, 150));
    }

    #[test]
    fn dcl_tokenizer_supports_original_quoted_and_escaped_tokens() {
        assert_eq!(
            tokenize_dcl_line(r#"file "folder/a b\"c\\d.bin" 1 copy"#),
            ["file", "folder/a b\"c\\d.bin", "1", "copy"]
        );
    }

    #[test]
    fn dcl_file_flags_ranges_and_atoi_semantics_match_original() {
        let root = unique_temp_dir("flags");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("flags.dcl");
        std::fs::write(
            &config_path,
            r#"archive "output archive.dz"
basedir "assets root"
file "payload file.bin" 7 zlib bzip random-access jpeg from 25% # to 75%
options dz
use_combuf yes
use_combuf 2trailing
WinSize 260
"#,
        )
        .unwrap();

        let config = parse_config(&config_path).unwrap();
        assert_eq!(config.archives, ["output archive.dz"]);
        assert_eq!(config.dcl_search_dirs, [root.join("assets root")]);
        let file = &config.files[0];
        assert_eq!(file.path, Path::new("payload file.bin"));
        assert_eq!(file.archive_file_index, 7);
        assert_eq!(file.selected_compression(), Some(Compression::Bzip));
        assert_eq!(
            file.dcl_flags(),
            Some(CHUNK_ZLIB | CHUNK_BZIP | CHUNK_RANDOMACCESS | CHUNK_JPEG)
        );
        // '#' is not a comment delimiter in dzip.exe. The later "to" token
        // therefore remains active and changes the range.
        assert_eq!(file.byte_range(200).unwrap(), (50, 150));
        let options = config.options.unwrap();
        assert!(options.use_combuf);
        assert_eq!(options.win_size, 4);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dcl_basedirs_are_global_and_nested_master_paths_use_root_directory() {
        let root = unique_temp_dir("master");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(
            root.join("root.dcl"),
            "file payload.bin 0 copy\nmaster sub/first.dcl\n",
        )
        .unwrap();
        std::fs::write(
            root.join("sub/first.dcl"),
            "basedir first\nmaster second.dcl\n",
        )
        .unwrap();
        std::fs::write(root.join("second.dcl"), "basedir second\nalign 64\n").unwrap();

        let config = parse_config(&root.join("root.dcl")).unwrap();
        assert_eq!(
            config.dcl_search_dirs,
            [root.join("first"), root.join("second")]
        );
        assert_eq!(config.align, Some(64));
        assert!(config.files[0].source_base_dir.is_none());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dcl_options_require_selection_and_commands_run_last() {
        let root = unique_temp_dir("commands");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("commands.dcl");
        std::fs::write(
            &config_path,
            "archive first.dz\nuse_combuf 1\noptions dz\npreprocess 0\n",
        )
        .unwrap();
        let commands = vec![
            "archive second.dz".to_string(),
            "use_combuf 1".to_string(),
            "align 32".to_string(),
        ];

        let config = parse_config_with_commands(&config_path, &commands).unwrap();
        assert_eq!(config.archives, ["first.dz", "second.dz"]);
        assert_eq!(config.align, Some(32));
        let options = config.options.unwrap();
        assert!(options.use_combuf);
        assert!(!options.preprocess);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dcl_reader_splits_physical_lines_like_fgets_256() {
        let mut bytes = vec![b'x'; 300];
        bytes.push(b'\n');
        let lines = dcl_fgets_lines(&bytes);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 255);
        assert_eq!(lines[1].len(), 45);
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dzip-rs-dcl-{label}-{}-{unique}",
            std::process::id()
        ))
    }
}
