//! NWC 2.x post-header body parser.
//!
//! The structure here is derived from the BSD-licensed music21 project's
//! `noteworthy/binaryTranslate.py`. Field naming and layout (especially the
//! per-staff metadata block, the lyric block, and the object-stream type
//! table) closely follow that reference; see the top-level NOTICE file for
//! attribution.
//!
//! Score-level body layout (NWC 2.x, abridged):
//!
//! ```text
//! after the score-info header (last cstr was `comment`):
//!   u8  extendLastSystem
//!   u8  increaseNoteSpacing
//!   5 bytes opaque
//!   u8  measureNumbers
//!   1 byte opaque
//!   u16 measureStart
//!   cstr margins
//!   1 byte opaque
//!   2 bytes opaque
//!   32 bytes groupVisibility
//!   u8 allowLayering
//!   cstr notationTypeface          (v>=200)
//!   u16 staffHeight
//!   advanceToNotNUL + skip 2 bytes  (the "10 00" tail)
//!   12 fonts: cstr name + u8 style + u8 size + u8 ? + u8 charset
//!   u8 titlePageInfo
//!   u8 staffLabels
//!   u16 pageNumberStart
//!   1 byte opaque (v>=200)
//!   u8 numberOfStaves
//!   1 byte opaque
//!
//! per staff:
//!   cstr name
//!   cstr label                      (v>=200)
//!   cstr instrumentName             (v>=200)
//!   cstr group
//!   27 bytes opaque
//!   u8  lines
//!   u16 layerWithNextStaff
//!   u16 transposition
//!   u16 partVolume
//!   u16 stereoPan
//!   u8  color
//!   u16 alignSyllable
//!   u16 numberOfLyrics
//!   if numberOfLyrics > 0:
//!     u16 lyricAlignment
//!     u16 staffOffset
//!   for verse in 0..numberOfLyrics:
//!     u16 lyricBlockSize
//!     if lyricBlockSize > 0:
//!       u16 unused_lyricSize
//!       u16 junk
//!       null-terminated syllables until empty cstr
//!       advance to (parsePositionStart + lyricBlockSize)
//!   if numberOfLyrics > 0: u16 junk
//!   u16 junk
//!   u16 numberOfObjects   (subtract 2 for v>150)
//!   for i in 0..numberOfObjects:
//!     u16 objectType
//!     u8 visible (v>=170)
//!     type-specific body
//! ```

use nwc_model::{
    Bar, BarStyle, Clef, ClefKind, Duration as NwcDuration, FontStyle as ModelFontStyle,
    Instrument, KeySignature, LyricLine, NoteValue, Pitch, Rest, Score, Staff, StaffObject,
    Step, TimeSigKind, TimeSignature, UserObject,
};

#[allow(dead_code)]
fn _step_keepalive(_: Step) {}

use crate::cursor::Cursor;
use crate::error::NwcError;
use crate::header::Header;
use crate::report::{ConversionReport, Severity};

pub fn parse_body(
    body: &[u8],
    header: &Header,
    report: &mut ConversionReport,
) -> Result<Score, NwcError> {
    let mut cur = Cursor::new(body);
    cur.skip(header.staves_offset, "header skip")?;

    // Score-level body layout for NWC 2.01 (verified against the 422-file
    // corpus). Music21's `parseHeader` treats this region as a sequence of
    // u8/u16 fields, but on real 2.01 files those positions actually hold
    // the cstrs `NY_\0F2\0` (page-template name) followed by the margins
    // cstr. Music21 was probably calibrated against an older minor and
    // those fields drifted.
    let _page_template = cur.read_cstr_lossy("page_template")?;
    let _page_setup_tag = cur.read_cstr_lossy("page_setup_tag")?;
    cur.skip(4, "page_setup_flags")?;
    let _margins = cur.read_cstr_lossy("page_margins")?;
    cur.skip(36, "page_setup_tail")?;
    let _font_slots = cur.read_u16_le("font_slots")?;

    // Font count: 12 for both product variants (0x46 and 0x4B). Music21's
    // table thinks 0x46 → v1.70 → 10 fonts, but byte-level inspection of
    // 0x46-product corpus files (e.g. Ska boys) shows 12 fonts followed
    // by the staff prelude, same as 0x4B files.
    let _ = header.product;
    let font_count: usize = 12;
    let mut fonts = Vec::with_capacity(font_count);
    for _ in 0..font_count {
        let name = cur.read_cstr_lossy("font_name")?;
        let style = cur.read_u8("font_style")?;
        let size = cur.read_u8("font_size")?;
        let _ = cur.read_u8("font_unused")?;
        let charset = cur.read_u8("font_charset")?;
        fonts.push(ModelFontStyle {
            name,
            style: style_flags_to_string(style),
            size,
            typeface: charset,
        });
    }

    // Staff prelude: 4 constant bytes (0xff 0x00 0x00 0x00) + u16 staff_count.
    cur.skip(4, "staff_prelude_constant")?;
    let staff_count = cur.read_u16_le("staff_count")? as usize;
    report.push(
        Severity::Info,
        cur.pos(),
        format!("staff_count = {staff_count}"),
    );

    let mut staves = Vec::with_capacity(staff_count);
    for s_idx in 0..staff_count {
        // Snapshot the cursor position so we can recover if mid-staff
        // decoding hits an unknown layout.
        let resume_pos = cur.pos();
        match parse_staff(&mut cur, header, s_idx, report) {
            Ok(staff) => staves.push(staff),
            Err(e) => {
                report.push(
                    Severity::Warn,
                    cur.pos(),
                    format!(
                        "staff #{s_idx}: full parse failed ({e}); falling back to scan"
                    ),
                );
                // Recover: scan for the next inter-staff separator and emit
                // an empty placeholder. This keeps the rest of the score
                // structurally intact.
                if let Some((name, group, after)) =
                    scan_to_next_staff(body, resume_pos)
                {
                    staves.push(make_empty_staff(s_idx, &name, &group));
                    if after > cur.pos() {
                        let _ = cur.skip(after - cur.pos(), "scan_to_next_staff");
                    }
                } else {
                    staves.push(make_empty_staff(s_idx, "", "Standard"));
                    break;
                }
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

const STAFF_GROUP_LABELS: &[&str] =
    &["Standard", "Brace", "Bracket", "Orchestra", "Choir", "Section"];

fn scan_to_next_staff(
    body: &[u8],
    from: usize,
) -> Option<(String, String, usize)> {
    let mut probe = from;
    while probe < body.len() {
        for &group in STAFF_GROUP_LABELS {
            let pat = format!("\0{group}\0");
            if let Some(rel) = find_subseq(&body[probe..], pat.as_bytes()) {
                let group_nul = probe + rel;
                let name_nul =
                    body[..group_nul].iter().rposition(|&b| b == 0)?;
                let name_bytes = &body[name_nul + 1..group_nul];
                if name_bytes.is_empty()
                    || name_bytes
                        .iter()
                        .any(|&b| !(b == b' ' || (0x21..=0x7e).contains(&b) || b >= 0x80))
                {
                    probe = group_nul + 1;
                    continue;
                }
                let name = String::from_utf8_lossy(name_bytes).into_owned();
                let group_str = group.to_string();
                let after = group_nul + 1 + group.len() + 1;
                return Some((name, group_str, after));
            }
        }
        probe += 1;
    }
    None
}

fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
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
        instrument: Instrument::default(),
        transposition: 0,
        lyrics: Vec::new(),
        objects: Vec::new(),
    }
}

fn parse_staff(
    cur: &mut Cursor<'_>,
    header: &Header,
    s_idx: usize,
    report: &mut ConversionReport,
) -> Result<Staff, NwcError> {
    let name = cur.read_cstr_lossy("staff_name")?;
    let group = cur.read_cstr_lossy("staff_group")?;
    let label = String::new();
    let instrument_name = String::new();

    // Per-staff metadata layout (v2.01 + product 0x4B == music21's "v175"):
    //   11 bytes opaque
    //   u8  midi_program (0..127)
    //   10 bytes opaque
    //   i8  transposition (signed semitones)
    //   6 bytes opaque
    //   u16 align_syllable
    //   u16 number_of_lyrics
    cur.skip(11, "staff opaque #1")?;
    let midi_program = cur.read_u8("midi_program")?;
    cur.skip(10, "staff opaque #2")?;
    let transposition = cur.read_i8("transposition")? as i16;
    cur.skip(6, "staff opaque #3")?;
    let _align_syllable = cur.read_u16_le("alignSyllable")?;
    let number_of_lyrics = cur.read_u16_le("numberOfLyrics")?;

    if number_of_lyrics > 0 {
        let _lyric_alignment = cur.read_u16_le("lyricAlignment")?;
        let _staff_offset = cur.read_u16_le("staffOffset")?;
    }

    let lyrics = parse_lyrics_block(cur, number_of_lyrics, report, s_idx)?;

    // After lyrics, music21 reads:
    //   if numberOfLyrics > 0: u16 junk
    //   u16 junk_2
    if number_of_lyrics > 0 {
        let _ = cur.read_u16_le("post-lyrics junk")?;
    }
    let _ = cur.read_u16_le("staff junk_2")?;

    let raw_object_count = cur.read_u16_le("numberOfObjects")?;
    // music21 says: subtract 2 for v>150. Treat the count as an i32 to avoid
    // underflow when a staff happens to declare 0 / 1 objects.
    let object_count: i32 = raw_object_count as i32 - 2;
    if object_count < 0 {
        report.push(
            Severity::Warn,
            cur.pos(),
            format!(
                "staff #{s_idx}: object count {raw_object_count} < 2; emitting empty stream"
            ),
        );
    }
    let object_count = object_count.max(0) as usize;

    let mut objects = Vec::with_capacity(object_count);
    let mut current_clef = ClefKind::Treble;
    for i in 0..object_count {
        let obj = parse_object(cur, header, report, &mut current_clef).map_err(|e| {
            NwcError::Malformed {
                offset: cur.pos(),
                message: format!(
                    "staff #{s_idx}: object {i}/{object_count}: {e}"
                ),
            }
        })?;
        objects.push(obj);
    }

    let display_name = if !label.is_empty() {
        label.clone()
    } else if !name.is_empty() {
        name.clone()
    } else {
        format!("Staff {}", s_idx + 1)
    };

    let instrument_name_resolved =
        if !instrument_name.is_empty() {
            Some(instrument_name)
        } else {
            midi_program_to_name(midi_program).map(|s| s.to_string())
        };

    Ok(Staff {
        name: display_name.clone(),
        label: Some(display_name),
        label_abbr: None,
        group: if group.is_empty() {
            None
        } else {
            Some(group)
        },
        instrument: Instrument {
            midi_program: Some(midi_program),
            midi_channel: None,
            name: instrument_name_resolved,
        },
        transposition: transposition.clamp(i8::MIN as i16, i8::MAX as i16) as i8,
        lyrics,
        objects,
    })
}

/// Parse `n_verses` lyric blocks following the music21 v1.75 flow:
/// each verse begins with a `lyricBlockSize` u16 and (if non-zero) a
/// `unused_lyricSize` u16 + `junk` u16 + one or more null-terminated
/// syllables. The cursor is realigned to `block_start + lyricBlockSize`
/// after each verse so we don't drift even if syllable parsing is wrong.
fn parse_lyrics_block(
    cur: &mut Cursor<'_>,
    n_verses: u16,
    report: &mut ConversionReport,
    s_idx: usize,
) -> Result<Vec<LyricLine>, NwcError> {
    let mut out = Vec::with_capacity(n_verses as usize);
    for _verse in 0..n_verses {
        let block_size = cur.read_u16_le("lyricBlockSize")? as usize;
        if block_size == 0 {
            out.push(LyricLine::default());
            continue;
        }
        let _unused_size = cur.read_u16_le("unused_lyricSize")?;
        let block_start = cur.pos();
        let target = block_start + block_size;
        if target > cur.remaining() + cur.pos() {
            report.push(
                Severity::Warn,
                cur.pos(),
                format!(
                    "staff #{s_idx}: lyric block size {block_size} exceeds remaining bytes; truncating"
                ),
            );
            return Err(NwcError::UnexpectedEof {
                offset: cur.pos(),
                context: "lyric block",
            });
        }
        let _ = cur.read_u16_le("lyric junk")?;
        let mut text = String::new();
        let mut syllables: Vec<String> = Vec::new();
        let mut iter = 0usize;
        while cur.pos() < target && iter < 1000 {
            iter += 1;
            let s = cur.read_cstr_lossy("lyric_syllable")?;
            if s.is_empty() {
                break;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&s);
            syllables.push(s);
        }
        if cur.pos() < target {
            cur.skip(target - cur.pos(), "lyric tail")?;
        }
        out.push(LyricLine { text, syllables });
    }
    Ok(out)
}

/// Standard MIDI Level 1 program names. NWC's per-staff `midi_program`
/// field is a 1-based MIDI program (1..128); music21 subtracts 1 to index
/// into a 0-based list. We do the same.
fn midi_program_to_name(p: u8) -> Option<&'static str> {
    if p == 0 {
        return None;
    }
    let idx = (p - 1) as usize;
    MIDI_INSTRUMENTS.get(idx).copied()
}

const MIDI_INSTRUMENTS: &[&str] = &[
    "Acoustic Grand Piano", "Bright Acoustic Piano", "Electric Grand Piano",
    "Honky-tonk Piano", "Electric Piano 1", "Electric Piano 2", "Harpsichord",
    "Clavinet", "Celesta", "Glockenspiel", "Music Box", "Vibraphone",
    "Marimba", "Xylophone", "Tubular Bells", "Dulcimer", "Drawbar Organ",
    "Percussive Organ", "Rock Organ", "Church Organ", "Reed Organ",
    "Accordion", "Harmonica", "Tango Accordion", "Acoustic Guitar (nylon)",
    "Acoustic Guitar (steel)", "Electric Guitar (jazz)", "Electric Guitar (clean)",
    "Electric Guitar (muted)", "Overdriven Guitar", "Distortion Guitar",
    "Guitar harmonics", "Acoustic Bass", "Electric Bass (finger)",
    "Electric Bass (pick)", "Fretless Bass", "Slap Bass 1", "Slap Bass 2",
    "Synth Bass 1", "Synth Bass 2", "Violin", "Viola", "Cello", "Contrabass",
    "Tremolo Strings", "Pizzicato Strings", "Orchestral Harp", "Timpani",
    "String Ensemble 1", "String Ensemble 2", "SynthStrings 1", "SynthStrings 2",
    "Choir Aahs", "Voice Oohs", "Synth Voice", "Orchestra Hit", "Trumpet",
    "Trombone", "Tuba", "Muted Trumpet", "French Horn", "Brass Section",
    "SynthBrass 1", "SynthBrass 2", "Soprano Sax", "Alto Sax", "Tenor Sax",
    "Baritone Sax", "Oboe", "English Horn", "Bassoon", "Clarinet", "Piccolo",
    "Flute", "Recorder", "Pan Flute", "Blown Bottle", "Shakuhachi", "Whistle",
    "Ocarina", "Lead 1 (square)", "Lead 2 (sawtooth)", "Lead 3 (calliope)",
    "Lead 4 (chiff)", "Lead 5 (charang)", "Lead 6 (voice)", "Lead 7 (fifths)",
    "Lead 8 (bass + lead)", "Pad 1 (new age)", "Pad 2 (warm)", "Pad 3 (polysynth)",
    "Pad 4 (choir)", "Pad 5 (bowed)", "Pad 6 (metallic)", "Pad 7 (halo)",
    "Pad 8 (sweep)", "FX 1 (rain)", "FX 2 (soundtrack)", "FX 3 (crystal)",
    "FX 4 (atmosphere)", "FX 5 (brightness)", "FX 6 (goblins)", "FX 7 (echoes)",
    "FX 8 (sci-fi)", "Sitar", "Banjo", "Shamisen", "Koto", "Kalimba",
    "Bag pipe", "Fiddle", "Shanai", "Tinkle Bell", "Agogo", "Steel Drums",
    "Woodblock", "Taiko Drum", "Melodic Tom", "Synth Drum", "Reverse Cymbal",
    "Guitar Fret Noise", "Breath Noise", "Seashore", "Bird Tweet",
    "Telephone Ring", "Helicopter", "Applause", "Gunshot",
];

fn parse_object(
    cur: &mut Cursor<'_>,
    header: &Header,
    report: &mut ConversionReport,
    current_clef: &mut ClefKind,
) -> Result<StaffObject, NwcError> {
    let object_type = cur.read_u16_le("objectType")?;
    let _visible = if header.version.minor >= 70 || header.version.major >= 2 {
        cur.read_u8("visible")?
    } else {
        0
    };
    match object_type {
        0 => {
            let obj = parse_clef(cur)?;
            if let StaffObject::Clef(c) = &obj {
                *current_clef = c.kind;
            }
            Ok(obj)
        }
        1 => parse_keysig(cur),
        2 => parse_barline(cur),
        3 => parse_ending(cur),
        4 => parse_instrument(cur, report),
        5 => parse_timesig(cur),
        6 => parse_tempo(cur, header),
        7 => parse_dynamic(cur),
        8 => parse_note(cur, header, *current_clef),
        9 => parse_rest(cur, header),
        10 => parse_chord(cur, header, report, current_clef),
        11 => parse_pedal(cur, report),
        12 => parse_flow(cur, header, report),
        13 => parse_mpc(cur, header, report),
        14 => parse_tempo_variation(cur, header, report),
        15 => parse_dynamic_variation(cur, header, report),
        16 => parse_performance(cur, header, report),
        17 => parse_text(cur, report),
        18 => parse_rest_chord(cur, header, report, current_clef),
        other => {
            // Unknown object type. We can't safely recover without knowing the
            // size, so bail.
            Err(NwcError::Malformed {
                offset: cur.pos(),
                message: format!("unknown object type {other}"),
            })
        }
    }
}

// =============================================================================
// Object decoders
// =============================================================================

fn parse_clef(cur: &mut Cursor<'_>) -> Result<StaffObject, NwcError> {
    let clef_type = cur.read_u16_le("clefType")?;
    let octave_shift = cur.read_u16_le("octaveShift")?;
    let kind = match clef_type {
        0 => ClefKind::Treble,
        1 => ClefKind::Bass,
        2 => ClefKind::Alto,
        3 => ClefKind::Tenor,
        4 => ClefKind::Percussion,
        _ => ClefKind::Treble,
    };
    let octave_shift = match octave_shift {
        1 => 1,
        2 => -1,
        _ => 0,
    };
    Ok(StaffObject::Clef(Clef { kind, octave_shift }))
}

fn parse_keysig(cur: &mut Cursor<'_>) -> Result<StaffObject, NwcError> {
    let flats_mask = cur.read_u8("keysig flats")?;
    let _ = cur.read_u8("keysig pad")?;
    let sharps_mask = cur.read_u8("keysig sharps")?;
    cur.skip(7, "keysig opaque")?;
    let fifths = if sharps_mask != 0 {
        sharps_mask.count_ones() as i8
    } else if flats_mask != 0 {
        -(flats_mask.count_ones() as i8)
    } else {
        0
    };
    Ok(StaffObject::KeySignature(KeySignature { fifths }))
}

fn parse_barline(cur: &mut Cursor<'_>) -> Result<StaffObject, NwcError> {
    let style_byte = cur.read_u8("barline style")?;
    let _local_repeat_count = cur.read_u8("barline lrc")?;
    // Music21 BarStyles index:
    //   0 Single, 1 Double, 2 SectionOpen, 3 SectionClose,
    //   4 LocalRepeatOpen, 5 LocalRepeatClose,
    //   6 MasterRepeatOpen, 7 MasterRepeatClose
    match style_byte {
        4 | 6 => Ok(StaffObject::RepeatOpen),
        5 | 7 => Ok(StaffObject::RepeatClose { count: None }),
        1 => Ok(StaffObject::Bar(Bar { style: BarStyle::Double })),
        2 => Ok(StaffObject::Bar(Bar { style: BarStyle::Heavy })),
        3 => Ok(StaffObject::Bar(Bar { style: BarStyle::Final })),
        _ => Ok(StaffObject::Bar(Bar { style: BarStyle::Single })),
    }
}

fn parse_ending(cur: &mut Cursor<'_>) -> Result<StaffObject, NwcError> {
    let style = cur.read_u8("ending style")?;
    let _ = cur.read_u8("ending pad")?;
    Ok(StaffObject::Ending(nwc_model::Ending {
        number: style.max(1),
        stop: false,
    }))
}

fn parse_instrument(
    cur: &mut Cursor<'_>,
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    cur.skip(8, "instrument")?;
    Ok(StaffObject::User(UserObject {
        type_byte: 4,
        tag: Some("Instrument".into()),
        raw: Vec::new(),
    }))
}

fn parse_timesig(cur: &mut Cursor<'_>) -> Result<StaffObject, NwcError> {
    let numerator = cur.read_u16_le("timesig numerator")?;
    let bits = cur.read_u16_le("timesig bits")?;
    let style = cur.read_u16_le("timesig style")?;
    let denominator: u16 = 1u16 << (bits.min(15));
    let kind = match style {
        1 => TimeSigKind::Common,
        2 => TimeSigKind::CutTime,
        _ => TimeSigKind::Standard,
    };
    Ok(StaffObject::TimeSignature(TimeSignature {
        beats: numerator.min(255) as u8,
        beat_type: denominator.min(255) as u8,
        kind,
    }))
}

fn parse_tempo(
    cur: &mut Cursor<'_>,
    header: &Header,
) -> Result<StaffObject, NwcError> {
    let _pos = cur.read_u8("tempo pos")?;
    let _placement = cur.read_u8("tempo placement")?;
    let value = cur.read_u16_le("tempo bpm")?;
    let base_byte = cur.read_u8("tempo base")?;
    if header.version.major < 2 && header.version.minor < 70 {
        let _ = cur.read_u16_le("tempo legacy junk")?;
    }
    let text = cur.read_cstr_lossy("tempo text")?;
    let base = duration_byte_to_value(base_byte);
    Ok(StaffObject::Tempo(nwc_model::Tempo {
        bpm: value,
        base,
        text: if text.is_empty() { None } else { Some(text) },
    }))
}

fn parse_dynamic(cur: &mut Cursor<'_>) -> Result<StaffObject, NwcError> {
    let _pos = cur.read_u8("dyn pos")?;
    let _placement = cur.read_u8("dyn placement")?;
    let style = cur.read_u8("dyn style")?;
    let _velocity = cur.read_u16_le("dyn velocity")?;
    let _volume = cur.read_u16_le("dyn volume")?;
    let dyn_kind = match style {
        0 => nwc_model::Dynamic::Pppp,
        1 => nwc_model::Dynamic::Ppp,
        2 => nwc_model::Dynamic::Pp,
        3 => nwc_model::Dynamic::P,
        4 => nwc_model::Dynamic::Mp,
        5 => nwc_model::Dynamic::Mf,
        6 => nwc_model::Dynamic::F,
        7 => nwc_model::Dynamic::Ff,
        8 => nwc_model::Dynamic::Fff,
        9 => nwc_model::Dynamic::Ffff,
        _ => nwc_model::Dynamic::Mf,
    };
    Ok(StaffObject::Dynamic(dyn_kind))
}

/// Construct a Note model from the 8-byte payload (without the
/// type-prefix u16 + visible u8). Used by the chord parser, which reads
/// the 10-byte chord header and treats bytes 0..8 as the top note's
/// payload.
fn note_from_payload(p: &[u8], current_clef: ClefKind) -> nwc_model::Note {
    let duration_byte = p[0];
    let _data2 = [p[1], p[2], p[3]];
    let attribute1 = [p[4], p[5]];
    let pos_signed = p[6] as i8 as i16;
    let pos = -pos_signed;
    let attribute2 = p[7];
    let pitch = staff_pos_to_pitch(pos, attribute2, current_clef);
    let base = duration_byte_to_value(duration_byte);
    let dot_attr = attribute1[0];
    let dots = if (dot_attr & 0x01) > 0 {
        2
    } else if (dot_attr & 0x04) > 0 {
        1
    } else {
        0
    };
    nwc_model::Note {
        pitch,
        duration: NwcDuration { base, dots, tuplet: None },
        stem: nwc_model::StemDir::Auto,
        beam: nwc_model::BeamState::None,
        tie: if (attribute1[0] & 0x10) != 0 {
            Some(nwc_model::TieDir::Start)
        } else {
            None
        },
        slur: None,
        articulations: nwc_model::Articulations::default(),
        ornaments: nwc_model::Ornaments::default(),
        grace: false,
        triplet: None,
        voice: 1,
        lyric_anchor: false,
        velocity: None,
        muted: false,
    }
}

/// NWC's note duration byte → NoteValue. Mirrors music21's
/// `constants.DurationValues` table: 0=Whole, 1=Half, 2=Quarter,
/// 3=Eighth, 4=16th, 5=32nd, 6=64th.
fn duration_byte_to_value(b: u8) -> NoteValue {
    match b {
        0 => NoteValue::Whole,
        1 => NoteValue::Half,
        2 => NoteValue::Quarter,
        3 => NoteValue::Eighth,
        4 => NoteValue::Sixteenth,
        5 => NoteValue::ThirtySecond,
        6 => NoteValue::SixtyFourth,
        _ => NoteValue::Quarter,
    }
}

fn parse_note(
    cur: &mut Cursor<'_>,
    header: &Header,
    current_clef: ClefKind,
) -> Result<StaffObject, NwcError> {
    let _ = header;
    let duration_byte = cur.read_u8("note duration")?;
    let data2 = [
        cur.read_u8("note data2[0]")?,
        cur.read_u8("note data2[1]")?,
        cur.read_u8("note data2[2]")?,
    ];
    let attribute1 = [
        cur.read_u8("note attr1[0]")?,
        cur.read_u8("note attr1[1]")?,
    ];
    let pos_signed = cur.read_i8("note pos")? as i16;
    let pos = -pos_signed; // music21 negates
    let attribute2 = cur.read_u8("note attr2")?;
    // NWC 2.01 files use the v1.75 layout (no stemLength byte even when
    // attribute2 & 0x40 is set; music21's "v>=200 might have stemLength"
    // branch only kicks in for later v2.x point releases).

    // Build pitch from staff position using the clef in scope.
    let pitch = staff_pos_to_pitch(pos, attribute2, current_clef);

    let base = duration_byte_to_value(duration_byte);
    let dot_attr = attribute1[0];
    let dots = if (dot_attr & 0x01) > 0 {
        2
    } else if (dot_attr & 0x04) > 0 {
        1
    } else {
        0
    };
    let _grace = (attribute1[1] & 0x20) != 0;
    let _ = data2;

    Ok(StaffObject::Note(nwc_model::Note {
        pitch,
        duration: NwcDuration {
            base,
            dots,
            tuplet: None,
        },
        stem: nwc_model::StemDir::Auto,
        beam: nwc_model::BeamState::None,
        tie: if (attribute1[0] & 0x10) != 0 {
            Some(nwc_model::TieDir::Start)
        } else {
            None
        },
        slur: None,
        articulations: nwc_model::Articulations::default(),
        ornaments: nwc_model::Ornaments::default(),
        grace: false,
        triplet: None,
        voice: 1,
        lyric_anchor: false,
        velocity: None,
        muted: false,
    }))
}

fn parse_rest(cur: &mut Cursor<'_>, header: &Header) -> Result<StaffObject, NwcError> {
    let _ = header;
    let duration_byte = cur.read_u8("rest duration")?;
    let data2 = [
        cur.read_u8("rest data2[0]")?,
        cur.read_u8("rest data2[1]")?,
        cur.read_u8("rest data2[2]")?,
        cur.read_u8("rest data2[3]")?,
        cur.read_u8("rest data2[4]")?,
    ];
    let _offset = cur.read_u16_le("rest offset")?;
    let base = duration_byte_to_value(duration_byte);
    let dots = if (data2[3] & 0x01) > 0 {
        2
    } else if (data2[3] & 0x04) > 0 {
        1
    } else {
        0
    };
    Ok(StaffObject::Rest(Rest {
        duration: NwcDuration {
            base,
            dots,
            tuplet: None,
        },
        voice: 1,
        triplet: None,
        fermata: false,
    }))
}

fn parse_chord(
    cur: &mut Cursor<'_>,
    header: &Header,
    report: &mut ConversionReport,
    current_clef: &mut ClefKind,
) -> Result<StaffObject, NwcError> {
    // music21's v=175 NoteChordMember layout:
    //   data1: 10 bytes
    //     [0..8] - same shape as a single Note's payload
    //     [8]    - numberOfNotes (additional chord members)
    //     [9]    - padding / unknown
    //   data2: N additional Note objects (parsed inline, type-prefixed)
    //
    // We synthesise a top note from data1 and append the additional pos
    // values from data2 to produce a Chord with N+1 voiced positions.
    let mut data1 = [0u8; 10];
    for slot in data1.iter_mut() {
        *slot = cur.read_u8("chord data1")?;
    }
    let number_of_notes = data1[8] as usize;

    let base_note = note_from_payload(&data1[..8], *current_clef);

    let mut chord_notes = Vec::with_capacity(number_of_notes + 1);
    chord_notes.push(base_note.clone());
    for _ in 0..number_of_notes {
        let obj = parse_object(cur, header, report, current_clef)?;
        if let StaffObject::Note(n) = obj {
            chord_notes.push(n);
        }
    }

    Ok(StaffObject::Chord(nwc_model::Chord {
        notes: chord_notes,
        duration: base_note.duration,
        stem: base_note.stem,
        beam: base_note.beam,
        voice: base_note.voice,
    }))
}

fn parse_rest_chord(
    cur: &mut Cursor<'_>,
    header: &Header,
    report: &mut ConversionReport,
    current_clef: &mut ClefKind,
) -> Result<StaffObject, NwcError> {
    // music21's restChordMember() calls noteChordMember(): same 10 bytes of
    // data1 plus N more chord-note objects parsed inline.
    let mut data1 = [0u8; 10];
    for slot in data1.iter_mut() {
        *slot = cur.read_u8("rest chord data1")?;
    }
    let number_of_notes = data1[8] as usize;
    for _ in 0..number_of_notes {
        let _ = parse_object(cur, header, report, current_clef)?;
    }
    Ok(StaffObject::User(UserObject {
        type_byte: 18,
        tag: Some("RestChordMember".into()),
        raw: Vec::new(),
    }))
}

fn parse_pedal(
    cur: &mut Cursor<'_>,
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    cur.skip(3, "pedal")?;
    Ok(StaffObject::User(UserObject {
        type_byte: 11,
        tag: Some("Pedal".into()),
        raw: Vec::new(),
    }))
}

fn parse_flow(
    cur: &mut Cursor<'_>,
    _header: &Header,
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    let _pos = cur.read_u8("flow pos")?;
    let _placement = cur.read_u8("flow placement")?;
    let style = cur.read_u16_le("flow style")?;
    let flow = match style {
        0 => nwc_model::Flow::Coda,
        1 => nwc_model::Flow::Segno,
        2 => nwc_model::Flow::Fine,
        3 => nwc_model::Flow::DaCapo,
        4 => nwc_model::Flow::DaCapoAlFine,
        5 => nwc_model::Flow::DalSegno,
        6 => nwc_model::Flow::DalSegnoAlFine,
        7 => nwc_model::Flow::DalSegnoAlCoda,
        _ => nwc_model::Flow::Segno,
    };
    Ok(StaffObject::Flow(flow))
}

fn parse_mpc(
    cur: &mut Cursor<'_>,
    _header: &Header,
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    let _pos = cur.read_u8("mpc pos")?;
    let _placement = cur.read_u8("mpc placement")?;
    // v=175 layout (matches our v2.01 files): 32-byte payload.
    cur.skip(32, "mpc payload")?;
    Ok(StaffObject::User(UserObject {
        type_byte: 13,
        tag: Some("MPC".into()),
        raw: Vec::new(),
    }))
}

fn parse_tempo_variation(
    cur: &mut Cursor<'_>,
    _header: &Header,
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    cur.skip(4, "tempoVariation")?;
    Ok(StaffObject::User(UserObject {
        type_byte: 14,
        tag: Some("TempoVariation".into()),
        raw: Vec::new(),
    }))
}

fn parse_dynamic_variation(
    cur: &mut Cursor<'_>,
    _header: &Header,
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    cur.skip(3, "dynVariation")?;
    Ok(StaffObject::User(UserObject {
        type_byte: 15,
        tag: Some("DynamicVariation".into()),
        raw: Vec::new(),
    }))
}

fn parse_performance(
    cur: &mut Cursor<'_>,
    _header: &Header,
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    cur.skip(3, "performance")?;
    Ok(StaffObject::User(UserObject {
        type_byte: 16,
        tag: Some("Performance".into()),
        raw: Vec::new(),
    }))
}

fn parse_text(
    cur: &mut Cursor<'_>,
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    let _pos = cur.read_i8("text pos")?;
    let _data = cur.read_u8("text data")?;
    let _font = cur.read_u8("text font")?;
    let text = cur.read_cstr_lossy("text body")?;
    Ok(StaffObject::Text(nwc_model::StaffText { text }))
}

// =============================================================================
// Helpers
// =============================================================================

#[allow(dead_code)]
fn advance_to_not_nul(cur: &mut Cursor<'_>) {
    while cur.peek_bytes(1).map(|b| b[0] == 0).unwrap_or(false) {
        if cur.skip(1, "advance_to_not_nul").is_err() {
            return;
        }
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

/// Map an NWC staff position (positive = above middle line, negative = below)
/// to a pitch using the clef in scope. NWC's pos=0 is the middle line of
/// a 5-line staff. Each clef has a different middle-line pitch:
///   Treble:    B4
///   Bass:      D3
///   Alto:      C4
///   Tenor:     A3
///   Percussion (1-line): treated as C4 placeholder.
fn staff_pos_to_pitch(pos: i16, attribute2: u8, clef: ClefKind) -> Pitch {
    let middle_diatonic_index: i32 = match clef {
        // C0=0, B0=6, C1=7, …; B4 = 4*7+6 = 34, D3 = 3*7+1 = 22,
        // C4 = 4*7+0 = 28, A3 = 3*7+5 = 26.
        ClefKind::Treble => 34,
        ClefKind::Bass => 22,
        ClefKind::Alto => 28,
        ClefKind::Tenor => 26,
        ClefKind::Percussion => 28,
    };
    let diatonic_index = middle_diatonic_index + pos as i32;
    let octave = ((diatonic_index).div_euclid(7)) as i8;
    let step_idx = (diatonic_index).rem_euclid(7) as u8;
    let step = match step_idx {
        0 => Step::C,
        1 => Step::D,
        2 => Step::E,
        3 => Step::F,
        4 => Step::G,
        5 => Step::A,
        _ => Step::B,
    };
    // Music21 AlterationTexts table (index = attribute2 & 0x07):
    //   0 Sharp '#', 1 Flat 'b', 2 Natural 'n',
    //   3 Double-sharp 'x', 4 Double-flat 'v', 5 none
    let alter_index = attribute2 & 0x07;
    let (alter, displayed) = match alter_index {
        0 => (1, Some(nwc_model::Accidental::Sharp)),
        1 => (-1, Some(nwc_model::Accidental::Flat)),
        2 => (0, Some(nwc_model::Accidental::Natural)),
        3 => (2, Some(nwc_model::Accidental::DoubleSharp)),
        4 => (-2, Some(nwc_model::Accidental::DoubleFlat)),
        _ => (0, None),
    };
    Pitch {
        step,
        octave,
        alter,
        displayed_accidental: displayed,
    }
}
