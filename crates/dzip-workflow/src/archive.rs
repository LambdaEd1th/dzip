use crate::error::{Result, WorkflowError};
use crate::model::{
    ArchiveSummary, DzConfig, EditableEntry, EditableSegment, EntrySummary, ExtractedFile,
    NamedBytes, SegmentSummary,
};
use dzip::{Archive, ArchivePreparation, DzOptions, EntryId, MemoryVolumeSource};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

type MemoryArchive = Archive<Cursor<Vec<u8>>, MemoryVolumeSource>;

/// A parsed archive retained by an application backend.
///
/// Keeping the reader alive preserves decoded DZ context and avoids sending or
/// reparsing complete archive volumes for every extraction request.
pub struct ArchiveSession {
    archive: MemoryArchive,
    summary: ArchiveSummary,
    name: String,
    source_size: u64,
}

impl ArchiveSession {
    pub fn open(
        main_name: String,
        main_bytes: Vec<u8>,
        auxiliary_files: Vec<NamedBytes>,
    ) -> Result<Self> {
        let main_size = main_bytes.len() as u64;
        let preparation =
            ArchivePreparation::read(Cursor::new(main_bytes), dzip::ReadOptions::default())?;
        let volumes =
            match_auxiliary_volumes(&preparation.metadata().volume_files, auxiliary_files, &[])?;
        let source_size = volumes.iter().try_fold(main_size, |total, (_, bytes)| {
            total
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| WorkflowError::invalid("archive source size overflow"))
        })?;
        let mut archive = preparation.open(MemoryVolumeSource::new(volumes))?;
        let available = archive.volume_source().available_ids().collect::<Vec<_>>();
        for id in available {
            archive.resolve_volume(id)?;
        }
        let summary = summarize(&archive, main_name.clone(), source_size);
        Ok(Self {
            archive,
            summary,
            name: main_name,
            source_size,
        })
    }

    pub const fn summary(&self) -> &ArchiveSummary {
        &self.summary
    }

    pub fn supply_volumes(&mut self, files: Vec<NamedBytes>) -> Result<()> {
        let expected = self.archive.index().raw_metadata().volume_files.clone();
        let existing = self
            .archive
            .volume_source_mut()
            .available_ids()
            .collect::<Vec<_>>();
        let volumes = match_auxiliary_volumes(&expected, files, &existing)?;
        for (id, bytes) in volumes {
            self.source_size = self
                .source_size
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| WorkflowError::invalid("archive source size overflow"))?;
            self.archive.volume_source_mut().insert(id, bytes);
            self.archive.resolve_volume(id)?;
        }
        self.refresh_summary();
        Ok(())
    }

    pub fn read_entries(&mut self, entry_ids: &[usize]) -> Result<Vec<ExtractedFile>> {
        let mut files = Vec::with_capacity(entry_ids.len());
        for &id in entry_ids {
            let entry = self
                .archive
                .entry(EntryId(id))
                .ok_or(WorkflowError::EntryNotFound(id))?;
            let path = portable_path(entry.path());
            let bytes = self.archive.read_entry(EntryId(id))?;
            files.push(ExtractedFile { path, bytes });
        }
        self.refresh_summary();
        Ok(files)
    }

    pub fn editable_entries(&mut self, entry_ids: &[usize]) -> Result<Vec<EditableEntry>> {
        let mut files = Vec::with_capacity(entry_ids.len());
        for &id in entry_ids {
            let entry = self
                .archive
                .entry(EntryId(id))
                .ok_or(WorkflowError::EntryNotFound(id))?;
            let path = portable_path(entry.path());
            let segments = entry
                .segments()
                .iter()
                .map(|segment| EditableSegment {
                    length: usize::try_from(
                        segment.decoded_range().end - segment.decoded_range().start,
                    )
                    .unwrap_or(usize::MAX),
                    encoding: segment.encoding(),
                    raw_flags: self.archive.index().stored_chunks()[segment.chunk_id() as usize]
                        .flags,
                    volume: segment.volume(),
                })
                .collect::<Vec<_>>();
            let bytes = self.archive.read_entry(EntryId(id))?;
            let expected = segments
                .iter()
                .try_fold(0usize, |total, segment| total.checked_add(segment.length));
            if expected != Some(bytes.len()) {
                return Err(WorkflowError::invalid(format!(
                    "entry {path} segment lengths do not match its decoded size"
                )));
            }
            files.push(EditableEntry {
                id,
                path,
                bytes,
                segments,
            });
        }
        self.refresh_summary();
        Ok(files)
    }

    fn refresh_summary(&mut self) {
        self.summary = summarize(&self.archive, self.name.clone(), self.source_size);
    }
}

fn summarize(archive: &MemoryArchive, name: String, source_size: u64) -> ArchiveSummary {
    let index = archive.index();
    let chunks = index.resolved_chunks();
    let mut unpacked_size = 0u64;
    let entries = archive
        .entries()
        .iter()
        .map(|entry| {
            let size = entry.decompressed_size();
            unpacked_size = unpacked_size.saturating_add(size);
            let packed_size = entry
                .segments()
                .iter()
                .all(|segment| archive.is_volume_resolved(segment.volume()))
                .then(|| {
                    entry
                        .chunk_ids()
                        .iter()
                        .filter_map(|id| chunks.get(*id as usize))
                        .map(|chunk| u64::from(chunk.physical_length))
                        .sum()
                });
            let path = portable_path(entry.path());
            let path_ref = Path::new(&path);
            EntrySummary {
                id: entry.id().0,
                name: path_ref
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone()),
                folder: path_ref
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .map(portable_path)
                    .unwrap_or_default(),
                path,
                size,
                packed_size,
                compression: entry.compression(),
                volume: entry.volume(),
                chunks: entry.segments().len(),
                segments: entry
                    .segments()
                    .iter()
                    .map(|segment| SegmentSummary {
                        decoded_start: segment.decoded_range().start,
                        decoded_end: segment.decoded_range().end,
                        encoding: segment.encoding(),
                        raw_flags: index.stored_chunks()[segment.chunk_id() as usize].flags,
                        volume: segment.volume(),
                    })
                    .collect(),
            }
        })
        .collect();

    let options = DzOptions {
        use_combuf: index.has_dz_common_buffer(),
        settings: index.range_settings().unwrap_or_default(),
        ..DzOptions::default()
    };
    ArchiveSummary {
        name,
        entries,
        dz_options: DzConfig::from_options(&options),
        source_size,
        source_complete: archive.volume_source().available_ids().count()
            == index.volume_files().len(),
        unpacked_size,
        chunk_count: chunks.len(),
        volume_count: index.volume_files().len() + 1,
        loaded_volume_count: archive.volume_source().available_ids().count() + 1,
    }
}

fn match_auxiliary_volumes(
    expected_names: &[dzip::ArchiveString],
    mut supplied: Vec<NamedBytes>,
    existing: &[u16],
) -> Result<Vec<(u16, Vec<u8>)>> {
    let mut by_name = HashMap::<String, Vec<u8>>::new();
    for file in supplied.drain(..) {
        let key = normalized_name(&file.name);
        if by_name.insert(key, file.bytes).is_some() {
            return Err(WorkflowError::invalid(format!(
                "duplicate auxiliary volume name {}",
                file.name
            )));
        }
    }

    let mut result = Vec::with_capacity(by_name.len());
    for (index, expected) in expected_names.iter().enumerate() {
        let id = u16::try_from(index + 1)
            .map_err(|_| WorkflowError::invalid("archive has more than 65535 volumes"))?;
        if existing.contains(&id) {
            continue;
        }
        let expected = expected.to_string_lossy();
        let key = normalized_name(&expected);
        let bytes = if let Some(bytes) = by_name.remove(&key) {
            Some(bytes)
        } else {
            let base = basename(&key);
            let matches = by_name
                .keys()
                .filter(|candidate| basename(candidate) == base)
                .cloned()
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [] => None,
                [matching] => by_name.remove(matching),
                _ => {
                    return Err(WorkflowError::invalid(format!(
                        "ambiguous auxiliary volume name: {expected}"
                    )));
                }
            }
        };
        if let Some(bytes) = bytes {
            result.push((id, bytes));
        }
    }
    if !by_name.is_empty() {
        let mut names = by_name.into_keys().collect::<Vec<_>>();
        names.sort();
        return Err(WorkflowError::invalid(format!(
            "unexpected auxiliary volume(s): {}",
            names.join(", ")
        )));
    }
    Ok(result)
}

fn normalized_name(name: &str) -> String {
    name.trim().replace('\\', "/").to_ascii_lowercase()
}

fn basename(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn portable_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
