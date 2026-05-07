//! Round-trip integration test: convert one of the corpus files, then ask
//! Python's music21 library to load the resulting MusicXML and report
//! some basic structural facts. This test is skipped (with a printed
//! reason) when:
//!   * `python3` is not on `PATH`,
//!   * the music21 venv at `/tmp/m21venv` is not present, or
//!   * the test corpus directory `/home/charles/Documents/P2LR/pistons`
//!     is not available (i.e. on someone else's machine).
//!
//! The test does not assert specific note counts — it asserts only that
//! music21 *successfully parses* our output and finds at least one note,
//! which is the strongest "MuseScore-readable" signal we can get without
//! a working MuseScore CLI.

use std::path::Path;
use std::process::Command;

fn workspace_root() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates
    p.pop(); // root
    p
}

fn python3_path() -> Option<&'static str> {
    let candidates = ["/tmp/m21venv/bin/python", "/tmp/m21venv/bin/python3"];
    for c in candidates {
        if Path::new(c).exists() {
            return Some(c);
        }
    }
    None
}

#[test]
fn music21_can_load_our_musicxml() {
    let py = match python3_path() {
        Some(p) => p,
        None => {
            eprintln!("skipping: music21 venv not at /tmp/m21venv");
            return;
        }
    };
    let corpus_file = "/home/charles/Documents/P2LR/pistons/partoches NWC/Le Lapin.nwc";
    if !Path::new(corpus_file).exists() {
        eprintln!("skipping: corpus file not present at {corpus_file}");
        return;
    }

    let cli = workspace_root().join("target/release/nwc2musescore");
    if !cli.exists() {
        eprintln!(
            "skipping: release binary not built at {}; run `cargo build -p nwc2musescore-cli --release`",
            cli.display()
        );
        return;
    }

    let out = std::env::temp_dir().join("nwc2musescore_m21_rt.musicxml");
    let convert = Command::new(&cli)
        .arg(corpus_file)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("invoke nwc2musescore CLI");
    assert!(
        convert.status.success(),
        "CLI conversion failed: {}",
        String::from_utf8_lossy(&convert.stderr)
    );

    let py_script = format!(
        "from music21 import converter\n\
         s = converter.parse(r\"{}\")\n\
         total = sum(len(el.pitches) for p in s.parts for el in p.flatten().notes)\n\
         print(f'parts={{len(s.parts)}} pitches={{total}}')\n\
         assert total > 0, 'expected at least one note'\n",
        out.display()
    );
    let res = Command::new(py)
        .arg("-c")
        .arg(&py_script)
        .output()
        .expect("invoke python");
    if !res.status.success() {
        panic!(
            "music21 failed to load our MusicXML:\n--- stdout ---\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&res.stdout),
            String::from_utf8_lossy(&res.stderr),
        );
    }
    let stdout = String::from_utf8_lossy(&res.stdout);
    println!("music21 loaded our output: {}", stdout.trim());
}
