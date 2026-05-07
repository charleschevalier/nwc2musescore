//! End-to-end smoke test: parse a tiny synthetic NWC 2.01 file (header-only,
//! zero staves) and confirm the writer emits well-formed MusicXML.
//!
//! Per-staff object decoding is M2; this test only validates the M1 pipeline.

use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

fn build_synthetic_nwc201(title: &str, author: &str) -> Vec<u8> {
    // Construct an inflated body with valid score-level metadata, a default
    // page-setup block, an all-zero font table, and zero staves.
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(b"[NoteWorthy ArtWare]");
    body.extend_from_slice(&[0, 0, 0]);
    body.extend_from_slice(b"[NoteWorthy Composer]");
    body.push(0); // null after marker
    body.push(0x4B); // product
    body.extend_from_slice(&[0x01, 0x02]); // version 0x0201
    body.extend_from_slice(&[0, 0, 0]); // padding

    // score-info cstrs
    body.extend_from_slice(author.as_bytes());
    body.push(0);
    body.push(0); // licence_tag (empty)
    body.extend_from_slice(&[0u8; 10]); // 10 reserved bytes
    body.extend_from_slice(title.as_bytes());
    body.push(0);
    body.push(0); // subtitle
    body.push(0); // copyright1
    body.push(0); // copyright2
    body.push(0); // comments

    // page setup
    body.extend_from_slice(b"NY_\0");
    body.extend_from_slice(b"F2\0");
    body.extend_from_slice(&[0, 0, 0, 0]); // 4-byte flags
    body.extend_from_slice(b"0.5 0.5 0.5 0.5\0");
    body.extend_from_slice(&[0u8; 36]); // 36-byte tail
    body.extend_from_slice(&[0x10, 0x00]); // font_slots = 16

    // 12 fonts: name="Times New Roman" (variant index for uniqueness),
    // style=0, size=8, two zero pad bytes.
    for _ in 0..12 {
        body.extend_from_slice(b"Times New Roman\0");
        body.extend_from_slice(&[0, 8, 0, 0]);
    }

    // staff prelude with staff_count=0
    body.extend_from_slice(&[0xff, 0x00, 0x00, 0x00]);
    body.extend_from_slice(&[0x00, 0x00]);

    // Wrap with [NWZ]\0 header + zlib stream.
    let mut out = Vec::new();
    out.extend_from_slice(b"[NWZ]\0");
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&body).unwrap();
    let compressed = enc.finish().unwrap();
    out.extend_from_slice(&compressed);
    out
}

#[test]
fn synthetic_nwc201_round_trips_to_musicxml() {
    let bytes = build_synthetic_nwc201("Hello World", "Test Author");
    let (score, _report) = nwc_parse::parse_bytes(&bytes).expect("parse");
    assert_eq!(score.info.title.as_deref(), Some("Hello World"));
    assert_eq!(score.info.author.as_deref(), Some("Test Author"));
    assert_eq!(score.source_version.major, 2);
    assert_eq!(score.source_version.minor, 1);
    assert!(score.staves.is_empty());

    let opts = musicxml_write::WriteOptions::default();
    let xml = musicxml_write::write(&score, &opts).expect("write");
    assert!(xml.contains("<work-title>Hello World</work-title>"));
    assert!(xml.contains("<creator type=\"composer\">Test Author</creator>"));
    assert!(xml.contains("<part-list>"));
    // 0 staves -> empty part-list -> no part elements.
    assert!(!xml.contains("<part id="));
}
