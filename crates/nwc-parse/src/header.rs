//! Inflated NWC header: signature blocks, version, score-level metadata.
//!
//! The inflated body of a NWC file looks like this for the 2.x family:
//!
//! ```text
//! [NoteWorthy ArtWare]\0\0\0
//! [NoteWorthy Composer]\0
//! <product u8> <version u16 LE> <padding [u8;3]>
//! <author cstr> <title cstr> ... <copyright1 cstr> <copyright2 cstr>
//! <comments cstr>
//! ... page-setup, font-table, staff blocks ...
//! ```
//!
//! NWC 1.x uncompressed bodies start directly with `[NoteWorthy ArtWare]`.

use nwc_model::{ScoreInfo, SourceVersion};

use crate::cursor::Cursor;
use crate::envelope::NW_ARTWARE_MAGIC;
use crate::error::NwcError;
use crate::report::{ConversionReport, Severity};

/// Result of parsing the score-level header. Returned alongside the cursor
/// position pointing at the start of the staff section.
#[derive(Debug, Clone)]
pub struct Header {
    pub version: SourceVersion,
    pub product: u8,
    pub info: ScoreInfo,
    /// Byte offset (within the inflated body) where staves begin.
    pub staves_offset: usize,
    /// How many staves are declared.
    pub staff_count: u16,
    /// Inflated body length, for diagnostics.
    pub body_len: usize,
}

const NW_COMPOSER_MAGIC: &[u8] = b"[NoteWorthy Composer]\0";

/// Parse just enough of the body header to identify the version. Does NOT
/// consume staff blocks; the actual staff parsing lives in `v2::parse_body`
/// (or `v1::parse_body`) and is given the full inflated body plus this
/// header's `staves_offset` cursor position.
pub fn parse_header(body: &[u8], report: &mut ConversionReport) -> Result<Header, NwcError> {
    let mut cur = Cursor::new(body);

    // Inflated bodies (and 1.x raw bodies) always start with the ArtWare tag.
    if !body.starts_with(NW_ARTWARE_MAGIC) {
        return Err(NwcError::Malformed {
            offset: 0,
            message: "missing [NoteWorthy ArtWare] marker".into(),
        });
    }
    cur.skip(NW_ARTWARE_MAGIC.len(), "artware marker")?;
    // Three NUL bytes after the ArtWare marker on 2.x bodies. 1.x bodies may
    // lack them; tolerate.
    while cur.peek_bytes(1).map(|b| b[0] == 0).unwrap_or(false) {
        cur.skip(1, "artware padding")?;
    }

    // 2.x bodies have a second [NoteWorthy Composer]\0 marker; 1.x do not.
    let has_composer_marker = cur
        .peek_bytes(NW_COMPOSER_MAGIC.len())
        .map(|b| b == NW_COMPOSER_MAGIC)
        .unwrap_or(false);

    if !has_composer_marker {
        // NWC 1.x — version is a single byte after the ArtWare block.
        let version_byte = cur.read_u8("v1 version")?;
        let version = SourceVersion {
            major: 1,
            minor: version_byte,
            raw: version_byte as u16,
        };
        report.push(
            Severity::Info,
            cur.pos(),
            format!("detected NWC 1.x file, minor=0x{:02x}", version_byte),
        );
        // Score info parsing for 1.x is not implemented in M1.
        return Ok(Header {
            version,
            product: 0,
            info: ScoreInfo::default(),
            staves_offset: cur.pos(),
            staff_count: 0,
            body_len: body.len(),
        });
    }

    cur.skip(NW_COMPOSER_MAGIC.len(), "composer marker")?;
    let product = cur.read_u8("product code")?;
    let version_raw = cur.read_u16_le("version")?;
    let version = SourceVersion {
        major: (version_raw >> 8) as u8,
        minor: (version_raw & 0xff) as u8,
        raw: version_raw,
    };
    // 3 bytes of padding / reserved.
    cur.skip(3, "header padding")?;

    // NWC 2.x score-info layout (verified against 2.01 corpus):
    //   author cstr
    //   licence-tag cstr  (registration token, ignored)
    //   10 raw bytes      (always 8 zeros + u16 LE 0x0010)
    //   title cstr
    //   subtitle cstr
    //   copyright1 cstr
    //   copyright2 cstr
    //   comments cstr
    let author = cur.read_cstr_lossy("author")?;
    let licence_tag = cur.read_cstr_lossy("licence_tag")?;
    let _ = licence_tag;
    cur.skip(10, "score-info reserved")?;
    let title = cur.read_cstr_lossy("title")?;
    let subtitle = cur.read_cstr_lossy("subtitle")?;
    let copyright1 = cur.read_cstr_lossy("copyright1")?;
    let copyright2 = cur.read_cstr_lossy("copyright2")?;
    let comments = cur.read_cstr_lossy("comments")?;

    // Subtitle and lyricist are different concepts in NWC; fold subtitle
    // into copyright lines is wrong, but treating it as a lyricist is also
    // wrong. For M1 store it as the first comment line if comments is empty.
    let merged_comments = match (nonempty(comments), nonempty(subtitle.clone())) {
        (Some(c), Some(s)) => Some(format!("{s}\n{c}")),
        (Some(c), None) => Some(c),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    };
    let _ = subtitle;

    let info = ScoreInfo {
        title: nonempty(title),
        author: nonempty(author),
        lyricist: None,
        copyright: [copyright1, copyright2]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect(),
        comments: merged_comments,
    };

    // Note: staff_count and the offset where staves begin are determined by
    // the version-specific body parser; we surface what we know here.
    Ok(Header {
        version,
        product,
        info,
        staves_offset: cur.pos(),
        staff_count: 0,
        body_len: body.len(),
    })
}

fn nonempty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
