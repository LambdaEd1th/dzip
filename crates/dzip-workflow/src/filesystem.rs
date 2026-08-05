use crate::error::{Result, WorkflowError};
use crate::model::{ExtractedFile, NamedBytes};
use std::path::{Component, Path, PathBuf};

pub fn write_extracted_files(root: impl AsRef<Path>, files: &[ExtractedFile]) -> Result<()> {
    write_relative_files(
        root.as_ref(),
        files
            .iter()
            .map(|file| (file.path.as_str(), file.bytes.as_slice())),
    )
}

pub fn write_named_files(root: impl AsRef<Path>, files: &[NamedBytes]) -> Result<()> {
    write_relative_files(
        root.as_ref(),
        files
            .iter()
            .map(|file| (file.name.as_str(), file.bytes.as_slice())),
    )
}

fn write_relative_files<'a>(
    root: &Path,
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<()> {
    std::fs::create_dir_all(root)?;
    reject_symlink(root)?;
    for (relative, bytes) in files {
        let components = safe_components(relative)?;
        let mut target = root.to_path_buf();
        for component in &components[..components.len() - 1] {
            target.push(component);
            if target.exists() {
                reject_symlink(&target)?;
                if !target.is_dir() {
                    return Err(WorkflowError::invalid(format!(
                        "archive output parent is not a directory: {}",
                        target.display()
                    )));
                }
            } else {
                std::fs::create_dir(&target)?;
            }
        }
        target.push(&components[components.len() - 1]);
        if target.exists() {
            reject_symlink(&target)?;
            if target.is_dir() {
                return Err(WorkflowError::invalid(format!(
                    "archive output target is a directory: {}",
                    target.display()
                )));
            }
        }
        std::fs::write(target, bytes)?;
    }
    Ok(())
}

fn safe_components(relative: &str) -> Result<Vec<PathBuf>> {
    let normalized = relative.replace('\\', "/");
    let components = Path::new(&normalized)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(Ok(PathBuf::from(part))),
            Component::CurDir => None,
            _ => Some(Err(WorkflowError::invalid(format!(
                "unsafe archive output path: {relative}"
            )))),
        })
        .collect::<Result<Vec<_>>>()?;
    if components.is_empty() {
        return Err(WorkflowError::invalid("archive output path is empty"));
    }
    Ok(components)
}

fn reject_symlink(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(WorkflowError::invalid(format!(
            "refusing to write through symbolic link: {}",
            path.display()
        )));
    }
    Ok(())
}
