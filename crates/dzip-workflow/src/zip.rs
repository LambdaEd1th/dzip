use crate::error::{Result, WorkflowError};
use crate::model::ExtractedFile;
use crc32fast::Hasher;
use std::io::Write;

pub fn make_store_zip(files: &[ExtractedFile]) -> Result<Vec<u8>> {
    if files.len() > u16::MAX as usize {
        return Err(WorkflowError::invalid(
            "ZIP export supports at most 65535 files",
        ));
    }
    struct CentralEntry {
        name: Vec<u8>,
        crc: u32,
        size: u32,
        offset: u32,
    }

    let mut output = Vec::new();
    let mut central = Vec::with_capacity(files.len());
    for file in files {
        let name = file.path.replace('\\', "/").into_bytes();
        let name_len = u16::try_from(name.len())
            .map_err(|_| WorkflowError::invalid(format!("ZIP path is too long: {}", file.path)))?;
        let size = u32::try_from(file.bytes.len()).map_err(|_| {
            WorkflowError::invalid(format!("file exceeds ZIP32 limit: {}", file.path))
        })?;
        let offset = u32::try_from(output.len())
            .map_err(|_| WorkflowError::invalid("ZIP export exceeds the ZIP32 limit"))?;
        let mut hasher = Hasher::new();
        hasher.update(&file.bytes);
        let crc = hasher.finalize();

        write_u32(&mut output, 0x0403_4b50)?;
        write_u16(&mut output, 20)?;
        write_u16(&mut output, 0x0800)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u32(&mut output, crc)?;
        write_u32(&mut output, size)?;
        write_u32(&mut output, size)?;
        write_u16(&mut output, name_len)?;
        write_u16(&mut output, 0)?;
        output.extend_from_slice(&name);
        output.extend_from_slice(&file.bytes);
        central.push(CentralEntry {
            name,
            crc,
            size,
            offset,
        });
    }

    let central_offset = u32::try_from(output.len())
        .map_err(|_| WorkflowError::invalid("ZIP export exceeds the ZIP32 limit"))?;
    for entry in &central {
        write_u32(&mut output, 0x0201_4b50)?;
        write_u16(&mut output, 20)?;
        write_u16(&mut output, 20)?;
        write_u16(&mut output, 0x0800)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u32(&mut output, entry.crc)?;
        write_u32(&mut output, entry.size)?;
        write_u32(&mut output, entry.size)?;
        write_u16(&mut output, entry.name.len() as u16)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u16(&mut output, 0)?;
        write_u32(&mut output, 0)?;
        write_u32(&mut output, entry.offset)?;
        output.extend_from_slice(&entry.name);
    }
    let central_size = u32::try_from(output.len())
        .map_err(|_| WorkflowError::invalid("ZIP export exceeds the ZIP32 limit"))?
        .saturating_sub(central_offset);
    write_u32(&mut output, 0x0605_4b50)?;
    write_u16(&mut output, 0)?;
    write_u16(&mut output, 0)?;
    write_u16(&mut output, central.len() as u16)?;
    write_u16(&mut output, central.len() as u16)?;
    write_u32(&mut output, central_size)?;
    write_u32(&mut output, central_offset)?;
    write_u16(&mut output, 0)?;
    Ok(output)
}

fn write_u16(output: &mut Vec<u8>, value: u16) -> Result<()> {
    output.write_all(&value.to_le_bytes()).map_err(Into::into)
}

fn write_u32(output: &mut Vec<u8>, value: u32) -> Result<()> {
    output.write_all(&value.to_le_bytes()).map_err(Into::into)
}
