use crate::error::{DzipError, Result};
use crate::reader::{ReadSeek, VolumeSource};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::File;
use std::path::PathBuf;

/// A volume manager that reads volumes from the filesystem using a base directory and a file list.
pub struct FileSystemVolumeManager {
    base_dir: PathBuf,
    file_list: Vec<String>,
    open_files: HashMap<u16, File>,
}

impl FileSystemVolumeManager {
    /// Creates a new FileSystemVolumeManager.
    ///
    /// # Arguments
    /// * `base_dir` - The directory containing the volume files.
    /// * `file_list` - List of filenames for auxiliary volumes (Volume 1, Volume 2, ...).
    pub fn new(base_dir: PathBuf, file_list: Vec<String>) -> Self {
        Self {
            base_dir,
            file_list,
            open_files: HashMap::new(),
        }
    }

    pub fn file_list(&self) -> &[String] {
        &self.file_list
    }
}

pub type PathVolumeSource = FileSystemVolumeManager;

/// An in-memory auxiliary-volume provider useful for embedded callers and tests.
pub struct MemoryVolumeSource {
    volumes: HashMap<u16, std::io::Cursor<Vec<u8>>>,
}

impl MemoryVolumeSource {
    pub fn new(volumes: impl IntoIterator<Item = (u16, Vec<u8>)>) -> Self {
        Self {
            volumes: volumes
                .into_iter()
                .map(|(id, bytes)| (id, std::io::Cursor::new(bytes)))
                .collect(),
        }
    }
}

impl VolumeSource for MemoryVolumeSource {
    fn open_volume(&mut self, id: u16) -> Result<&mut dyn ReadSeek> {
        self.volumes
            .get_mut(&id)
            .map(|reader| reader as &mut dyn ReadSeek)
            .ok_or(DzipError::VolumeNotFound(id))
    }

    fn volume_len(&mut self, id: u16) -> Result<Option<u64>> {
        Ok(self
            .volumes
            .get(&id)
            .map(|reader| reader.get_ref().len() as u64))
    }
}

impl VolumeSource for FileSystemVolumeManager {
    fn open_volume(&mut self, id: u16) -> Result<&mut dyn ReadSeek> {
        // ID 0 is reserved for the main file, which is typically handled by the DzipReader itself
        // before calling into VolumeSource for other chunks. However, if open_volume IS called with 0,
        // it implies the caller expects the manager to handle it.
        // In dzip-rs, VolumeSource is used for "Auxiliary" volumes.
        // The DzipReader usually manages the main reader.
        if id == 0 {
            return Err(DzipError::Io(std::io::Error::other(
                "Volume ID 0 is reserved for main file",
            )));
        }

        let list_index = (id - 1) as usize;
        if list_index >= self.file_list.len() {
            return Err(DzipError::VolumeNotFound(id));
        }

        match self.open_files.entry(id) {
            Entry::Occupied(e) => Ok(e.into_mut()),
            Entry::Vacant(e) => {
                let file_name = &self.file_list[list_index];
                let relative = crate::path::resolve_relative_path(file_name).map_err(|error| {
                    DzipError::VolumeOpenError(id, format!("unsafe volume path: {error}"))
                })?;
                let path = self.base_dir.join(relative);
                let file =
                    File::open(&path).map_err(|e| DzipError::VolumeOpenError(id, e.to_string()))?;
                Ok(e.insert(file))
            }
        }
    }
}
