//! MusicXML 4.0 partwise emission.
//!
//! M1 supports: score-partwise / part-list / per-staff measures with
//! `<attributes>` (divisions, key, time, clef) and `<note>` (pitch, duration,
//! voice, type). No directions, no notations.

use std::io::{Cursor, Write};

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;

use nwc_model::{
    Accidental, BarStyle, Clef, ClefKind, KeySignature, Note, Pitch, Rest, Score, Staff,
    StaffObject, Step, TimeSigKind, TimeSignature,
};

use crate::context::WriterCtx;
use crate::measures;
use crate::WriteError;
use crate::WriteOptions;

/// Default divisions-per-quarter. 480 is divisible by everything we need
/// for whole/half/quarter/eighth/16th/32nd plus triplets.
const DEFAULT_DIVISIONS: u32 = 480;

pub(crate) fn write_bytes(score: &Score, opts: &WriteOptions) -> Result<Vec<u8>, WriteError> {
    let mut buf = Cursor::new(Vec::with_capacity(8 * 1024));
    let mut w = if let Some(indent) = opts.indent {
        Writer::new_with_indent(&mut buf, b' ', indent as usize)
    } else {
        Writer::new(&mut buf)
    };

    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(xml_err)?;

    // DOCTYPE for MusicXML — quick-xml wraps the payload with <!DOCTYPE …>.
    let doctype = match opts.musicxml_version.as_str() {
        "3.1" | "3" => {
            r#" score-partwise PUBLIC "-//Recordare//DTD MusicXML 3.1 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd""#
        }
        _ => {
            r#" score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd""#
        }
    };
    w.write_event(Event::DocType(BytesText::from_escaped(doctype)))
        .map_err(xml_err)?;

    let mut root = BytesStart::new("score-partwise");
    root.push_attribute(("version", opts.musicxml_version.as_str()));
    w.write_event(Event::Start(root)).map_err(xml_err)?;

    write_work(&mut w, score)?;
    write_identification(&mut w, score)?;
    write_part_list(&mut w, score)?;

    for (idx, staff) in score.staves.iter().enumerate() {
        write_part(&mut w, staff, idx)?;
    }

    w.write_event(Event::End(BytesEnd::new("score-partwise")))
        .map_err(xml_err)?;

    Ok(buf.into_inner())
}

fn write_work<W: Write>(w: &mut Writer<W>, score: &Score) -> Result<(), WriteError> {
    if let Some(title) = &score.info.title {
        w.write_event(Event::Start(BytesStart::new("work"))).map_err(xml_err)?;
        write_text_element(w, "work-title", title)?;
        w.write_event(Event::End(BytesEnd::new("work"))).map_err(xml_err)?;
    }
    Ok(())
}

fn write_identification<W: Write>(w: &mut Writer<W>, score: &Score) -> Result<(), WriteError> {
    let has_creator = score.info.author.is_some() || score.info.lyricist.is_some();
    let has_rights = !score.info.copyright.is_empty();
    if !has_creator && !has_rights {
        return Ok(());
    }
    w.write_event(Event::Start(BytesStart::new("identification")))
        .map_err(xml_err)?;
    if let Some(author) = &score.info.author {
        let mut e = BytesStart::new("creator");
        e.push_attribute(("type", "composer"));
        w.write_event(Event::Start(e)).map_err(xml_err)?;
        w.write_event(Event::Text(BytesText::new(author))).map_err(xml_err)?;
        w.write_event(Event::End(BytesEnd::new("creator"))).map_err(xml_err)?;
    }
    if let Some(lyr) = &score.info.lyricist {
        let mut e = BytesStart::new("creator");
        e.push_attribute(("type", "lyricist"));
        w.write_event(Event::Start(e)).map_err(xml_err)?;
        w.write_event(Event::Text(BytesText::new(lyr))).map_err(xml_err)?;
        w.write_event(Event::End(BytesEnd::new("creator"))).map_err(xml_err)?;
    }
    for line in &score.info.copyright {
        write_text_element(w, "rights", line)?;
    }
    w.write_event(Event::Start(BytesStart::new("encoding"))).map_err(xml_err)?;
    write_text_element(w, "software", "nwc2musescore")?;
    w.write_event(Event::End(BytesEnd::new("encoding"))).map_err(xml_err)?;

    w.write_event(Event::End(BytesEnd::new("identification")))
        .map_err(xml_err)?;
    Ok(())
}

fn write_part_list<W: Write>(w: &mut Writer<W>, score: &Score) -> Result<(), WriteError> {
    w.write_event(Event::Start(BytesStart::new("part-list")))
        .map_err(xml_err)?;
    for (idx, staff) in score.staves.iter().enumerate() {
        let id = part_id(idx);
        let mut e = BytesStart::new("score-part");
        e.push_attribute(("id", id.as_str()));
        w.write_event(Event::Start(e)).map_err(xml_err)?;

        let display_name = staff
            .label
            .clone()
            .or_else(|| (!staff.name.is_empty()).then(|| staff.name.clone()))
            .unwrap_or_else(|| format!("Staff {}", idx + 1));
        write_text_element(w, "part-name", &display_name)?;
        if let Some(abbr) = &staff.label_abbr {
            write_text_element(w, "part-abbreviation", abbr)?;
        }
        w.write_event(Event::End(BytesEnd::new("score-part"))).map_err(xml_err)?;
    }
    w.write_event(Event::End(BytesEnd::new("part-list"))).map_err(xml_err)?;
    Ok(())
}

fn write_part<W: Write>(w: &mut Writer<W>, staff: &Staff, idx: usize) -> Result<(), WriteError> {
    let id = part_id(idx);
    let mut e = BytesStart::new("part");
    e.push_attribute(("id", id.as_str()));
    w.write_event(Event::Start(e)).map_err(xml_err)?;

    let mut ctx = WriterCtx::new(DEFAULT_DIVISIONS);
    let measures_v = measures::group(staff);

    // Lyric anchoring index per verse: each non-rest, non-grace note that
    // appears in the part consumes one syllable from each verse in order.
    let mut lyric_cursors: Vec<usize> = vec![0; staff.lyrics.len()];

    // Track per-part state so we can emit <attributes> changes only when
    // they actually change between measures.
    let mut emitted_initial_attributes = false;
    let mut current_clef: Option<Clef> = None;
    let mut current_key: Option<KeySignature> = None;
    let mut current_time: Option<TimeSignature> = None;

    for (m_idx, measure) in measures_v.iter().enumerate() {
        ctx.measure_number = (m_idx + 1) as u32;
        let mut start = BytesStart::new("measure");
        let num = ctx.measure_number.to_string();
        start.push_attribute(("number", num.as_str()));
        w.write_event(Event::Start(start)).map_err(xml_err)?;

        // Collect attribute updates from leading objects of this measure.
        let mut new_clef: Option<Clef> = None;
        let mut new_key: Option<KeySignature> = None;
        let mut new_time: Option<TimeSignature> = None;
        let mut musical_objects: Vec<&StaffObject> = Vec::new();
        for obj in &measure.objects {
            match obj {
                StaffObject::Clef(c) => new_clef = Some(*c),
                StaffObject::KeySignature(k) => new_key = Some(k.clone()),
                StaffObject::TimeSignature(t) => new_time = Some(*t),
                _ => musical_objects.push(*obj),
            }
        }

        let needs_attributes = !emitted_initial_attributes
            || new_clef.is_some()
            || new_key.is_some()
            || new_time.is_some();
        if needs_attributes {
            let clef = new_clef.or(current_clef).unwrap_or(Clef {
                kind: ClefKind::Treble,
                octave_shift: 0,
            });
            let key = new_key.clone().or(current_key.clone()).unwrap_or_default();
            let time = new_time.or(current_time);
            write_attributes(
                w,
                ctx.divisions,
                !emitted_initial_attributes || new_clef.is_some(),
                !emitted_initial_attributes || new_key.is_some(),
                !emitted_initial_attributes || new_time.is_some(),
                &clef,
                &key,
                time.as_ref(),
            )?;
            current_clef = Some(clef);
            current_key = Some(key);
            if let Some(t) = time {
                current_time = Some(t);
            }
            emitted_initial_attributes = true;
        }

        if measure.opens_repeat {
            write_left_repeat(w)?;
        }

        for obj in musical_objects {
            match obj {
                StaffObject::Note(n) => {
                    let lyrics = pop_syllables(&staff.lyrics, &mut lyric_cursors);
                    write_note(w, n, ctx.divisions, false, &lyrics)?;
                }
                StaffObject::Rest(r) => write_rest(w, r, ctx.divisions)?,
                StaffObject::Chord(c) => {
                    let lyrics = pop_syllables(&staff.lyrics, &mut lyric_cursors);
                    write_chord(w, c, ctx.divisions, &lyrics)?;
                }
                _ => {
                    // Other event kinds are dropped for now (Tempo,
                    // Dynamic, Text, Flow, Ending, ...). M3 work.
                }
            }
        }

        if let Some(bar) = measure.closing_bar {
            write_closing_bar(w, bar)?;
        }

        w.write_event(Event::End(BytesEnd::new("measure"))).map_err(xml_err)?;
    }

    // If the staff has zero objects produce a single empty measure so the
    // resulting MusicXML is still valid.
    if measures_v.is_empty() {
        let mut start = BytesStart::new("measure");
        start.push_attribute(("number", "1"));
        w.write_event(Event::Start(start)).map_err(xml_err)?;
        write_attributes(
            w,
            ctx.divisions,
            true, true, true,
            &Clef { kind: ClefKind::Treble, octave_shift: 0 },
            &KeySignature::default(),
            None,
        )?;
        w.write_event(Event::End(BytesEnd::new("measure"))).map_err(xml_err)?;
    }

    w.write_event(Event::End(BytesEnd::new("part"))).map_err(xml_err)?;
    Ok(())
}

fn write_attributes<W: Write>(
    w: &mut Writer<W>,
    divisions: u32,
    write_clef: bool,
    write_key: bool,
    write_time: bool,
    clef: &Clef,
    key: &KeySignature,
    time: Option<&TimeSignature>,
) -> Result<(), WriteError> {
    w.write_event(Event::Start(BytesStart::new("attributes")))
        .map_err(xml_err)?;
    write_text_element(w, "divisions", &divisions.to_string())?;
    if write_key {
        w.write_event(Event::Start(BytesStart::new("key"))).map_err(xml_err)?;
        write_text_element(w, "fifths", &key.fifths.to_string())?;
        w.write_event(Event::End(BytesEnd::new("key"))).map_err(xml_err)?;
    }
    if write_time {
        if let Some(t) = time {
            let mut e = BytesStart::new("time");
            match t.kind {
                TimeSigKind::Common => e.push_attribute(("symbol", "common")),
                TimeSigKind::CutTime => e.push_attribute(("symbol", "cut")),
                TimeSigKind::Standard => {}
            }
            w.write_event(Event::Start(e)).map_err(xml_err)?;
            write_text_element(w, "beats", &t.beats.to_string())?;
            write_text_element(w, "beat-type", &t.beat_type.to_string())?;
            w.write_event(Event::End(BytesEnd::new("time"))).map_err(xml_err)?;
        } else {
            // Default 4/4 to keep MuseScore happy on first measure.
            w.write_event(Event::Start(BytesStart::new("time"))).map_err(xml_err)?;
            write_text_element(w, "beats", "4")?;
            write_text_element(w, "beat-type", "4")?;
            w.write_event(Event::End(BytesEnd::new("time"))).map_err(xml_err)?;
        }
    }
    if write_clef {
        w.write_event(Event::Start(BytesStart::new("clef"))).map_err(xml_err)?;
        let (sign, line) = match clef.kind {
            ClefKind::Treble => ("G", "2"),
            ClefKind::Bass => ("F", "4"),
            ClefKind::Alto => ("C", "3"),
            ClefKind::Tenor => ("C", "4"),
            ClefKind::Percussion => ("percussion", "2"),
        };
        write_text_element(w, "sign", sign)?;
        write_text_element(w, "line", line)?;
        if clef.octave_shift != 0 {
            write_text_element(w, "clef-octave-change", &clef.octave_shift.to_string())?;
        }
        w.write_event(Event::End(BytesEnd::new("clef"))).map_err(xml_err)?;
    }
    w.write_event(Event::End(BytesEnd::new("attributes"))).map_err(xml_err)?;
    Ok(())
}

fn write_note<W: Write>(
    w: &mut Writer<W>,
    n: &Note,
    divisions: u32,
    is_chord_member: bool,
    lyrics: &[(u32, String, bool)],
) -> Result<(), WriteError> {
    w.write_event(Event::Start(BytesStart::new("note"))).map_err(xml_err)?;
    if is_chord_member {
        w.write_event(Event::Empty(BytesStart::new("chord"))).map_err(xml_err)?;
    }
    write_pitch(w, &n.pitch)?;
    write_text_element(w, "duration", &n.duration.in_divisions(divisions).to_string())?;
    let voice = n.voice.max(1);
    write_text_element(w, "voice", &voice.to_string())?;
    write_text_element(w, "type", n.duration.base.musicxml_name())?;
    for _ in 0..n.duration.dots {
        w.write_event(Event::Empty(BytesStart::new("dot"))).map_err(xml_err)?;
    }
    if let Some(acc) = n.pitch.displayed_accidental {
        write_text_element(w, "accidental", accidental_name(acc))?;
    }
    if !is_chord_member {
        for (verse, syllable, next_continues) in lyrics {
            write_lyric(w, *verse, syllable, *next_continues)?;
        }
    }
    w.write_event(Event::End(BytesEnd::new("note"))).map_err(xml_err)?;
    Ok(())
}

fn write_lyric<W: Write>(
    w: &mut Writer<W>,
    verse: u32,
    syllable: &str,
    next_continues: bool,
) -> Result<(), WriteError> {
    let mut e = BytesStart::new("lyric");
    let n = verse.to_string();
    e.push_attribute(("number", n.as_str()));
    w.write_event(Event::Start(e)).map_err(xml_err)?;
    // NWC encodes syllable boundaries via prefixes / suffixes on the
    // syllable text itself:
    //   leading '-'      → continuation of the previous syllable's word
    //   leading ' '      → start of a new word
    //   trailing '_'     → extender (held over)
    //   trailing '-' alone is rare; seen mid-word too
    // The syllabic dimension is computed as a function of "this syllable
    // starts a new word" and "the next syllable continues this word".
    let starts_continuation = syllable.starts_with('-');
    let extender = syllable.ends_with('_');
    let mut core = syllable.to_string();
    if let Some(stripped) = core.strip_prefix('-') {
        core = stripped.to_string();
    }
    if let Some(stripped) = core.strip_prefix(' ') {
        core = stripped.to_string();
    }
    if let Some(stripped) = core.strip_prefix('\r') {
        core = stripped.to_string();
    }
    if let Some(stripped) = core.strip_suffix('_') {
        core = stripped.to_string();
    }
    let syllabic = match (starts_continuation, next_continues) {
        (false, false) => "single",
        (false, true) => "begin",
        (true, true) => "middle",
        (true, false) => "end",
    };
    write_text_element(w, "syllabic", syllabic)?;
    write_text_element(w, "text", &core)?;
    if extender {
        w.write_event(Event::Empty(BytesStart::new("extend"))).map_err(xml_err)?;
    }
    w.write_event(Event::End(BytesEnd::new("lyric"))).map_err(xml_err)?;
    Ok(())
}

/// Pop one syllable per verse, returning `(verse_number, syllable, next_continues)` triples.
/// `next_continues` is true if the *next* syllable in the same verse begins
/// with `-` (i.e., this one is mid-word).
fn pop_syllables(
    lyrics: &[nwc_model::LyricLine],
    cursors: &mut [usize],
) -> Vec<(u32, String, bool)> {
    let mut out = Vec::new();
    for (verse_idx, line) in lyrics.iter().enumerate() {
        if cursors[verse_idx] < line.syllables.len() {
            let s = line.syllables[cursors[verse_idx]].clone();
            let next_continues = line
                .syllables
                .get(cursors[verse_idx] + 1)
                .map(|n| n.starts_with('-'))
                .unwrap_or(false);
            cursors[verse_idx] += 1;
            if !s.trim().is_empty() {
                out.push(((verse_idx + 1) as u32, s, next_continues));
            }
        }
    }
    out
}

fn write_chord<W: Write>(
    w: &mut Writer<W>,
    c: &nwc_model::Chord,
    divisions: u32,
    lyrics: &[(u32, String, bool)],
) -> Result<(), WriteError> {
    for (i, n) in c.notes.iter().enumerate() {
        let l: &[(u32, String, bool)] = if i == 0 { lyrics } else { &[] };
        write_note(w, n, divisions, i > 0, l)?;
    }
    Ok(())
}

fn write_left_repeat<W: Write>(w: &mut Writer<W>) -> Result<(), WriteError> {
    let mut bl = BytesStart::new("barline");
    bl.push_attribute(("location", "left"));
    w.write_event(Event::Start(bl)).map_err(xml_err)?;
    write_text_element(w, "bar-style", "heavy-light")?;
    let mut rep = BytesStart::new("repeat");
    rep.push_attribute(("direction", "forward"));
    w.write_event(Event::Empty(rep)).map_err(xml_err)?;
    w.write_event(Event::End(BytesEnd::new("barline"))).map_err(xml_err)?;
    Ok(())
}

fn write_closing_bar<W: Write>(
    w: &mut Writer<W>,
    bar: &StaffObject,
) -> Result<(), WriteError> {
    let (bar_style, repeat) = match bar {
        StaffObject::Bar(b) => (
            match b.style {
                BarStyle::Single => None,
                BarStyle::Double => Some("light-light"),
                BarStyle::Final => Some("light-heavy"),
                BarStyle::Heavy => Some("heavy"),
            },
            None,
        ),
        StaffObject::RepeatClose { .. } => (Some("light-heavy"), Some("backward")),
        StaffObject::RepeatOpen => (Some("heavy-light"), Some("forward")),
        _ => return Ok(()),
    };
    if bar_style.is_none() && repeat.is_none() {
        return Ok(());
    }
    let mut bl = BytesStart::new("barline");
    bl.push_attribute(("location", "right"));
    w.write_event(Event::Start(bl)).map_err(xml_err)?;
    if let Some(s) = bar_style {
        write_text_element(w, "bar-style", s)?;
    }
    if let Some(dir) = repeat {
        let mut rep = BytesStart::new("repeat");
        rep.push_attribute(("direction", dir));
        w.write_event(Event::Empty(rep)).map_err(xml_err)?;
    }
    w.write_event(Event::End(BytesEnd::new("barline"))).map_err(xml_err)?;
    Ok(())
}

fn write_rest<W: Write>(w: &mut Writer<W>, r: &Rest, divisions: u32) -> Result<(), WriteError> {
    w.write_event(Event::Start(BytesStart::new("note"))).map_err(xml_err)?;
    w.write_event(Event::Empty(BytesStart::new("rest"))).map_err(xml_err)?;
    write_text_element(w, "duration", &r.duration.in_divisions(divisions).to_string())?;
    let voice = r.voice.max(1);
    write_text_element(w, "voice", &voice.to_string())?;
    write_text_element(w, "type", r.duration.base.musicxml_name())?;
    for _ in 0..r.duration.dots {
        w.write_event(Event::Empty(BytesStart::new("dot"))).map_err(xml_err)?;
    }
    w.write_event(Event::End(BytesEnd::new("note"))).map_err(xml_err)?;
    Ok(())
}

fn write_pitch<W: Write>(w: &mut Writer<W>, p: &Pitch) -> Result<(), WriteError> {
    w.write_event(Event::Start(BytesStart::new("pitch"))).map_err(xml_err)?;
    let step_str = step_letter(p.step);
    write_text_element(w, "step", &step_str)?;
    if p.alter != 0 {
        write_text_element(w, "alter", &p.alter.to_string())?;
    }
    write_text_element(w, "octave", &p.octave.to_string())?;
    w.write_event(Event::End(BytesEnd::new("pitch"))).map_err(xml_err)?;
    Ok(())
}

fn step_letter(s: Step) -> String {
    s.letter().to_string()
}

fn accidental_name(a: Accidental) -> &'static str {
    match a {
        Accidental::Natural => "natural",
        Accidental::Sharp => "sharp",
        Accidental::Flat => "flat",
        Accidental::DoubleSharp => "double-sharp",
        Accidental::DoubleFlat => "flat-flat",
    }
}

fn write_text_element<W: Write>(
    w: &mut Writer<W>,
    name: &str,
    text: &str,
) -> Result<(), WriteError> {
    let cleaned = sanitize_xml_text(text);
    w.write_event(Event::Start(BytesStart::new(name.to_string())))
        .map_err(xml_err)?;
    w.write_event(Event::Text(BytesText::new(&cleaned))).map_err(xml_err)?;
    w.write_event(Event::End(BytesEnd::new(name.to_string())))
        .map_err(xml_err)?;
    Ok(())
}

/// XML 1.0 forbids most C0 control characters even when escaped. Strip
/// anything below U+0020 except TAB / LF / CR. Non-ASCII characters are
/// passed through; quick-xml will encode them as UTF-8.
fn sanitize_xml_text(s: &str) -> String {
    s.chars()
        .filter(|&c| c >= ' ' || c == '\t' || c == '\n' || c == '\r')
        .collect()
}

fn part_id(idx: usize) -> String {
    format!("P{}", idx + 1)
}

fn xml_err(e: quick_xml::Error) -> WriteError {
    WriteError::Xml(e.to_string())
}

