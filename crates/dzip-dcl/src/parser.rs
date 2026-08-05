use super::directives::{parse_file_directive, set_dcl_option};
use super::lexer::{dcl_fgets_lines, tokenize_dcl_line};
use super::model::{ConfigError, DclConfig, GlobalOptions, Result};
use super::number::atoi_compat;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn parse_config(path: &Path) -> Result<DclConfig> {
    parse_config_with_commands(path, &[])
}

pub fn parse_config_with_commands(path: &Path, commands: &[String]) -> Result<DclConfig> {
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
            "file" if parts.len() >= 3 => {
                self.config.files.push(parse_file_directive(&parts));
            }
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
}
