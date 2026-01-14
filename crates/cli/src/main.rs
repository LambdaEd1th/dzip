mod args;
mod fs;

use clap::Parser;
use log::{LevelFilter, info};
use std::fs as std_fs;
use std::path::{MAIN_SEPARATOR, Path};

use args::{Cli, Commands};
use fs::{FsPackSink, FsPackSource, FsUnpackSink, FsUnpackSource, normalize_path};

use dzip_core::{Result, do_list, do_pack, do_unpack, model::Config};

fn main() {
    let cli = Cli::parse();

    let mut builder = env_logger::Builder::from_default_env();
    if std::env::var("RUST_LOG").is_err() {
        builder.filter(None, LevelFilter::Info);
    }
    if cli.verbose {
        builder.filter(None, LevelFilter::Debug);
    }
    builder.init();

    let run = || -> Result<()> {
        match &cli.command {
            Commands::Unpack {
                input,
                output,
                keep_raw,
            } => {
                let base_dir = input.parent().unwrap_or(Path::new(".")).to_path_buf();
                let file_name = input
                    .file_name()
                    .ok_or_else(|| dzip_core::DzipError::Generic("Invalid filename".into()))?
                    .to_string_lossy()
                    .to_string();

                let source = FsUnpackSource {
                    base_path: base_dir,
                    main_file_name: file_name,
                };

                let base_stem = input.file_stem().unwrap().to_string_lossy();
                let out_dir = output
                    .clone()
                    .unwrap_or_else(|| std::path::PathBuf::from(base_stem.to_string()));

                let sink = FsUnpackSink {
                    output_dir: out_dir,
                };

                // Get Config from Core (Paths are neutral '/')
                let mut config = do_unpack(&source, &sink, *keep_raw)?;

                // Localize paths for the Config file (CLI Responsibility)
                let sep = MAIN_SEPARATOR.to_string();
                if sep != "/" {
                    for file in &mut config.files {
                        file.path = file.path.replace('/', &sep);
                        file.directory = file.directory.replace('/', &sep);
                    }
                }

                let toml_str =
                    toml::to_string_pretty(&config).map_err(dzip_core::DzipError::TomlSer)?;
                let config_path = format!("{}.toml", base_stem);
                std_fs::write(&config_path, toml_str)
                    .map_err(|e| dzip_core::DzipError::IoContext(config_path.clone(), e))?;

                info!("Config saved to {}", config_path);
                Ok(())
            }

            Commands::Pack { config } => {
                let toml_content = std_fs::read_to_string(config).map_err(|e| {
                    dzip_core::DzipError::IoContext(config.display().to_string(), e)
                })?;

                // 1. Deserialize
                let mut core_config: Config =
                    toml::from_str(&toml_content).map_err(dzip_core::DzipError::TomlDe)?;

                // 2. Sanitize paths (CLI Responsibility)
                for file in &mut core_config.files {
                    let raw_path = std::path::Path::new(&file.path);
                    let normalized = normalize_path(raw_path);
                    file.path = normalized.to_string_lossy().to_string();
                }

                let config_parent = config.parent().unwrap_or(Path::new(".")).to_path_buf();
                let base_name = config.file_stem().unwrap().to_string_lossy().to_string();

                let source = FsPackSource {
                    root_dir: config_parent.join(&base_name),
                };
                let sink = Box::new(FsPackSink {
                    output_dir: config_parent,
                    base_name: base_name.clone(),
                });

                do_pack(core_config, base_name, sink, &source)
            }

            Commands::List { input } => {
                let base_dir = input.parent().unwrap_or(Path::new(".")).to_path_buf();
                let file_name = input.file_name().unwrap().to_string_lossy().to_string();
                let source = FsUnpackSource {
                    base_path: base_dir,
                    main_file_name: file_name,
                };

                let entries = do_list(&source)?;

                println!();
                println!("{:<15} | {:<8} | Path", "Size (Bytes)", "Chunks");
                println!("{:-<15}-|-{:-<8}-|--------------------------------", "", "");
                for entry in &entries {
                    println!(
                        "{:<15} | {:<8} | {}",
                        entry.original_size, entry.chunk_count, entry.path
                    );
                }
                println!("\nTotal files: {}", entries.len());
                Ok(())
            }
        }
    };

    if let Err(e) = run() {
        eprintln!("\x1b[31mError:\x1b[0m {:#}", e);
        std::process::exit(1);
    }
}
