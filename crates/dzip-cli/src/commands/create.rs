use dzip::{ChunkEncoding, Compression, ContentHint, Result};
use dzip_workflow::{BuildEntry, BuildPlan, DzConfig, write_archive_to_directory};
use log::{debug, info};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    Bytes(u64),
    Percent(u8),
}

impl Boundary {
    fn resolve(self, length: usize) -> Result<usize> {
        match self {
            Self::Bytes(value) => usize::try_from(value)
                .map_err(|_| invalid_input("range boundary exceeds platform limits")),
            Self::Percent(value) => Ok(length.saturating_mul(usize::from(value)) / 100),
        }
    }
}

impl FromStr for Boundary {
    type Err = ParseBoundaryError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        if let Some(percent) = value.strip_suffix('%') {
            let percent = percent
                .parse::<u8>()
                .map_err(|_| ParseBoundaryError(value.to_string()))?;
            if percent > 100 {
                return Err(ParseBoundaryError(value.to_string()));
            }
            return Ok(Self::Percent(percent));
        }
        value
            .parse::<u64>()
            .map(Self::Bytes)
            .map_err(|_| ParseBoundaryError(value.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseBoundaryError(String);

impl fmt::Display for ParseBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "expected a non-negative byte offset or percentage from 0% to 100%, got '{}'",
            self.0
        )
    }
}

impl std::error::Error for ParseBoundaryError {}

pub struct CreateRequest<'a> {
    pub archive: &'a str,
    pub files: &'a [String],
    pub output_dir: &'a str,
    pub source_dirs: &'a [String],
    pub alignment: u32,
    pub compression: Compression,
    pub start: Option<Boundary>,
    pub end: Option<Boundary>,
    pub random_access: bool,
    pub content_hint: Option<ContentHint>,
    pub use_combuf: bool,
}

pub fn create_archive(request: CreateRequest<'_>) -> Result<()> {
    let archive_path = Path::new(request.archive);
    if archive_path.is_absolute() {
        return Err(invalid_input(
            "ARCHIVE must be relative to --output; pass its directory with --output",
        ));
    }

    let source_dirs: Vec<PathBuf> = if request.source_dirs.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        request.source_dirs.iter().map(PathBuf::from).collect()
    };
    let dz_options = DzConfig {
        use_combuf: request.use_combuf,
        ..DzConfig::default()
    };
    let mut plan = BuildPlan {
        archive_name: request.archive.to_string(),
        volume_names: vec![request.archive.to_string()],
        alignment: request.alignment,
        dz_options,
        entries: Vec::with_capacity(request.files.len()),
    };

    for file in request.files {
        let archive_path = Path::new(file);
        if archive_path.is_absolute() {
            return Err(invalid_input(format!(
                "input path '{}' must be archive-relative; use --dir for its source root",
                archive_path.display()
            )));
        }
        let (source, bytes) = read_source(archive_path, &source_dirs)?;
        let start = request
            .start
            .map_or(Ok(0), |boundary| boundary.resolve(bytes.len()))?;
        let end = request
            .end
            .map_or(Ok(bytes.len()), |boundary| boundary.resolve(bytes.len()))?;
        if start > end || end > bytes.len() {
            return Err(invalid_input(format!(
                "range {start}..{end} is outside {} ({} bytes)",
                archive_path.display(),
                bytes.len()
            )));
        }

        plan.entries.push(BuildEntry {
            path: archive_path.to_string_lossy().into_owned(),
            bytes: bytes[start..end].to_vec(),
            encoding: ChunkEncoding {
                compression: request.compression,
                random_access: request.random_access,
                common_buffer: false,
                content_hint: request.content_hint,
                unknown_flags: 0,
            },
            raw_flags: None,
            volume: 0,
        });
        debug!("Added {}", source.display());
    }

    let report = write_archive_to_directory(plan, request.output_dir).map_err(workflow_error)?;
    info!(
        "Created {} with {} files and {} chunks",
        request.archive, report.entries, report.chunks
    );
    Ok(())
}

fn workflow_error(error: impl std::fmt::Display) -> dzip::DzipError {
    invalid_input(error.to_string())
}

fn read_source(path: &Path, source_dirs: &[PathBuf]) -> Result<(PathBuf, Vec<u8>)> {
    let mut last_error = None;
    for directory in source_dirs {
        let source = directory.join(path);
        match std::fs::read(&source) {
            Ok(bytes) => return Ok((source, bytes)),
            Err(error) => last_error = Some((source, error)),
        }
    }
    let (source, error) = last_error.expect("create always has at least one source directory");
    Err(dzip::DzipError::Io(std::io::Error::new(
        error.kind(),
        format!("failed to read {}: {error}", source.display()),
    )))
}

fn invalid_input(message: impl Into<String>) -> dzip::DzipError {
    dzip::DzipError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_are_typed_and_bounded() {
        assert_eq!("4096".parse(), Ok(Boundary::Bytes(4096)));
        assert_eq!("25%".parse(), Ok(Boundary::Percent(25)));
        assert!("101%".parse::<Boundary>().is_err());
        assert!("-1".parse::<Boundary>().is_err());
    }
}
