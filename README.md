# nwc2musescore

Convert NoteWorthy Composer (`.nwc` / `.nwz`) binary files to MusicXML, the
standard import format for MuseScore 4.x and most other notation editors.

## Status

Working end-to-end for NWC 2.01 files. Validated against a 422-file corpus:

| Check                                            | Result      |
| ------------------------------------------------ | ----------- |
| Files that convert without aborting              | 422 / 422   |
| MusicXML 4.0 XSD-valid output                    | 422 / 422   |
| Loaded successfully by Python's `music21`        | 422 / 422   |
| Full structured staff/object parse               | 408 / 422   |
| Note-by-note match against music21's `nwctxt`    | 403 / 422   |

The remaining 14 files fall through to a scanner-based fallback that emits
empty placeholder staves rather than incorrect notes — these are corner-case
files that even music21's own NWC parser fails on.

Currently emitted from the binary into MusicXML:
- Score-level metadata (title, composer, copyright)
- Per-staff name + group + MIDI program (e.g. "Trumpet")
- Clef, key signature, time signature, tempo
- Notes (pitch, duration, dots, accidentals) with clef-aware octave mapping
- Rests, chords (multi-pitch), repeats, voltas, double / final / heavy bars
- Lyrics with proper `<syllabic>` markers (begin / middle / end / single)

Not yet emitted (planned):
- Ties, slurs, articulations, dynamics
- Tempo as `<direction>` with `<sound tempo>`
- Page text / score credits beyond title

## Usage

```sh
cargo run -p nwc2musescore-cli -- input.nwc -o output.musicxml
```

## Crates

- `nwc-model` — format-version-agnostic domain types.
- `nwc-parse` — binary `.nwc` / `.nwz` decoder.
- `musicxml-write` — MusicXML 4.0 partwise emitter.
- `nwc2musescore-cli` — command-line driver.

## Validation

Generated MusicXML can be validated against the official MusicXML 4.0 XSD with
`xmllint`:

```sh
./scripts/fetch-schema.sh   # one-time, downloads to ./schema/ (gitignored)
xmllint --noout --nonet --schema schema/musicxml-4.0/musicxml.xsd output.musicxml
```

The `xsd_validate` integration test under `crates/nwc2musescore-cli/tests/`
runs this automatically when both `xmllint` and the schema files are
present (it prints "skipping" otherwise so CI without those still passes).

## License

Dual-licensed under MIT or Apache-2.0, at your option.

This project does not contain code derived from any GPL-licensed reference
implementation. Reverse-engineering of the NWC binary format draws on
publicly-discussed format facts and behavioral comparison against existing
open-source converters; no GPL source code has been read while writing the
parser.
