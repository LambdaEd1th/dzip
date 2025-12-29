use clap::{Parser, Subcommand};
use log::LevelFilter;
use std::path::PathBuf;

use dzip_cli::{create_default_registry, do_pack, do_unpack};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable verbose logging (Debug level) for troubleshooting.
    /// This is a global argument usable with any subcommand.
    #[arg(short, long, global = true)]
    verbose: bool,

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
    // 1. Parse CLI arguments first to check for the --verbose flag
    let cli = Cli::parse();

    // 2. Initialize the logger builder
    let mut builder = env_logger::Builder::from_default_env();

    // Set the default log level to 'Info' if RUST_LOG environment variable is not set.
    // This ensures users see standard output messages by default.
    if std::env::var("RUST_LOG").is_err() {
        builder.filter(None, LevelFilter::Info);
    }

    // If --verbose is passed, force the log level to 'Debug'.
    // This will show detailed logs from the application and dependencies.
    if cli.verbose {
        builder.filter(None, LevelFilter::Debug);
    }

    builder.init();

    // 3. Create the codec registry
    let registry = create_default_registry();

    // 4. Execute the command logic
    let result = match &cli.command {
        Commands::Unpack {
            input,
            output,
            keep_raw,
        } => do_unpack(input, output.clone(), *keep_raw, &registry),
        Commands::Pack { config } => do_pack(config, &registry),
    };

    // 5. Handle errors gracefully (Optimized Error Reporting)
    if let Err(e) = result {
        // Print errors in red using ANSI escape codes for better visibility
        // {:#} prints the alternate view (causal chain) of the error
        eprintln!("\x1b[31mError:\x1b[0m {:#}", e);
        std::process::exit(1);
    }
}