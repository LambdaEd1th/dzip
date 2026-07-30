use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DzError {
    message: String,
}

impl DzError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    // Kept as a temporary source-compatibility constructor while the native
    // codec is separated from the archive crate.
    #[allow(non_snake_case)]
    pub(crate) fn InvalidDz(message: String) -> Self {
        Self::new(message)
    }
}

impl fmt::Display for DzError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DzError {}

pub type Result<T> = std::result::Result<T, DzError>;
