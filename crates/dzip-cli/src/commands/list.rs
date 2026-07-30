use dzip::{Archive, Result};

pub fn list_archive(input_path: &str, show_output: bool) -> Result<()> {
    let archive = Archive::open_path(input_path)?;
    if !show_output {
        return Ok(());
    }

    println!(
        "{:<5} | {:<10} | {:<10} | {:<8} | Path",
        "Idx", "Size", "Packed", "Method"
    );
    println!(
        "{:-<5}-+-{:-<10}-+-{:-<10}-+-{:-<8}-+-{:-<20}",
        "", "", "", "", ""
    );
    for entry in archive.entries() {
        let packed: u64 = entry
            .chunk_ids()
            .iter()
            .filter_map(|id| archive.index().chunks().get(*id as usize))
            .map(|chunk| u64::from(chunk.compressed_length))
            .sum();
        println!(
            "{:<5} | {:<10} | {:<10} | {:<8} | {}",
            entry.id().0,
            entry.decompressed_size(),
            packed,
            entry.compression(),
            entry.path().display()
        );
    }
    Ok(())
}
