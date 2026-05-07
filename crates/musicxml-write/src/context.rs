//! Writer state held across a single staff's measure stream.

#[derive(Debug, Default)]
pub struct WriterCtx {
    pub divisions: u32,
    pub measure_number: u32,
}

impl WriterCtx {
    pub fn new(divisions: u32) -> Self {
        Self {
            divisions,
            measure_number: 0,
        }
    }
}
