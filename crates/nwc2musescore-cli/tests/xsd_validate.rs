//! Validate emitter output against the MusicXML 4.0 XSD using `xmllint`.
//!
//! This test is skipped (with a printed reason) when either `xmllint` is
//! not on `PATH` or the schema files are not present at
//! `<repo>/schema/musicxml-4.0/musicxml.xsd`. To populate the schema, run:
//!
//! ```sh
//! ./scripts/fetch-schema.sh
//! ```

use std::path::PathBuf;
use std::process::Command;

use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR for nwc2musescore-cli is `<root>/crates/nwc2musescore-cli`.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates
    p.pop(); // root
    p
}

fn schema_path() -> PathBuf {
    workspace_root().join("schema/musicxml-4.0/musicxml.xsd")
}

fn xmllint_available() -> bool {
    Command::new("xmllint")
        .arg("--version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty())
        .unwrap_or(false)
}

fn build_synthetic_nwc201_with_one_staff() -> Vec<u8> {
    // Same shape as in m1_smoke.rs but with staff_count=1 so the emitter
    // produces a non-empty <part-list> + a <part>.
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(b"[NoteWorthy ArtWare]");
    body.extend_from_slice(&[0, 0, 0]);
    body.extend_from_slice(b"[NoteWorthy Composer]");
    body.push(0);
    body.push(0x4B);
    body.extend_from_slice(&[0x01, 0x02]);
    body.extend_from_slice(&[0, 0, 0]);
    body.push(0); // user (empty)
    body.push(0); // unknown (empty)
    body.extend_from_slice(&[0u8; 10]);
    body.extend_from_slice(b"Hello\0"); // title
    body.push(0); // author (empty)
    body.push(0); // copyright1
    body.push(0); // copyright2
    body.push(0); // comment
    body.extend_from_slice(b"NY_\0F2\0");
    body.extend_from_slice(&[0, 0, 0, 0]);
    body.extend_from_slice(b"0.5 0.5 0.5 0.5\0");
    body.extend_from_slice(&[0u8; 36]);
    body.extend_from_slice(&[0x10, 0x00]);
    for _ in 0..12 {
        body.extend_from_slice(b"Times New Roman\0");
        body.extend_from_slice(&[0, 8, 0, 0]);
    }
    body.extend_from_slice(&[0xff, 0x00, 0x00, 0x00]);
    body.extend_from_slice(&[0x01, 0x00]); // staff_count = 1
    body.extend_from_slice(b"Voice 1\0"); // staff name
    body.extend_from_slice(b"Standard\0"); // staff group

    let mut out = Vec::new();
    out.extend_from_slice(b"[NWZ]\0");
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&body).unwrap();
    out.extend_from_slice(&enc.finish().unwrap());
    out
}

#[test]
fn synthetic_output_passes_musicxml_4_0_xsd() {
    let schema = schema_path();
    if !schema.exists() {
        eprintln!(
            "skipping: schema not present at {}; run scripts/fetch-schema.sh",
            schema.display()
        );
        return;
    }
    if !xmllint_available() {
        eprintln!("skipping: xmllint not found on PATH");
        return;
    }

    let nwc = build_synthetic_nwc201_with_one_staff();
    let (score, _report) = nwc_parse::parse_bytes(&nwc).expect("parse");
    let opts = musicxml_write::WriteOptions::default();
    let xml = musicxml_write::write(&score, &opts).expect("write");

    let tmp = std::env::temp_dir().join("nwc2musescore_xsd_test.musicxml");
    std::fs::write(&tmp, &xml).unwrap();

    let output = Command::new("xmllint")
        .arg("--noout")
        .arg("--nonet")
        .arg("--schema")
        .arg(&schema)
        .arg(&tmp)
        .output()
        .expect("invoke xmllint");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "xmllint XSD validation failed:\n{stderr}\n--- generated XML ---\n{xml}",
        );
    }
}
