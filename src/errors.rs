//! Error types for seismotail.
//!
//! Uses `thiserror` for library-style error definitions.

use thiserror::Error;

/// Errors that can occur in seismotail operations.
#[derive(Error, Debug)]
pub enum SeismotailError {
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON parsing failed
    #[error("Failed to parse JSON: {0}")]
    Parse(#[from] serde_json::Error),

    /// API returned an error status
    #[error("USGS API error (HTTP {status}): {message}")]
    Api { status: u16, message: String },

    /// Invalid response structure
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// Event validation failed
    #[error("Invalid event data: {0}")]
    Validation(String),
}
