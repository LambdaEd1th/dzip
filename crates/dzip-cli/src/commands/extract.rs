use dzip::{Archive, ExtractOptions, Result};
use log::info;
use std::path::{Path, PathBuf};

pub fn extract_archive(input_path: &str, output_dir: Option<&str>) -> Result<()> {
    let destination = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output_dir(Path::new(input_path)));
    info!("Extracting {} to {}", input_path, destination.display());

    let mut archive = Archive::open_path(input_path)?;
    let report = archive.extract_to(&destination, ExtractOptions::default())?;
    info!("Extracted {} files ({} bytes)", report.files, report.bytes);
    Ok(())
}

fn default_output_dir(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .filter(|stem| !stem.is_empty())
        .unwrap_or(input.as_os_str());
    input.parent().unwrap_or_else(|| Path::new(".")).join(stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_directory_is_next_to_the_archive() {
        assert_eq!(
            default_output_dir(Path::new("archives/game.data.dz")),
            Path::new("archives/game.data")
        );
        assert_eq!(
            default_output_dir(Path::new("archive")),
            Path::new("archive")
        );
    }
}
