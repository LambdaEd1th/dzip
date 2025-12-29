use clap::{Parser, Subcommand};
use std::path::PathBuf;

use dzip_cli::{create_default_registry, do_pack, do_unpack};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Unpack a .dz archive
    Unpack {
        /// Input .dz file
        input: PathBuf,

        /// Optional output directory (default: same as input filename)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Keep raw data if decompression fails or for proprietary chunks
        #[arg(short, long)]
        keep_raw: bool,
    },
    /// Pack a directory into a .dz archive based on a .toml config
    Pack {
        /// Input .toml configuration file
        config: PathBuf,
    },
}

fn main() {
    // Initialize the logger based on the RUST_LOG environment variable
    env_logger::init();

    let cli = Cli::parse();
    let registry = create_default_registry();

    // Execute the command and capture the result
    let result = match &cli.command {
        Commands::Unpack {
            input,
            output,
            keep_raw,
        } => do_unpack(input, output.clone(), *keep_raw, &registry),
        Commands::Pack { config } => do_pack(config, &registry),
    };

    // Handle errors gracefully without returning Result<()> which might print ugly stack traces
    if let Err(e) = result {
        // Print the error in red (ANSI escape code \x1b[31m)
        // {:#} prints the alternative formatting for anyhow errors (the cause chain)
        eprintln!("\x1b[31mError:\x1b[0m {:#}", e);
        std::process::exit(1);
    }
}
