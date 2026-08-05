//! Parser for the original dzip.exe DCL build language.

mod directives;
mod lexer;
mod model;
mod number;
mod parser;

pub use model::{ConfigError, DclConfig, FileEntry, GlobalOptions, Result};
pub use parser::{parse_config, parse_config_with_commands};

#[cfg(test)]
mod tests;
