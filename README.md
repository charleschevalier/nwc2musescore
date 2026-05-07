# nwc2musescore

Convert NoteWorthy Composer (`.nwc` / `.nwz`) binary files to MusicXML, the
standard import format for MuseScore 4.x and most other notation editors.

## Status

Early development. See the implementation plan in
`docs/PLAN.md` (or the original at
`/home/charles/.claude/plans/i-want-to-convert-linked-blossom.md`).

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
