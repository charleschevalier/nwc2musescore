//! NoteWorthy Composer (.nwc / .nwz) binary decoder.
//!
//! Entry point: [`parse_bytes`].

#![forbid(unsafe_code)]

use std::path::Path;

pub mod cursor;
pub mod envelope;
pub mod error;
pub mod header;
pub mod report;
pub mod v2;

pub use error::NwcError;
pub use report::{ConversionReport, Diagnostic, Severity};

use nwc_model::Score;

/// Parse a `.nwc` or `.nwz` byte stream into a [`Score`].
pub fn parse_bytes(bytes: &[u8]) -> Result<(Score, ConversionReport), NwcError> {
    let mut report = ConversionReport::default();
    let body = envelope::unwrap(bytes, &mut report)?;
    let header = header::parse_header(&body, &mut report)?;
    match header.version.major {
        2 => {
            let score = v2::parse_body(&body, &header, &mut report)?;
            Ok((score, report))
        }
        other => Err(NwcError::UnsupportedVersion {
            major: other,
            minor: header.version.minor,
        }),
    }
}

/// Convenience wrapper around [`parse_bytes`] for paths.
pub fn parse_file(path: &Path) -> Result<(Score, ConversionReport), NwcError> {
    let bytes = std::fs::read(path).map_err(|e| NwcError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    parse_bytes(&bytes)
}
