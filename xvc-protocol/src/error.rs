use std::{
    error::Error,
    fmt::{self, Display},
    io,
    num::ParseIntError,
    str::Utf8Error,
};

use crate::codec::ParseErr;

/// Errors that may occur when reading a message from a stream.
#[derive(Debug)]
pub enum ReadError {
    IoError(io::Error),
    InvalidCommand(String),
    InvalidFormat(String),
    TooManyBytes { max: usize, need: usize },
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

impl From<ParseVersionError> for ReadError {
    fn from(value: ParseVersionError) -> Self {
        Self::InvalidFormat(format!("{}", value))
    }
}

impl From<crate::codec::ParseErr> for ReadError {
    fn from(value: crate::codec::ParseErr) -> Self {
        match value {
            ParseErr::Incomplete => ReadError::IoError(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete message",
            )),
            ParseErr::InvalidCommand(items) => {
                ReadError::InvalidCommand(String::from_utf8_lossy(&items).to_string())
            }
            ParseErr::TooManyBytes { max, got } => ReadError::TooManyBytes { max, need: got },
            ParseErr::Utf8Error(utf8_error) => {
                ReadError::InvalidFormat(format!("Invalid utf8: {}", utf8_error))
            }
            ParseErr::ParseIntError(parse_int_error) => {
                ReadError::InvalidFormat(format!("Invalid integer: {}", parse_int_error))
            }
            ParseErr::ParseVersionError(parse_version_error) => ReadError::InvalidFormat(format!(
                "Could not parse version: {}",
                parse_version_error
            )),
        }
    }
}

impl Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::IoError(error) => write!(f, "{}", error),
            ReadError::InvalidCommand(cmd) => write!(f, "Received invalid command {}", cmd),
            ReadError::InvalidFormat(format) => write!(f, "{}", format),
            ReadError::TooManyBytes { max, need: got } => {
                write!(f, "Message too large! Maximum is {}, but got {}", max, got)
            }
        }
    }
}

impl Error for ReadError {}

/// Errors that may occur when parsing a Version.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum ParseVersionError {
    MissingDot,
    ParseInt(ParseIntError),
}

impl From<ParseIntError> for ParseVersionError {
    fn from(value: ParseIntError) -> Self {
        ParseVersionError::ParseInt(value)
    }
}

impl Display for ParseVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseVersionError::MissingDot => write!(f, "Missing dot in version"),
            ParseVersionError::ParseInt(parse_err) => write!(f, "{}", parse_err),
        }
    }
}

impl Error for ParseVersionError {}
