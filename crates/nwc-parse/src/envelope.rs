//! File-envelope handling: detect `.nwz` zip wrappers and `[NWZ]\0`
//! zlib-compressed bodies, return uncompressed NWC bytes ready for header
//! parsing.

use std::io::Read;

use flate2::read::ZlibDecoder;

use crate::error::NwcError;
use crate::report::{ConversionReport, Severity};

/// Magic bytes at the start of an NWC-2.x compressed body.
pub const NWZ_MAGIC: &[u8] = b"[NWZ]\0";
/// Magic at the start of an NWC-1.x uncompressed body. Also appears at the
/// start of every inflated 2.x body, after the envelope is stripped.
pub const NW_ARTWARE_MAGIC: &[u8] = b"[NoteWorthy ArtWare]";
/// Local-file-header signature for a zip / `.nwz` archive.
pub const ZIP_LOCAL_HEADER: &[u8] = b"PK\x03\x04";

pub fn unwrap(bytes: &[u8], report: &mut ConversionReport) -> Result<Vec<u8>, NwcError> {
    if bytes.starts_with(ZIP_LOCAL_HEADER) {
        return unwrap_nwz(bytes, report);
    }
    if bytes.starts_with(NWZ_MAGIC) {
        return inflate_after_magic(bytes, report);
    }
    if bytes.starts_with(NW_ARTWARE_MAGIC) {
        // NWC 1.x uncompressed: pass-through.
        return Ok(bytes.to_vec());
    }
    Err(NwcError::NotNwc {
        header: bytes
            .iter()
            .take(8)
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" "),
    })
}

fn unwrap_nwz(bytes: &[u8], report: &mut ConversionReport) -> Result<Vec<u8>, NwcError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor)?;
    // .nwz archives in the wild contain exactly one entry, but be defensive.
    let mut chosen: Option<usize> = None;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let name = entry.name().to_lowercase();
        if name.ends_with(".nwc") {
            chosen = Some(i);
            break;
        }
    }
    let idx = chosen.unwrap_or(0);
    let mut entry = zip.by_index(idx)?;
    if entry.is_dir() {
        return Err(NwcError::Malformed {
            offset: 0,
            message: "no file entry in .nwz archive".into(),
        });
    }
    let mut inner = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut inner)
        .map_err(|e| NwcError::Malformed {
            offset: 0,
            message: format!("reading inner zip entry: {e}"),
        })?;
    drop(entry);

    // Recurse: the inner file may itself be `[NWZ]\0`-compressed or 1.x raw.
    report.push(Severity::Info, 0, "unwrapped .nwz archive");
    unwrap(&inner, report)
}

fn inflate_after_magic(bytes: &[u8], report: &mut ConversionReport) -> Result<Vec<u8>, NwcError> {
    let body = &bytes[NWZ_MAGIC.len()..];
    let mut dec = ZlibDecoder::new(body);
    let mut out = Vec::with_capacity(bytes.len() * 4);
    dec.read_to_end(&mut out).map_err(|e| NwcError::Zlib(e.to_string()))?;
    report.push(Severity::Info, 0, format!("inflated NWZ body: {} -> {} bytes", body.len(), out.len()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_magic() {
        let mut r = ConversionReport::default();
        let err = unwrap(b"GARBAGE!", &mut r).unwrap_err();
        assert!(matches!(err, NwcError::NotNwc { .. }));
    }
}
