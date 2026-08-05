use crate::error::{Result, WorkflowError};
use crate::model::{BuildPlan, NamedBytes};
use dzip::{ArchiveBuilder, BuildReport, EntryOptions, MemoryVolumeSink, PackOptions};
use std::path::Path;

pub fn build_archive(plan: BuildPlan) -> Result<Vec<NamedBytes>> {
    let builder = builder_from_plan(plan)?;

    let mut sink = MemoryVolumeSink::default();
    let report = builder.write_to_sink(&mut sink)?;
    (0..report.volumes)
        .map(|id| {
            let id = u16::try_from(id)
                .map_err(|_| WorkflowError::invalid("archive has more than 65535 volumes"))?;
            Ok(NamedBytes {
                name: sink
                    .name(id)
                    .ok_or_else(|| WorkflowError::invalid(format!("volume {id} has no name")))?
                    .to_string(),
                bytes: sink
                    .volume(id)
                    .ok_or_else(|| WorkflowError::invalid(format!("volume {id} has no data")))?
                    .to_vec(),
            })
        })
        .collect()
}

pub fn write_archive_to_directory(
    plan: BuildPlan,
    output_directory: impl AsRef<Path>,
) -> Result<BuildReport> {
    builder_from_plan(plan)?
        .write_to_directory(output_directory)
        .map_err(Into::into)
}

fn builder_from_plan(plan: BuildPlan) -> Result<ArchiveBuilder> {
    if plan.entries.is_empty() {
        return Err(WorkflowError::invalid(
            "a build plan must contain at least one entry",
        ));
    }
    let minimum_volume_count = plan
        .entries
        .iter()
        .map(|entry| usize::from(entry.volume) + 1)
        .max()
        .unwrap_or(1);
    let volume_names = if plan.volume_names.is_empty() {
        archive_volume_names(&plan.archive_name, minimum_volume_count)?
    } else {
        if plan.volume_names.len() < minimum_volume_count {
            return Err(WorkflowError::invalid(format!(
                "entry refers to volume {}, but the plan declares only {} volume(s)",
                minimum_volume_count - 1,
                plan.volume_names.len()
            )));
        }
        plan.volume_names
    };
    let mut builder = ArchiveBuilder::with_options(PackOptions {
        volume_names,
        alignment: plan.alignment,
        dz: plan.dz_options.to_options()?,
    });
    for entry in plan.entries {
        let path = entry.path.trim();
        if path.is_empty() {
            return Err(WorkflowError::invalid(
                "archive entry paths cannot be empty",
            ));
        }
        builder.add_bytes(
            path,
            entry.bytes,
            EntryOptions::new()
                .compression(entry.encoding.compression)
                .volume(entry.volume)
                .raw_flags(entry.raw_flags.unwrap_or_else(|| entry.encoding.to_flags())),
        )?;
    }
    Ok(builder)
}

pub fn archive_volume_names(archive_name: &str, count: usize) -> Result<Vec<String>> {
    if count == 0 || count > u16::MAX as usize {
        return Err(WorkflowError::invalid(
            "archive volume count is outside 1..=65535",
        ));
    }
    let main_name = normalize_archive_name(archive_name);
    let lower = main_name.to_ascii_lowercase();
    let extension_len = if lower.ends_with(".dzip") { 5 } else { 3 };
    let stem = main_name[..main_name.len() - extension_len].to_string();
    let mut names = Vec::with_capacity(count);
    names.push(main_name);
    for index in 1..count {
        names.push(format!("{stem}.{index:03}"));
    }
    Ok(names)
}

pub fn normalize_archive_name(value: &str) -> String {
    let value = value.trim();
    let name = if value.is_empty() { "archive" } else { value };
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".dz") || lower.ends_with(".dzip") {
        name.to_string()
    } else {
        format!("{name}.dz")
    }
}
