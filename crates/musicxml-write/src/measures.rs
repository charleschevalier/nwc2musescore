//! Group a staff's flat object stream into MusicXML measures (bounded by
//! `Bar` objects).

use nwc_model::{Staff, StaffObject};

#[derive(Debug, Default)]
pub struct Measure<'a> {
    pub objects: Vec<&'a StaffObject>,
    pub closing_bar: Option<&'a StaffObject>,
    /// True if this measure starts with a `RepeatOpen` carried over from
    /// the previous one's closing bar.
    pub opens_repeat: bool,
}

pub fn group(staff: &Staff) -> Vec<Measure<'_>> {
    let mut out: Vec<Measure<'_>> = Vec::new();
    let mut current = Measure::default();
    for obj in &staff.objects {
        match obj {
            StaffObject::Bar(_)
            | StaffObject::RepeatClose { .. } => {
                current.closing_bar = Some(obj);
                out.push(std::mem::take(&mut current));
            }
            StaffObject::RepeatOpen => {
                // A repeat-open marker closes the current measure (if it
                // already had any content) and the *next* measure opens
                // with the forward repeat.
                if !current.objects.is_empty() {
                    current.closing_bar = Some(obj);
                    out.push(std::mem::take(&mut current));
                }
                current.opens_repeat = true;
            }
            _ => current.objects.push(obj),
        }
    }
    if !current.objects.is_empty() || current.closing_bar.is_some() {
        out.push(current);
    }
    out
}
