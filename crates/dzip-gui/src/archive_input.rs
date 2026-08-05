//! Pure preparation of dropped archive volumes and extraction paths.

use crate::file_drop;

pub(crate) type NamedFileBytes = (String, Vec<u8>);
pub(crate) type PreparedArchiveFiles = (String, Vec<u8>, Vec<NamedFileBytes>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DroppedArchiveError {
    NoMainArchive,
    MultipleMainArchives,
    Read(String),
    Open(String),
}

pub(crate) fn prepare_dropped_archive(
    files: Vec<file_drop::DroppedFile>,
) -> Result<PreparedArchiveFiles, DroppedArchiveError> {
    prepare_archive_files(
        files
            .into_iter()
            .map(|file| (dropped_file_name(&file.path).to_string(), file.bytes))
            .collect(),
    )
}

pub(crate) fn prepare_archive_files(
    files: Vec<NamedFileBytes>,
) -> Result<PreparedArchiveFiles, DroppedArchiveError> {
    let mut main_archives = Vec::new();
    let mut auxiliary = Vec::new();
    for (name, bytes) in files {
        if is_main_archive_name(&name) {
            main_archives.push((name, bytes));
        } else if is_archive_volume_name(&name) {
            auxiliary.push((name, bytes));
        }
    }
    if main_archives.is_empty() {
        return Err(DroppedArchiveError::NoMainArchive);
    }
    if main_archives.len() > 1 {
        return Err(DroppedArchiveError::MultipleMainArchives);
    }
    auxiliary.sort_by_key(|(name, _)| name.to_ascii_lowercase());
    let (main_name, main_bytes) = main_archives
        .pop()
        .ok_or(DroppedArchiveError::NoMainArchive)?;
    Ok((main_name, main_bytes, auxiliary))
}

pub(crate) fn rebase_extracted_files(
    mut files: Vec<NamedFileBytes>,
    export_base: Option<&str>,
) -> Vec<NamedFileBytes> {
    let Some(export_base) = export_base.map(str::trim).filter(|path| !path.is_empty()) else {
        return files;
    };
    let prefix = format!("{}/", export_base.trim_matches('/'));
    if !files
        .iter()
        .all(|(path, _)| path.trim_matches('/').starts_with(&prefix))
    {
        return files;
    }
    for (path, _) in &mut files {
        *path = path
            .trim_matches('/')
            .strip_prefix(&prefix)
            .unwrap_or(path)
            .to_string();
    }
    files
}

fn dropped_file_name(path: &str) -> &str {
    path.trim_matches('/').rsplit('/').next().unwrap_or(path)
}

fn is_main_archive_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".dz") || name.ends_with(".dzip")
}

fn is_archive_volume_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        extension.len() >= 3 && extension.bytes().all(|byte| byte.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_drop_finds_main_and_sorts_volumes() {
        let files = vec![
            file_drop::DroppedFile {
                path: "release/game.002".into(),
                bytes: vec![2],
            },
            file_drop::DroppedFile {
                path: "release/game.DZ".into(),
                bytes: vec![0],
            },
            file_drop::DroppedFile {
                path: "release/game.001".into(),
                bytes: vec![1],
            },
        ];
        let prepared = prepare_dropped_archive(files).unwrap();
        assert_eq!(prepared.0, "game.DZ");
        assert_eq!(
            prepared
                .2
                .iter()
                .map(|item| item.0.as_str())
                .collect::<Vec<_>>(),
            ["game.001", "game.002"]
        );
    }

    #[test]
    fn extraction_rebases_only_a_common_selected_parent() {
        let files = vec![
            ("Data/UI/a".to_string(), vec![1]),
            ("Data/UI/b".to_string(), vec![2]),
        ];
        assert_eq!(
            rebase_extracted_files(files.clone(), Some("Data"))[0].0,
            "UI/a"
        );
        assert_eq!(rebase_extracted_files(files.clone(), None), files);
        assert_eq!(
            prepare_dropped_archive(vec![file_drop::DroppedFile {
                path: "game.001".into(),
                bytes: vec![1],
            }]),
            Err(DroppedArchiveError::NoMainArchive)
        );
    }
}
