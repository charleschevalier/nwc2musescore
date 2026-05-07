//! Note-duration types.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteValue {
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    SixtyFourth,
}

impl NoteValue {
    /// Value as a fraction of a whole note: numerator/denominator.
    /// Whole = 1/1, Quarter = 1/4, etc.
    pub fn as_fraction(self) -> (u32, u32) {
        match self {
            NoteValue::Whole => (1, 1),
            NoteValue::Half => (1, 2),
            NoteValue::Quarter => (1, 4),
            NoteValue::Eighth => (1, 8),
            NoteValue::Sixteenth => (1, 16),
            NoteValue::ThirtySecond => (1, 32),
            NoteValue::SixtyFourth => (1, 64),
        }
    }

    /// MusicXML `<type>` text.
    pub fn musicxml_name(self) -> &'static str {
        match self {
            NoteValue::Whole => "whole",
            NoteValue::Half => "half",
            NoteValue::Quarter => "quarter",
            NoteValue::Eighth => "eighth",
            NoteValue::Sixteenth => "16th",
            NoteValue::ThirtySecond => "32nd",
            NoteValue::SixtyFourth => "64th",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Duration {
    pub base: NoteValue,
    pub dots: u8,
    pub tuplet: Option<Tuplet>,
}

impl Duration {
    /// Duration in arbitrary `divisions`-per-quarter ticks.
    pub fn in_divisions(self, divisions: u32) -> u32 {
        let (num, den) = self.base.as_fraction();
        // Quarter note = `divisions` ticks => whole note = `4 * divisions`.
        let whole = 4u32 * divisions;
        let mut ticks = whole * num / den;
        let mut dot_ticks = ticks / 2;
        for _ in 0..self.dots {
            ticks += dot_ticks;
            dot_ticks /= 2;
        }
        if let Some(t) = self.tuplet {
            ticks = ticks * t.normal_count as u32 / t.actual_count as u32;
        }
        ticks
    }
}

/// Position of a note inside a tuplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TupletPos {
    Start,
    Middle,
    End,
}

/// Tuplet ratio: `actual` notes in the time of `normal` notes.
/// E.g. a triplet is `Tuplet { actual_count: 3, normal_count: 2 }`.
#[derive(Debug, Clone, Copy)]
pub struct Tuplet {
    pub actual_count: u8,
    pub normal_count: u8,
}

impl Tuplet {
    pub fn triplet() -> Self {
        Tuplet { actual_count: 3, normal_count: 2 }
    }
}
