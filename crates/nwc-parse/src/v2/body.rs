//! NWC 2.x post-header body parser.
//!
//! Layout reverse-engineered against the NWC 2.01 corpus
//! (`/home/charles/Documents/P2LR/pistons`, 406 files):
//!
//! After the score-info header (which ends with the `comments` cstr) the
//! file continues with a fixed-shape page-setup block, a font table, then
//! a staff section.
//!
//! ```text
//! page-setup:
//!   "NY_\0"                       (page-template name? always "NY_")
//!   "F2\0"                        (constant)
//!   4 bytes (LE u16 + LE u16)     (header/footer page numbers? 0x0000 / 0x0001)
//!   margins cstr                  ("L T R B" floats, space-separated)
//!   34 bytes opaque               (page setup fields — not interpreted)
//!   u16 LE                        (always 0x0010 — possibly font slots)
//! font table:
//!   12 records, each:
//!     cstr name
//!     u8 style_flags  (1=italic, 2=bold, 3=italic+bold, …)
//!     u8 size
//!     u8 ?            (always 0)
//!     u8 ?            (always 0)
//! staff prelude:
//!   4 bytes 0xff 0x00 0x00 0x00   (constant)
//!   u16 LE staff_count
//! per-staff:
//!   name cstr
//!   group cstr  ("Standard" | "Brace" | "Bracket" | …)
//!   ... metadata + object stream ...
//! ```
//!
//! M1 implements: page-setup skip, font-table skip, staff prelude, and
//! per-staff name/group extraction. The object stream is not yet decoded;
//! it's surfaced as a single `User` object with raw bytes for diagnostics.

use nwc_model::{Score, Staff};

use crate::cursor::Cursor;
use crate::error::NwcError;
use crate::header::Header;
use crate::report::{ConversionReport, Severity};

const FONTS_IN_2X: usize = 12;
/// Opaque bytes between the margins cstr and the `font_slots` u16. Verified
/// constant length across the NWC 2.01 corpus.
const PAGE_SETUP_TAIL_BYTES: usize = 36;

pub fn parse_body(
    body: &[u8],
    header: &Header,
    report: &mut ConversionReport,
) -> Result<Score, NwcError> {
    let mut cur = Cursor::new(body);
    cur.skip(header.staves_offset, "header skip")?;

    // --- page setup ---------------------------------------------------------
    let page_template = cur.read_cstr_lossy("page_template")?;
    let _ = page_template;
    let unknown_tag = cur.read_cstr_lossy("page_setup_tag")?;
    let _ = unknown_tag;
    cur.skip(4, "page_setup_flags")?;
    let _margins = cur.read_cstr_lossy("page_margins")?;
    cur.skip(PAGE_SETUP_TAIL_BYTES, "page_setup_tail")?;
    let _font_slots = cur.read_u16_le("font_slots")?;

    // --- font table ---------------------------------------------------------
    let mut fonts = Vec::with_capacity(FONTS_IN_2X);
    for i in 0..FONTS_IN_2X {
        let name = cur.read_cstr_lossy("font_name")?;
        let style = cur.read_u8("font_style")?;
        let size = cur.read_u8("font_size")?;
        cur.skip(2, "font_pad")?;
        fonts.push(nwc_model::FontStyle {
            name,
            style: style_flags_to_string(style),
            size,
            typeface: 0,
        });
        let _ = i;
    }

    // --- staff prelude ------------------------------------------------------
    cur.skip(4, "staff_prelude_constant")?;
    let staff_count = cur.read_u16_le("staff_count")?;
    report.push(
        Severity::Info,
        cur.pos(),
        format!("staff_count = {staff_count}"),
    );

    // M1 strategy: extract only the first staff's name + group cstrs. For
    // remaining staves, scan the body for the inter-staff separator pattern
    // (`00 02 00 00 00 00 00 00 00` followed by the next staff's name cstr).
    // Subsequent staves whose names cannot be located are filled in as
    // placeholders so the resulting MusicXML still has the correct part count.
    let mut staves = Vec::with_capacity(staff_count as usize);
    if staff_count == 0 {
        return Ok(Score {
            info: header.info.clone(),
            fonts,
            staves,
            source_version: header.version,
        });
    }

    let first_name = cur.read_cstr_lossy("staff_1_name")?;
    let first_group = cur.read_cstr_lossy("staff_1_group")?;
    staves.push(make_empty_staff(0, &first_name, &first_group));
    report.push(Severity::Info, cur.pos(), format!("staff #0: {first_name:?}"));

    for s_idx in 1..staff_count {
        match scan_next_staff(body, cur.pos()) {
            Some((name_start, name, group, after_group)) => {
                let _ = name_start;
                staves.push(make_empty_staff(s_idx as usize, &name, &group));
                report.push(Severity::Info, after_group, format!("staff #{s_idx}: {name:?}"));
                cur.skip(after_group - cur.pos(), "skip to next staff")?;
            }
            None => {
                report.push(
                    Severity::Warn,
                    cur.pos(),
                    format!(
                        "could not locate staff #{s_idx}; emitting placeholder"
                    ),
                );
                let placeholder = format!("Staff {}", s_idx + 1);
                staves.push(make_empty_staff(s_idx as usize, &placeholder, "Standard"));
            }
        }
    }

    Ok(Score {
        info: header.info.clone(),
        fonts,
        staves,
        source_version: header.version,
    })
}

const STAFF_SEP: &[u8] = b"\x00\x02\x00\x00\x00\x00\x00\x00\x00";

/// Scan forward from `from` looking for the inter-staff separator. Returns
/// `(name_start_offset, staff_name, staff_group, offset_after_group_cstr)`.
fn scan_next_staff(
    body: &[u8],
    from: usize,
) -> Option<(usize, String, String, usize)> {
    let mut probe = from;
    while probe + STAFF_SEP.len() < body.len() {
        if let Some(rel) = find_subseq(&body[probe..], STAFF_SEP) {
            let name_start = probe + rel + STAFF_SEP.len();
            // Read name cstr at name_start.
            let nul = body[name_start..].iter().position(|&b| b == 0)?;
            // A non-printable name is almost certainly a false positive; advance.
            let name_bytes = &body[name_start..name_start + nul];
            if name_bytes.is_empty()
                || name_bytes
                    .iter()
                    .any(|&b| !(b == b' ' || (0x21..=0x7e).contains(&b) || b >= 0x80))
            {
                probe = name_start + 1;
                continue;
            }
            let group_start = name_start + nul + 1;
            let g_nul = body[group_start..].iter().position(|&b| b == 0)?;
            let group_bytes = &body[group_start..group_start + g_nul];
            // Validate: group must look like a real staff group label.
            let group_str = String::from_utf8_lossy(group_bytes).into_owned();
            if !is_known_staff_group(&group_str) {
                probe = name_start + 1;
                continue;
            }
            return Some((
                name_start,
                String::from_utf8_lossy(name_bytes).into_owned(),
                group_str,
                group_start + g_nul + 1,
            ));
        } else {
            return None;
        }
    }
    None
}

fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

fn is_known_staff_group(s: &str) -> bool {
    matches!(
        s,
        "Standard" | "Brace" | "Bracket" | "Orchestra" | "Choir" | "Section"
    )
}

fn make_empty_staff(idx: usize, name: &str, group: &str) -> Staff {
    let display = if name.is_empty() {
        format!("Staff {}", idx + 1)
    } else {
        name.to_string()
    };
    Staff {
        name: display.clone(),
        label: Some(display),
        label_abbr: None,
        group: if group.is_empty() {
            None
        } else {
            Some(group.to_string())
        },
        instrument: nwc_model::Instrument::default(),
        transposition: 0,
        lyrics: Vec::new(),
        objects: Vec::new(),
    }
}

fn style_flags_to_string(style: u8) -> String {
    let mut parts = Vec::new();
    if style & 0x01 != 0 {
        parts.push("italic");
    }
    if style & 0x02 != 0 {
        parts.push("bold");
    }
    if parts.is_empty() {
        "regular".into()
    } else {
        parts.join("+")
    }
}
