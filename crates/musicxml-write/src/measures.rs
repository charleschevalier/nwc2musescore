//! Group a staff's flat object stream into MusicXML measures (bounded by
//! `Bar` objects).

use nwc_model::{Staff, StaffObject};

#[derive(Debug, Default)]
pub struct Measure<'a> {
    pub objects: Vec<&'a StaffObject>,
    pub closing_bar: Option<&'a StaffObject>,
}

pub fn group(staff: &Staff) -> Vec<Measure<'_>> {
    let mut out: Vec<Measure<'_>> = Vec::new();
    let mut current = Measure::default();
    for obj in &staff.objects {
        match obj {
            StaffObject::Bar(_) => {
                current.closing_bar = Some(obj);
                out.push(std::mem::take(&mut current));
            }
            _ => current.objects.push(obj),
        }
    }
    if !current.objects.is_empty() || current.closing_bar.is_some() {
        out.push(current);
    }
    out
}
