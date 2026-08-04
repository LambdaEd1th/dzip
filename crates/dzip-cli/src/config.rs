use dzip::format::{
    CHUNK_BZIP, CHUNK_COMBUF, CHUNK_COPYCOMP, CHUNK_DZ, CHUNK_JPEG, CHUNK_LZMA, CHUNK_MP3,
    CHUNK_RANDOMACCESS, CHUNK_ZERO, CHUNK_ZLIB,
};
use dzip::{ChunkEncoding, Compression};
use std::collections::HashSet;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug)]
pub enum ConfigError {
    Io {
        context: Option<String>,
        source: io::Error,
    },
    Invalid(String),
}

impl ConfigError {
    fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: Some(context.into()),
            source,
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                context: Some(context),
                source,
            } => write!(formatter, "{context}: {source}"),
            Self::Io {
                context: None,
                source,
            } => source.fmt(formatter),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Invalid(_) => None,
        }
    }
}

impl From<io::Error> for ConfigError {
    fn from(source: io::Error) -> Self {
        Self::Io {
            context: None,
            source,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DclConfig {
    pub archives: Vec<String>,
    pub align: u32,
    pub files: Vec<FileEntry>,
    pub options: GlobalOptions,
    pub(crate) dcl_search_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub archive_file_index: i32,
    pub(crate) range: DclRange,
    pub(crate) flags: u16,
}

impl FileEntry {
    pub fn byte_range(&self, total_len: usize) -> Result<(usize, usize)> {
        self.range.resolve(total_len, &self.path)
    }

    pub const fn dcl_flags(&self) -> u16 {
        self.flags
    }

    pub fn selected_compression(&self) -> Option<Compression> {
        dcl_compression(self.flags)
    }

    pub const fn requested_archive_file_index(&self) -> i32 {
        self.archive_file_index
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
                    .ok_or_else(|| ConfigError::invalid("DCL percentage range overflow"))?
                    / 100;
                usize::try_from(value).map_err(|_| {
                    ConfigError::invalid("DCL percentage range exceeds platform limits")
                })
            } else {
                Ok(raw as usize)
            }
        };

        let start = resolve_boundary(self.from)?;
        let end = resolve_boundary(self.to)?;
        if start > end || end > total_len {
            return Err(ConfigError::invalid(format!(
                "DCL byte range {}..{} is outside {} ({} bytes)",
                start,
                end,
                path.display(),
                total_len
            )));
        }
        Ok((start, end))
    }
}

#[derive(Debug, Clone)]
pub struct GlobalOptions {
    pub method: String,
    pub is_not_default: bool,
    pub max_mem_usage: i32,
    pub use_combuf: bool,
    pub preprocess: bool,
    pub trim_reference_factor: i32,
    pub win_size: u8,
    pub flags: u8,
    pub offset_table_size: u8,
    pub offset_tables: u8,
    pub offset_contexts: u8,
    pub ref_length_table_size: u8,
    pub ref_length_tables: u8,
    pub ref_offset_table_size: u8,
    pub ref_offset_tables: u8,
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

pub fn parse_config(path: &Path) -> Result<DclConfig> {
    parse_config_with_commands(path, &[])
}

pub fn parse_config_with_commands(path: &Path, commands: &[String]) -> Result<DclConfig> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dcl"))
    {
        return Err(ConfigError::invalid(format!(
            "Dzip build configuration must use the .dcl extension: {}",
            path.display()
        )));
    }

    let mut config = DclConfig {
        archives: Vec::new(),
        align: 0,
        files: Vec::new(),
        options: GlobalOptions::default(),
        dcl_search_dirs: Vec::new(),
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
    config: &'a mut DclConfig,
    root_dir: PathBuf,
    include_stack: HashSet<PathBuf>,
    options_selected: bool,
}

impl DclParser<'_> {
    fn parse_file(&mut self, path: &Path, is_root: bool) -> Result<()> {
        // dzip.exe resets alignment every time a master file is entered.
        self.config.align = 0;

        let canonical_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !self.include_stack.insert(canonical_path.clone()) {
            return Err(ConfigError::invalid(format!(
                "Recursive master include detected at {}",
                path.display()
            )));
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
                return Err(ConfigError::io(
                    format!("Failed to read config {}", path.display()),
                    error,
                ));
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
                self.config.align = u32::try_from(value).unwrap_or(0);
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
                self.config.options.method = "dz".to_string();
            }
            "options" => {}
            key if self.options_selected => {
                set_dcl_option(&mut self.config.options, key, &parts[1]);
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
            archive_file_index: dcl_archive_file_index,
            range,
            flags,
        });
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
            CHUNK_ZLIB | CHUNK_BZIP | CHUNK_RANDOMACCESS | CHUNK_JPEG
        );
        // '#' is not a comment delimiter in dzip.exe. The later "to" token
        // therefore remains active and changes the range.
        assert_eq!(file.byte_range(200).unwrap(), (50, 150));
        let options = config.options;
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
        assert_eq!(config.align, 64);

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
        assert_eq!(config.align, 32);
        let options = config.options;
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

    #[test]
    fn toml_manifests_are_rejected() {
        let root = unique_temp_dir("toml-rejected");
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("pack.toml");
        std::fs::write(&config_path, "archives = [\"output.dz\"]\n").unwrap();

        let error = parse_config(&config_path).unwrap_err();
        assert!(error.to_string().contains("must use the .dcl extension"));

        std::fs::remove_dir_all(root).unwrap();
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
