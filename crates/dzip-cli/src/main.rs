use clap::{Parser, Subcommand, ValueEnum};
use dzip::Result;
use dzip::{Compression, ContentHint};
use log::info;

mod commands;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable verbose logging/output
    #[arg(short, long, global = true, conflicts_with = "quiet")]
    verbose: bool,

    /// Suppress informational command output
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract a Dzip archive
    Extract {
        /// The Dzip archive to extract
        input: String,
        /// Output directory; defaults to the archive file stem
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Build archives from a Dzip .dcl configuration
    Build {
        /// Dzip .dcl configuration
        input: String,
        /// The output directory
        #[arg(short, long, default_value = ".")]
        output: String,
        /// Extra dzip.exe DCL commands applied after the configuration file
        #[arg(short = 'c', long = "command")]
        command: Vec<String>,
    },
    /// Create an archive directly from one or more files
    Create {
        /// Output archive name
        archive: String,
        /// Archive-relative file paths to add
        #[arg(required = true)]
        files: Vec<String>,
        /// Directory in which to write the archive
        #[arg(short, long, default_value = ".")]
        output: String,
        /// Source search directory; may be repeated
        #[arg(short = 'D', long = "dir")]
        source_dirs: Vec<String>,
        /// Align stored chunks to this byte boundary
        #[arg(short = 'A', long = "align", default_value_t = 0)]
        alignment: u32,
        /// Compression strategy applied to every input file
        #[arg(short = 't', long = "type", value_enum, default_value_t = CompressionArg::Dz)]
        compression: CompressionArg,
        /// Start byte or percentage, for example 4096 or 25%
        #[arg(short = 's', long = "start")]
        start: Option<commands::create::Boundary>,
        /// End byte or percentage, for example 8192 or 75%
        #[arg(short = 'e', long = "end")]
        end: Option<commands::create::Boundary>,
        /// Mark entries as random-access
        #[arg(long)]
        random_access: bool,
        /// Optional MP3/JPEG content hint
        #[arg(long, value_enum)]
        content_hint: Option<ContentHintArg>,
        /// Enable native DZ common-buffer generation
        #[arg(long)]
        combuf: bool,
    },
    /// List archive contents without decompressing entries
    List {
        /// Input archive file
        input: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CompressionArg {
    Dz,
    Zlib,
    Bzip,
    Lzma,
    Zero,
    Copy,
}

impl From<CompressionArg> for Compression {
    fn from(value: CompressionArg) -> Self {
        match value {
            CompressionArg::Dz => Self::Dz,
            CompressionArg::Zlib => Self::Zlib,
            CompressionArg::Bzip => Self::Bzip,
            CompressionArg::Lzma => Self::Lzma,
            CompressionArg::Zero => Self::Zero,
            CompressionArg::Copy => Self::Copy,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ContentHintArg {
    Mp3,
    Jpeg,
}

impl From<ContentHintArg> for ContentHint {
    fn from(value: ContentHintArg) -> Self {
        match value {
            ContentHintArg::Mp3 => Self::Mp3,
            ContentHintArg::Jpeg => Self::Jpeg,
        }
    }
}

fn main() {
    let cli = Cli::parse();

    let log_level = if cli.quiet {
        "off"
    } else if cli.verbose {
        "debug"
    } else {
        "info"
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    if let Err(error) = run_modern(&cli) {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run_modern(cli: &Cli) -> Result<()> {
    match &cli.command {
        Commands::Extract { input, output } => {
            commands::extract::extract_archive(input, output.as_deref())?;
        }
        Commands::Build {
            input,
            output,
            command,
        } => {
            info!("Building from config {} to output dir {}", input, output);
            if command.is_empty() {
                commands::build::build_from_config(input, output)?;
            } else {
                commands::build::build_from_config_with_commands(input, output, command)?;
            }
        }
        Commands::Create {
            archive,
            files,
            output,
            source_dirs,
            alignment,
            compression,
            start,
            end,
            random_access,
            content_hint,
            combuf,
        } => {
            commands::create::create_archive(commands::create::CreateRequest {
                archive,
                files,
                output_dir: output,
                source_dirs,
                alignment: *alignment,
                compression: (*compression).into(),
                start: *start,
                end: *end,
                random_access: *random_access,
                content_hint: (*content_hint).map(Into::into),
                use_combuf: *combuf,
            })?;
        }
        Commands::List { input } => {
            commands::list::list_archive(input, !cli.quiet)?;
        }
    }

    Ok(())
}
