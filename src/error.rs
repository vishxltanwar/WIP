//! Unified error types for Rice Studio

use std::fmt;

#[derive(Debug)]
#[allow(dead_code)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    Image(image::ImageError),
    Parse(String),
    System(String),
    Config(String),
    NotFound(String),
    InvalidInput(String),
    Font(String),
    Gpu(String),
    Apply(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Json(e) => write!(f, "JSON error: {e}"),
            Error::Image(e) => write!(f, "Image error: {e}"),
            Error::Parse(e) => write!(f, "Parse error: {e}"),
            Error::System(e) => write!(f, "System error: {e}"),
            Error::Config(e) => write!(f, "Config error: {e}"),
            Error::NotFound(e) => write!(f, "Not found: {e}"),
            Error::InvalidInput(e) => write!(f, "Invalid input: {e}"),
            Error::Font(e) => write!(f, "Font error: {e}"),
            Error::Gpu(e) => write!(f, "GPU error: {e}"),
            Error::Apply(e) => write!(f, "Apply error: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::Image(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

impl From<image::ImageError> for Error {
    fn from(e: image::ImageError) -> Self {
        Error::Image(e)
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(e: std::num::ParseIntError) -> Self {
        Error::Parse(e.to_string())
    }
}

impl From<std::time::SystemTimeError> for Error {
    fn from(e: std::time::SystemTimeError) -> Self {
        Error::System(e.to_string())
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Error::Parse(e.to_string())
    }
}

impl From<std::string::FromUtf16Error> for Error {
    fn from(e: std::string::FromUtf16Error) -> Self {
        Error::Parse(e.to_string())
    }
}

// windows_sys::core::Error doesn't exist in this version; using string conversion instead

#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, Error>;