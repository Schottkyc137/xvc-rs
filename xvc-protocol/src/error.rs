use std::{
    error::Error,
    fmt::Display,
    io::{self},
    num::ParseIntError,
    str::Utf8Error,
};

/// Errors that may occur when reading a message from a stream.
#[derive(Debug)]
pub enum ReadError {
    IoError(io::Error),
    InvalidCommand(String),
    InvalidCommandPrefix(String),
    UnsupportedVersion(String),
    InvalidFormat(String),
    TooManyBytes { max: usize, got: usize },
}

impl From<io::Error> for ReadError {
    fn from(value: io::Error) -> Self {
        ReadError::IoError(value)
    }
}

impl From<Utf8Error> for ReadError {
    fn from(value: Utf8Error) -> Self {
        ReadError::InvalidFormat(format!("Invalid UTF8: {}", value))
    }
}

impl From<ParseIntError> for ReadError {
    fn from(value: ParseIntError) -> Self {
        ReadError::InvalidFormat(format!("Invalid integer: {}", value))
    }
}

impl Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::IoError(error) => write!(f, "{}", error),
            ReadError::InvalidCommand(cmd) => write!(f, "Received invalid command {}", cmd),
            ReadError::UnsupportedVersion(version) => write!(f, "Unsupported version {}", version),
            ReadError::InvalidFormat(format) => write!(f, "{}", format),
            ReadError::InvalidCommandPrefix(prefix) => {
                write!(f, "Received invalid command with prefix {}", prefix)
            }
            ReadError::TooManyBytes { max, got } => {
                write!(f, "Message too large! Maximum is {}, but gut {}", max, got)
            }
        }
    }
}

impl Error for ReadError {}
