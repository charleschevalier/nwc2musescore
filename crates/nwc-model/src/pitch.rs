//! Pitch types.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    C, D, E, F, G, A, B,
}

impl Step {
    pub fn letter(self) -> char {
        match self {
            Step::C => 'C', Step::D => 'D', Step::E => 'E', Step::F => 'F',
            Step::G => 'G', Step::A => 'A', Step::B => 'B',
        }
    }

    /// 0..=6, C-relative — used as an internal NWC-style staff index.
    pub fn diatonic(self) -> u8 {
        match self {
            Step::C => 0, Step::D => 1, Step::E => 2, Step::F => 3,
            Step::G => 4, Step::A => 5, Step::B => 6,
        }
    }

    pub fn from_diatonic(n: u8) -> Self {
        match n % 7 {
            0 => Step::C, 1 => Step::D, 2 => Step::E, 3 => Step::F,
            4 => Step::G, 5 => Step::A, 6 => Step::B,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accidental {
    Natural,
    Sharp,
    Flat,
    DoubleSharp,
    DoubleFlat,
}

impl Accidental {
    pub fn alter(self) -> i8 {
        match self {
            Accidental::Natural => 0,
            Accidental::Sharp => 1,
            Accidental::Flat => -1,
            Accidental::DoubleSharp => 2,
            Accidental::DoubleFlat => -2,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Pitch {
    pub step: Step,
    /// Scientific octave (middle C = 4).
    pub octave: i8,
    /// Effective pitch alteration in semitones, after key-signature is applied.
    pub alter: i8,
    /// Accidental shown in front of the note, if any.
    pub displayed_accidental: Option<Accidental>,
}
