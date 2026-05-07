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

    // 12 fonts: cstr name + 4 trailing bytes (style, size, unused, charset).
    let font_count = 12usize;
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

    // Per-staff metadata for NWC 2.01. Music21 documents a 27-byte opaque
    // prefix before `lines u8`; empirically that offset is 18 in 2.01 files
    // (the `lines = 5` value lands there for a standard 5-line staff). The
    // remaining field positions then line up with music21.
    cur.skip(18, "staff_opaque_v201")?;
    let _lines = cur.read_u8("lines")?;
    let _layer_with_next = cur.read_u16_le("layerWithNextStaff")?;
    let transposition = cur.read_u16_le("transposition")? as i16;
    let _part_volume = cur.read_u16_le("partVolume")?;
    let _stereo_pan = cur.read_u16_le("stereoPan")?;
    let _color = cur.read_u8("color")?;
    let _align_syllable = cur.read_u16_le("alignSyllable")?;
    let number_of_lyrics = cur.read_u16_le("numberOfLyrics")?;
    if number_of_lyrics > 0 {
        let _lyric_alignment = cur.read_u16_le("lyricAlignment")?;
        let _staff_offset = cur.read_u16_le("staffOffset")?;
    }
    let lyrics: Vec<LyricLine> = if number_of_lyrics == 0 {
        Vec::new()
    } else {
        // Lyric block format still needs corpus validation; skip for now.
        report.push(
            Severity::Info,
            cur.pos(),
            format!(
                "staff #{s_idx}: {number_of_lyrics} lyric verses present but not yet decoded"
            ),
        );
        Vec::new()
    };
    if number_of_lyrics > 0 {
        // Without decoding lyric blocks, we cannot advance past them. Bail
        // and let the outer fallback scan recover.
        return Err(NwcError::Malformed {
            offset: cur.pos(),
            message: "lyric-bearing staff: lyric block decoding TODO".into(),
        });
    }
    let _ = cur.read_u16_le("staff junk")?;

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
    for i in 0..object_count {
        let obj = parse_object(cur, header, report).map_err(|e| {
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
            midi_program: None,
            midi_channel: None,
            name: if instrument_name.is_empty() {
                None
            } else {
                Some(instrument_name)
            },
        },
        transposition: transposition.clamp(i8::MIN as i16, i8::MAX as i16) as i8,
        lyrics,
        objects,
    })
}

#[allow(dead_code)]
fn parse_lyrics(cur: &mut Cursor<'_>, n_verses: u16) -> Result<Vec<LyricLine>, NwcError> {
    let mut out = Vec::with_capacity(n_verses as usize);
    for _ in 0..n_verses {
        let block_size = cur.read_u16_le("lyricBlockSize")? as usize;
        if block_size == 0 {
            out.push(LyricLine::default());
            continue;
        }
        let block_start = cur.pos();
        let _ = cur.read_u16_le("unused_lyricSize")?;
        let _ = cur.read_u16_le("junk")?;
        let mut text = String::new();
        let max_iter = 1024;
        for _ in 0..max_iter {
            if cur.pos() >= block_start + block_size {
                break;
            }
            let s = cur.read_cstr_lossy("lyric_syllable")?;
            if s.is_empty() {
                break;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&s);
        }
        // Realign cursor exactly to block_start + block_size.
        let target = block_start + block_size;
        if cur.pos() < target {
            cur.skip(target - cur.pos(), "lyric tail")?;
        }
        out.push(LyricLine { text });
    }
    Ok(out)
}

fn parse_object(
    cur: &mut Cursor<'_>,
    header: &Header,
    report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    let object_type = cur.read_u16_le("objectType")?;
    let _visible = if header.version.minor >= 70 || header.version.major >= 2 {
        cur.read_u8("visible")?
    } else {
        0
    };
    match object_type {
        0 => parse_clef(cur),
        1 => parse_keysig(cur),
        2 => parse_barline(cur),
        3 => parse_ending(cur),
        4 => parse_instrument(cur, report),
        5 => parse_timesig(cur),
        6 => parse_tempo(cur, header),
        7 => parse_dynamic(cur),
        8 => parse_note(cur, header),
        9 => parse_rest(cur, header),
        10 => parse_chord(cur, header, report),
        11 => parse_pedal(cur, report),
        12 => parse_flow(cur, header, report),
        13 => parse_mpc(cur, header, report),
        14 => parse_tempo_variation(cur, header, report),
        15 => parse_dynamic_variation(cur, header, report),
        16 => parse_performance(cur, header, report),
        17 => parse_text(cur, report),
        18 => parse_rest_chord(cur, header, report),
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
    let style = cur.read_u8("barline style")?;
    let _local_repeat_count = cur.read_u8("barline lrc")?;
    let bar_style = match style {
        1 => BarStyle::Final,
        2 => BarStyle::Double,
        3 => BarStyle::Heavy,
        _ => BarStyle::Single,
    };
    Ok(StaffObject::Bar(Bar { style: bar_style }))
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
    let base = cur.read_u8("tempo base")?;
    if header.version.major < 2 && header.version.minor < 70 {
        let _ = cur.read_u16_le("tempo legacy junk")?;
    }
    let text = cur.read_cstr_lossy("tempo text")?;
    let base = match base {
        0 => NoteValue::Whole,
        1 => NoteValue::Half,
        2 => NoteValue::Quarter,
        3 => NoteValue::Eighth,
        4 => NoteValue::Sixteenth,
        5 => NoteValue::ThirtySecond,
        6 => NoteValue::SixtyFourth,
        _ => NoteValue::Quarter,
    };
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

fn parse_note(cur: &mut Cursor<'_>, header: &Header) -> Result<StaffObject, NwcError> {
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
    if header.version.major >= 2 && (attribute2 & 0x40) != 0 {
        let _stem_length = cur.read_u8("note stemLength")?;
    }

    // Build pitch from staff position. NWC stores pitch as a staff-line
    // offset; we map it to (step, octave) under a treble-clef assumption
    // here. Real clef-aware mapping requires writer-side context (M3 work).
    let pitch = staff_pos_to_pitch(pos, attribute2);

    // Duration value table (music21):
    // 0=64th, 1=32nd, 2=16th, 3=Eighth, 4=Quarter, 5=Half, 6=Whole
    let base = match duration_byte {
        0 => NoteValue::SixtyFourth,
        1 => NoteValue::ThirtySecond,
        2 => NoteValue::Sixteenth,
        3 => NoteValue::Eighth,
        4 => NoteValue::Quarter,
        5 => NoteValue::Half,
        6 => NoteValue::Whole,
        _ => NoteValue::Quarter,
    };
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
    let base = match duration_byte {
        0 => NoteValue::SixtyFourth,
        1 => NoteValue::ThirtySecond,
        2 => NoteValue::Sixteenth,
        3 => NoteValue::Eighth,
        4 => NoteValue::Quarter,
        5 => NoteValue::Half,
        6 => NoteValue::Whole,
        _ => NoteValue::Quarter,
    };
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
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    // Skip the chord header bytes (8 for v>=200) plus an optional stem length.
    cur.skip(8, "chord data1")?;
    if header.version.major >= 2 {
        // music21 reads attribute byte 7, checks 0x40 to know if stemLength
        // follows — but we already skipped 8 bytes including that byte. To
        // remain compatible with that branch, we don't read more here. M3
        // can refine.
    }
    Ok(StaffObject::User(UserObject {
        type_byte: 10,
        tag: Some("NoteChordMember".into()),
        raw: Vec::new(),
    }))
}

fn parse_rest_chord(
    cur: &mut Cursor<'_>,
    _header: &Header,
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    cur.skip(10, "rest chord")?;
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
    header: &Header,
    _report: &mut ConversionReport,
) -> Result<StaffObject, NwcError> {
    let _pos = cur.read_u8("mpc pos")?;
    let _placement = cur.read_u8("mpc placement")?;
    let payload_len = if header.version.major >= 2 {
        31
    } else if header.version.minor >= 55 {
        31
    } else {
        32
    };
    cur.skip(payload_len, "mpc payload")?;
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
/// to a pitch under a treble-clef assumption. NWC's staff position 0 is the
/// middle line of a 5-line staff (B4 on treble clef).
fn staff_pos_to_pitch(pos: i16, attribute2: u8) -> Pitch {
    // Treble clef: middle line = B4 (step B, octave 4, diatonic index 6).
    // Step diatonic offset from C0:
    //   C0 -> 0, D0 -> 1, ... B0 -> 6, C1 -> 7, ...
    // B4's diatonic index is 4*7 + 6 = 34.
    let middle_diatonic_index: i32 = 34;
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
    let alter_index = attribute2 & 0x07;
    let (alter, displayed) = match alter_index {
        1 => (1, Some(nwc_model::Accidental::Sharp)),
        2 => (2, Some(nwc_model::Accidental::DoubleSharp)),
        3 => (-1, Some(nwc_model::Accidental::Flat)),
        4 => (-2, Some(nwc_model::Accidental::DoubleFlat)),
        5 => (0, Some(nwc_model::Accidental::Natural)),
        _ => (0, None),
    };
    Pitch {
        step,
        octave,
        alter,
        displayed_accidental: displayed,
    }
}
