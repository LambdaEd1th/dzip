use dzip::Result;
use dzip_dcl as config;
use dzip_workflow::{build_plan_from_dcl, write_archive_to_directory};
use log::info;
use std::path::Path;

pub fn build_from_config(input_path: &str, output_dir: &str) -> Result<()> {
    build_from_config_with_commands(input_path, output_dir, &[])
}

pub fn build_from_config_with_commands(
    input_path: &str,
    output_dir: &str,
    commands: &[String],
) -> Result<()> {
    let config_path = Path::new(input_path);
    info!("Parsing config file: {}", config_path.display());
    let parsed = if commands.is_empty() {
        config::parse_config(config_path)
    } else {
        config::parse_config_with_commands(config_path, commands)
    };
    let config = parsed.map_err(invalid_data)?;
    let plan = build_plan_from_dcl(config).map_err(invalid_data)?;
    let report = write_archive_to_directory(plan, Path::new(output_dir)).map_err(invalid_data)?;
    info!(
        "Packed {} files into {} chunks across {} volume(s)",
        report.entries, report.chunks, report.volumes
    );
    Ok(())
}

fn invalid_data(error: impl std::fmt::Display) -> dzip::DzipError {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()).into()
}
