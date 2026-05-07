//! Emit MusicXML 4.0 (partwise) from a [`nwc_model::Score`].

#![forbid(unsafe_code)]

mod context;
mod emit;
mod measures;

use thiserror::Error;

use nwc_model::Score;

#[derive(Debug, Error)]
pub enum WriteError {
    #[error("XML serialization error: {0}")]
    Xml(String),
    #[error("score has no staves")]
    EmptyScore,
}

#[derive(Debug, Clone)]
pub struct WriteOptions {
    /// "3.1" or "4.0". Default: "4.0".
    pub musicxml_version: String,
    /// Pretty-print with this many spaces of indent. None = compact.
    pub indent: Option<u8>,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self { musicxml_version: "4.0".into(), indent: Some(2) }
    }
}

/// Emit a MusicXML partwise document as a UTF-8 string.
pub fn write(score: &Score, opts: &WriteOptions) -> Result<String, WriteError> {
    let bytes = emit::write_bytes(score, opts)?;
    String::from_utf8(bytes).map_err(|e| WriteError::Xml(e.to_string()))
}

/// Emit a MusicXML partwise document as raw UTF-8 bytes.
pub fn write_to_bytes(score: &Score, opts: &WriteOptions) -> Result<Vec<u8>, WriteError> {
    emit::write_bytes(score, opts)
}
