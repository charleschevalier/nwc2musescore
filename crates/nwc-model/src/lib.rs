//! Domain types for a NoteWorthy Composer score.
//!
//! These types are deliberately **format-version-agnostic**: nothing here
//! mentions NWC 1.x vs 2.x. The parser is responsible for normalising every
//! supported NWC format version into this shape, and the writer is allowed
//! to consume only this shape.

#![forbid(unsafe_code)]

pub mod duration;
pub mod pitch;

pub use duration::{Duration, NoteValue, Tuplet, TupletPos};
pub use pitch::{Accidental, Pitch, Step};

/// A complete score: metadata, fonts, and one or more staves.
#[derive(Debug, Clone, Default)]
pub struct Score {
    pub info: ScoreInfo,
    pub fonts: Vec<FontStyle>,
    pub staves: Vec<Staff>,
    /// Informational only. The writer must not branch on this.
    pub source_version: SourceVersion,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceVersion {
    /// Version major byte (e.g. `2` for NWC 2.x).
    pub major: u8,
    /// Version minor byte (e.g. `01` for NWC 2.01, `75` for NWC 2.75).
    pub minor: u8,
    /// Raw little-endian u16 (e.g. `0x0201`).
    pub raw: u16,
}

#[derive(Debug, Clone, Default)]
pub struct ScoreInfo {
    pub title: Option<String>,
    pub author: Option<String>,
    pub lyricist: Option<String>,
    pub copyright: Vec<String>,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FontStyle {
    pub name: String,
    pub style: String,
    pub size: u8,
    pub typeface: u8,
}

/// A single staff (NWC: "staff"; MusicXML: a `<part>`).
#[derive(Debug, Clone, Default)]
pub struct Staff {
    pub name: String,
    pub label: Option<String>,
    pub label_abbr: Option<String>,
    pub group: Option<String>,
    pub instrument: Instrument,
    /// Concert-pitch transposition in semitones.
    pub transposition: i8,
    /// Up to 8 lyric verses (NWC's hard cap).
    pub lyrics: Vec<LyricLine>,
    /// Ordered stream of musical events. Bars are explicit objects.
    pub objects: Vec<StaffObject>,
}

#[derive(Debug, Clone, Default)]
pub struct Instrument {
    pub midi_program: Option<u8>,
    pub midi_channel: Option<u8>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LyricLine {
    /// Raw lyric text, NWC-style separators preserved (`-` continued, `_` extender, ` ` single).
    pub text: String,
    /// Individual syllables extracted from the lyric block. Each syllable
    /// corresponds to one note-anchor in NWC's display. The writer attaches
    /// them to non-rest notes in order.
    pub syllables: Vec<String>,
}

/// One element on a staff. Objects are time-implicit; the writer derives
/// measure boundaries from explicit `Bar` markers.
#[derive(Debug, Clone)]
pub enum StaffObject {
    Clef(Clef),
    KeySignature(KeySignature),
    TimeSignature(TimeSignature),
    Tempo(Tempo),
    Dynamic(Dynamic),
    Note(Note),
    Chord(Chord),
    Rest(Rest),
    Bar(Bar),
    RepeatOpen,
    RepeatClose { count: Option<u8> },
    Ending(Ending),
    Flow(Flow),
    Text(StaffText),
    /// Object whose type byte was recognised but body could not be fully
    /// parsed, or an entirely unknown type. Preserved so the writer can
    /// emit a comment rather than silently drop it.
    User(UserObject),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClefKind {
    Treble,
    Bass,
    Alto,
    Tenor,
    Percussion,
}

#[derive(Debug, Clone, Copy)]
pub struct Clef {
    pub kind: ClefKind,
    /// Octave offset (e.g. -1 for "treble 8vb"). 0 in nearly all real files.
    pub octave_shift: i8,
}

#[derive(Debug, Clone, Default)]
pub struct KeySignature {
    /// Number of fifths: positive = sharps, negative = flats. -7..=7.
    pub fifths: i8,
}

#[derive(Debug, Clone, Copy)]
pub struct TimeSignature {
    pub beats: u8,
    pub beat_type: u8, // 1, 2, 4, 8, 16, …
    pub kind: TimeSigKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSigKind {
    Standard,
    Common,
    CutTime,
}

#[derive(Debug, Clone)]
pub struct Tempo {
    pub bpm: u16,
    pub base: NoteValue,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum Dynamic {
    Pppp, Ppp, Pp, P, Mp, Mf, F, Ff, Fff, Ffff,
    Sfz, Rfz, Fp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StemDir { Up, Down, Auto }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeamState { None, Begin, Continue, End, ForwardHook, BackwardHook }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TieDir { Start, Stop, Continue }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlurDir { Start, Stop, Continue }

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Articulations {
    pub staccato: bool,
    pub accent: bool,
    pub tenuto: bool,
    pub marcato: bool,
    pub fermata: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ornaments {
    pub trill: bool,
    pub mordent: bool,
    pub turn: bool,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub pitch: Pitch,
    pub duration: Duration,
    pub stem: StemDir,
    pub beam: BeamState,
    pub tie: Option<TieDir>,
    pub slur: Option<SlurDir>,
    pub articulations: Articulations,
    pub ornaments: Ornaments,
    pub grace: bool,
    pub triplet: Option<TupletPos>,
    pub voice: u8,
    pub lyric_anchor: bool,
    pub velocity: Option<u8>,
    pub muted: bool,
}

#[derive(Debug, Clone)]
pub struct Chord {
    pub notes: Vec<Note>,
    pub duration: Duration,
    pub stem: StemDir,
    pub beam: BeamState,
    pub voice: u8,
}

#[derive(Debug, Clone)]
pub struct Rest {
    pub duration: Duration,
    pub voice: u8,
    pub triplet: Option<TupletPos>,
    pub fermata: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarStyle {
    Single,
    Double,
    Final,
    Heavy,
}

#[derive(Debug, Clone, Copy)]
pub struct Bar {
    pub style: BarStyle,
}

#[derive(Debug, Clone)]
pub struct Ending {
    pub number: u8,
    pub stop: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Segno,
    Coda,
    Fine,
    DaCapo,
    DalSegno,
    DaCapoAlFine,
    DalSegnoAlFine,
    DalSegnoAlCoda,
}

#[derive(Debug, Clone)]
pub struct StaffText {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct UserObject {
    pub type_byte: u8,
    pub tag: Option<String>,
    pub raw: Vec<u8>,
}
