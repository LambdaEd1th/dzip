//! Pure archive-browser projection and path operations.

use dzip_gui::model::{DraftFile, EntryView};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FolderView {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) file_count: usize,
    pub(crate) size: u64,
    pub(crate) packed_size: Option<u64>,
    pub(crate) volume: Option<u16>,
    pub(crate) entry_ids: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserCrumb {
    pub(crate) name: String,
    pub(crate) path: String,
}

trait BrowserItem: Clone {
    fn browser_path(&self) -> &str;
    fn browser_size(&self) -> u64;
    fn browser_packed_size(&self) -> Option<u64>;
    fn browser_volume(&self) -> u16;
    fn browser_entry_id(&self) -> Option<usize>;
}

impl BrowserItem for EntryView {
    fn browser_path(&self) -> &str {
        &self.path
    }

    fn browser_size(&self) -> u64 {
        self.size
    }

    fn browser_packed_size(&self) -> Option<u64> {
        self.packed_size
    }

    fn browser_volume(&self) -> u16 {
        self.volume
    }

    fn browser_entry_id(&self) -> Option<usize> {
        Some(self.id)
    }
}

impl BrowserItem for DraftFile {
    fn browser_path(&self) -> &str {
        &self.path
    }

    fn browser_size(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn browser_packed_size(&self) -> Option<u64> {
        None
    }

    fn browser_volume(&self) -> u16 {
        self.volume
    }

    fn browser_entry_id(&self) -> Option<usize> {
        None
    }
}

pub(crate) fn build_browser_listing(
    entries: &[EntryView],
    current_dir: &str,
    query: &str,
) -> (Vec<FolderView>, Vec<EntryView>) {
    if !query.is_empty() {
        let mut files: Vec<EntryView> = entries
            .iter()
            .filter(|entry| entry.path.to_lowercase().contains(query))
            .cloned()
            .collect();
        files.sort_by_key(|entry| entry.path.to_lowercase());
        return (Vec::new(), files);
    }
    build_directory_listing(entries, current_dir)
}

pub(crate) fn build_draft_browser_listing(
    files: &[DraftFile],
    current_dir: &str,
) -> (Vec<FolderView>, Vec<DraftFile>) {
    build_directory_listing(files, current_dir)
}

fn build_directory_listing<T: BrowserItem>(
    items: &[T],
    current_dir: &str,
) -> (Vec<FolderView>, Vec<T>) {
    let current_dir = current_dir.trim_matches('/');
    let prefix = if current_dir.is_empty() {
        String::new()
    } else {
        format!("{current_dir}/")
    };
    let mut folder_map = BTreeMap::<String, FolderView>::new();
    let mut files = Vec::new();

    for item in items {
        let path = item.browser_path().trim_matches('/');
        let Some(relative) = path.strip_prefix(&prefix) else {
            continue;
        };
        if let Some((folder_name, _)) = relative.split_once('/') {
            if folder_name.is_empty() {
                continue;
            }
            let folder_path = if current_dir.is_empty() {
                folder_name.to_string()
            } else {
                format!("{current_dir}/{folder_name}")
            };
            let folder = folder_map
                .entry(folder_path.clone())
                .or_insert_with(|| FolderView {
                    name: folder_name.to_string(),
                    path: folder_path,
                    file_count: 0,
                    size: 0,
                    packed_size: item.browser_packed_size().map(|_| 0),
                    volume: Some(item.browser_volume()),
                    entry_ids: Vec::new(),
                });
            if folder.file_count > 0 && folder.volume != Some(item.browser_volume()) {
                folder.volume = None;
            }
            folder.file_count += 1;
            folder.size = folder.size.saturating_add(item.browser_size());
            folder.packed_size = match (folder.packed_size, item.browser_packed_size()) {
                (Some(total), Some(size)) => Some(total.saturating_add(size)),
                _ => None,
            };
            if let Some(id) = item.browser_entry_id() {
                folder.entry_ids.push(id);
            }
        } else if !relative.is_empty() {
            files.push(item.clone());
        }
    }

    let mut folders: Vec<FolderView> = folder_map.into_values().collect();
    folders.sort_by_key(|folder| folder.name.to_lowercase());
    files.sort_by_key(|item| draft_file_name(item.browser_path()).to_lowercase());
    (folders, files)
}

pub(crate) fn remove_draft_folder(files: &mut Vec<DraftFile>, folder_path: &str) {
    let folder_path = folder_path.trim_matches('/');
    let prefix = format!("{folder_path}/");
    files.retain(|file| !file.path.trim_matches('/').starts_with(&prefix));
}

pub(crate) fn entry_ids_in_directory(entries: &[EntryView], directory: &str) -> Vec<usize> {
    let directory = directory.trim_matches('/');
    if directory.is_empty() {
        return entries.iter().map(|entry| entry.id).collect();
    }
    let prefix = format!("{directory}/");
    entries
        .iter()
        .filter(|entry| entry.path.trim_matches('/').starts_with(&prefix))
        .map(|entry| entry.id)
        .collect()
}

pub(crate) fn browser_breadcrumbs(current_dir: &str) -> Vec<BrowserCrumb> {
    let mut breadcrumb_path = String::new();
    current_dir
        .split('/')
        .filter(|part| !part.is_empty())
        .map(|part| {
            if !breadcrumb_path.is_empty() {
                breadcrumb_path.push('/');
            }
            breadcrumb_path.push_str(part);
            BrowserCrumb {
                name: part.to_string(),
                path: breadcrumb_path.clone(),
            }
        })
        .collect()
}

pub(crate) fn parent_browser_directory(current_dir: &str) -> String {
    current_dir
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

pub(crate) fn draft_file_name(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

pub(crate) fn replace_draft_file_name(path: &str, file_name: &str) -> String {
    path.trim_end_matches('/')
        .rsplit_once('/')
        .map(|(parent, _)| format!("{parent}/{file_name}"))
        .unwrap_or_else(|| file_name.to_string())
}

pub(crate) fn join_archive_path(directory: &str, path: &str) -> String {
    let directory = directory.trim_matches('/');
    let path = path.trim_matches('/');
    if directory.is_empty() {
        path.to_string()
    } else if path.is_empty() {
        directory.to_string()
    } else {
        format!("{directory}/{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dzip_gui::model::CompressionChoice;
    use std::sync::Arc;

    fn entry(id: usize, path: &str, size: u64) -> EntryView {
        EntryView {
            id,
            path: path.to_string(),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            folder: path
                .rsplit_once('/')
                .map(|(folder, _)| folder.to_string())
                .unwrap_or_default(),
            size,
            packed_size: Some(size / 2),
            compression: CompressionChoice::Dz,
            volume: 0,
            chunks: 1,
        }
    }

    fn draft(id: u64, path: &str, size: usize) -> DraftFile {
        DraftFile {
            id,
            path: path.to_string(),
            bytes: Arc::from(vec![0u8; size]),
            compression: CompressionChoice::Dz,
            volume: 0,
            preserved_segments: None,
        }
    }

    #[test]
    fn directory_projection_and_global_search_are_stable() {
        let entries = vec![
            entry(0, "readme.txt", 20),
            entry(1, "Data/config.json", 40),
            entry(2, "Data/Images/logo.png", 60),
            entry(3, "Sounds/theme.ogg", 80),
        ];
        let (folders, files) = build_browser_listing(&entries, "", "");
        assert_eq!(
            folders
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["Data", "Sounds"]
        );
        assert_eq!(folders[0].packed_size, Some(50));
        assert_eq!(files[0].path, "readme.txt");
        assert_eq!(entry_ids_in_directory(&entries, "Data"), [1, 2]);
        let (_, search) = build_browser_listing(&entries, "Data", "theme");
        assert_eq!(search[0].path, "Sounds/theme.ogg");
    }

    #[test]
    fn draft_paths_and_folder_removal_are_pure() {
        let mut files = vec![
            draft(1, "Data/config.json", 40),
            draft(2, "Data/Images/logo.png", 60),
            draft(3, "Database/index.bin", 20),
        ];
        let (folders, _) = build_draft_browser_listing(&files, "");
        assert_eq!(folders[0].name, "Data");
        assert_eq!(browser_breadcrumbs("Data/Images")[1].path, "Data/Images");
        assert_eq!(replace_draft_file_name("Data/a", "b"), "Data/b");
        assert_eq!(join_archive_path("Data", "Images/a"), "Data/Images/a");
        remove_draft_folder(&mut files, "Data");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "Database/index.bin");
    }
}
