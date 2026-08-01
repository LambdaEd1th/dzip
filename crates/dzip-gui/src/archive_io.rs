use dzip::{
    Archive, ArchiveBuilder, BuildReport, Compatibility, Compression, ContentHint, EntryId,
    EntryOptions, MemoryVolumeSink, MemoryVolumeSource, PackOptions,
};
use std::io::Cursor;
use std::path::Path;

const DZIP_MAGIC: [u8; 4] = *b"DTRZ";

#[derive(Debug, Clone)]
pub struct NamedBytes {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ArchiveEntryView {
    pub id: EntryId,
    pub path: String,
    pub size: u64,
    pub compression: Compression,
    pub volume: u16,
}

#[derive(Debug, Clone)]
pub struct LoadedArchive {
    main_name: String,
    main: Vec<u8>,
    volumes: Vec<(u16, Vec<u8>)>,
    entries: Vec<ArchiveEntryView>,
    volume_count: usize,
}

impl LoadedArchive {
    pub fn open(files: Vec<NamedBytes>) -> Result<Self, String> {
        if files.is_empty() {
            return Err("请选择主 .dz 文件；分卷归档需要同时选择全部分卷".to_string());
        }

        let mut last_error = None;
        for (main_index, candidate) in files.iter().enumerate() {
            if !candidate.bytes.starts_with(&DZIP_MAGIC) {
                continue;
            }

            let preliminary = match Archive::open_with_volumes(
                Cursor::new(candidate.bytes.clone()),
                MemoryVolumeSource::new([]),
            ) {
                Ok(archive) => archive,
                Err(error) => {
                    last_error = Some(error.to_string());
                    continue;
                }
            };

            let expected_names = preliminary.index().volume_files().to_vec();
            let volumes = match match_auxiliary_volumes(&files, main_index, &expected_names) {
                Ok(volumes) => volumes,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };

            let archive = Archive::open_with_volumes(
                Cursor::new(candidate.bytes.clone()),
                MemoryVolumeSource::new(volumes.clone()),
            )
            .map_err(|error| format!("无法打开归档：{error}"))?;

            let entries = archive
                .entries()
                .iter()
                .map(|entry| ArchiveEntryView {
                    id: entry.id(),
                    path: entry.path().to_string_lossy().replace('\\', "/"),
                    size: entry.decompressed_size(),
                    compression: entry.compression(),
                    volume: entry.volume(),
                })
                .collect();

            return Ok(Self {
                main_name: candidate.name.clone(),
                main: candidate.bytes.clone(),
                volumes,
                entries,
                volume_count: expected_names.len() + 1,
            });
        }

        Err(last_error.unwrap_or_else(|| "所选文件中没有有效的 Dzip 主卷".to_string()))
    }

    pub fn name(&self) -> &str {
        &self.main_name
    }

    pub fn entries(&self) -> &[ArchiveEntryView] {
        &self.entries
    }

    pub const fn volume_count(&self) -> usize {
        self.volume_count
    }

    pub fn extract_entry(&self, id: EntryId) -> Result<NamedBytes, String> {
        let view = self
            .entries
            .get(id.0)
            .ok_or_else(|| format!("归档条目 {} 不存在", id.0))?;
        let mut archive = self.reopen()?;
        let bytes = archive
            .read_entry(id)
            .map_err(|error| format!("解压 {} 失败：{error}", view.path))?;
        Ok(NamedBytes {
            name: download_name(&view.path),
            bytes,
        })
    }

    pub fn verify(&self) -> Result<(usize, u64), String> {
        let mut archive = self.reopen()?;
        let mut total = 0u64;
        for entry in &self.entries {
            let bytes = archive
                .read_entry(entry.id)
                .map_err(|error| format!("校验 {} 失败：{error}", entry.path))?;
            total = total
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| "解压数据总量溢出".to_string())?;
        }
        Ok((self.entries.len(), total))
    }

    fn reopen(&self) -> Result<Archive<Cursor<Vec<u8>>, MemoryVolumeSource>, String> {
        Archive::open_with_volumes(
            Cursor::new(self.main.clone()),
            MemoryVolumeSource::new(self.volumes.clone()),
        )
        .map_err(|error| format!("重新打开归档失败：{error}"))
    }
}

#[derive(Debug, Clone)]
pub struct PackInput {
    pub path: String,
    pub bytes: Vec<u8>,
    pub volume: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintChoice {
    Auto,
    None,
    Mp3,
    Jpeg,
}

#[derive(Debug, Clone)]
pub struct PackRequest {
    pub archive_name: String,
    pub volume_count: u16,
    pub alignment: u32,
    pub compression: Compression,
    pub compatibility: Compatibility,
    pub random_access: bool,
    pub hint: HintChoice,
}

#[derive(Debug, Clone)]
pub struct PackedArchive {
    pub files: Vec<NamedBytes>,
    pub report: BuildReport,
}

pub fn pack(inputs: &[PackInput], request: &PackRequest) -> Result<PackedArchive, String> {
    if inputs.is_empty() {
        return Err("请先选择需要打包的文件".to_string());
    }

    let archive_name = normalize_archive_name(&request.archive_name)?;
    if request.volume_count == 0 {
        return Err("卷数至少为 1".to_string());
    }
    let volume_names = make_volume_names(&archive_name, request.volume_count);
    let mut builder = ArchiveBuilder::with_options(PackOptions {
        volume_names: volume_names.clone(),
        alignment: request.alignment,
        compatibility: request.compatibility,
        ..PackOptions::default()
    });

    for input in inputs {
        if input.path.trim().is_empty() {
            return Err("归档内路径不能为空".to_string());
        }
        if input.volume >= request.volume_count {
            return Err(format!(
                "{} 指定了卷 {}，但当前只有 {} 个卷",
                input.path, input.volume, request.volume_count
            ));
        }
        if request.compression == Compression::Zero && input.bytes.iter().any(|byte| *byte != 0) {
            return Err(format!(
                "{} 含有非零数据，不能使用 zero 存储方式",
                input.path
            ));
        }

        let mut options = EntryOptions::new()
            .compression(request.compression)
            .volume(input.volume)
            .random_access(request.random_access);
        if let Some(hint) = content_hint(request.hint, &input.path) {
            options = options.content_hint(hint);
        }
        builder
            .add_bytes(&input.path, input.bytes.clone(), options)
            .map_err(|error| format!("无法添加 {}：{error}", input.path))?;
    }

    let mut sink = MemoryVolumeSink::default();
    let report = builder
        .write_to_sink(&mut sink)
        .map_err(|error| format!("创建归档失败：{error}"))?;
    let mut volumes = sink.into_volumes();
    let files = volume_names
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let id = u16::try_from(index).map_err(|_| "分卷数量过多".to_string())?;
            let bytes = volumes
                .remove(&id)
                .ok_or_else(|| format!("编码器没有生成卷 {id}"))?;
            Ok(NamedBytes { name, bytes })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(PackedArchive { files, report })
}

fn match_auxiliary_volumes(
    files: &[NamedBytes],
    main_index: usize,
    expected_names: &[String],
) -> Result<Vec<(u16, Vec<u8>)>, String> {
    expected_names
        .iter()
        .enumerate()
        .map(|(index, expected)| {
            let expected = base_name(expected);
            let selected = files
                .iter()
                .enumerate()
                .find(|(file_index, file)| {
                    *file_index != main_index
                        && base_name(&file.name).eq_ignore_ascii_case(expected)
                })
                .map(|(_, file)| file)
                .ok_or_else(|| format!("缺少分卷：{expected}"))?;
            let id = u16::try_from(index + 1).map_err(|_| "分卷数量过多".to_string())?;
            Ok((id, selected.bytes.clone()))
        })
        .collect()
}

fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn download_name(path: &str) -> String {
    let name = base_name(path);
    if name.is_empty() {
        "extracted.bin".to_string()
    } else {
        name.to_string()
    }
}

fn normalize_archive_name(name: &str) -> Result<String, String> {
    let name = base_name(name.trim());
    if name.is_empty() || name == "." || name == ".." {
        return Err("输出文件名无效".to_string());
    }
    if name.to_ascii_lowercase().ends_with(".dz") {
        Ok(name.to_string())
    } else {
        Ok(format!("{name}.dz"))
    }
}

fn make_volume_names(main_name: &str, count: u16) -> Vec<String> {
    let stem = &main_name[..main_name.len() - 3];
    (0..count)
        .map(|index| {
            if index == 0 {
                main_name.to_string()
            } else {
                format!("{stem}{index}.dz")
            }
        })
        .collect()
}

fn content_hint(choice: HintChoice, path: &str) -> Option<ContentHint> {
    match choice {
        HintChoice::Auto => match Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("mp3") => Some(ContentHint::Mp3),
            Some("jpg" | "jpeg") => Some(ContentHint::Jpeg),
            _ => None,
        },
        HintChoice::None => None,
        HintChoice::Mp3 => Some(ContentHint::Mp3),
        HintChoice::Jpeg => Some(ContentHint::Jpeg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_pack_open_extract_and_verify() {
        let inputs = vec![
            PackInput {
                path: "Data/hello.txt".to_string(),
                bytes: b"hello dzip".to_vec(),
                volume: 0,
            },
            PackInput {
                path: "empty.bin".to_string(),
                bytes: Vec::new(),
                volume: 1,
            },
        ];
        let packed = pack(
            &inputs,
            &PackRequest {
                archive_name: "test".to_string(),
                volume_count: 2,
                alignment: 16,
                compression: Compression::Copy,
                compatibility: Compatibility::Dzip113,
                random_access: false,
                hint: HintChoice::Auto,
            },
        )
        .unwrap();

        assert_eq!(packed.files[0].name, "test.dz");
        assert_eq!(packed.files[1].name, "test1.dz");
        assert_eq!(packed.report.entries, 2);
        let archive = LoadedArchive::open(packed.files).unwrap();
        assert_eq!(archive.entries().len(), 2);
        assert_eq!(
            archive.extract_entry(EntryId(0)).unwrap().bytes,
            b"hello dzip"
        );
        assert_eq!(archive.verify().unwrap(), (2, 10));
    }

    #[test]
    fn zero_storage_rejects_non_zero_input() {
        let error = pack(
            &[PackInput {
                path: "not-zero.bin".to_string(),
                bytes: vec![0, 1],
                volume: 0,
            }],
            &PackRequest {
                archive_name: "bad.dz".to_string(),
                volume_count: 1,
                alignment: 0,
                compression: Compression::Zero,
                compatibility: Compatibility::Dzip113,
                random_access: false,
                hint: HintChoice::None,
            },
        )
        .unwrap_err();
        assert!(error.contains("非零数据"));
    }
}
