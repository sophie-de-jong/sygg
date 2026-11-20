//! Error types shared by the parser, rewrite loader and engine.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} (column {column})")]
pub struct ParseError {
    pub message: String,
    pub column: usize,
}

impl ParseError {
    pub fn new(message: impl Into<String>, column: usize) -> Self {
        Self {
            message: message.into(),
            column,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AlgebraError {
    #[error(transparent)]
    Parse(#[from] ParseError),

    #[error("{path}:{line}: {message}")]
    RuleFile {
        path: String,
        line: usize,
        message: String,
    },

    #[error("rewrite rule error: {0}")]
    Rule(String),

    #[error("{0}")]
    Message(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<String> for AlgebraError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

impl From<&str> for AlgebraError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AlgebraError>;
