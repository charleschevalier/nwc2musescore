use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warn,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => f.write_str("info"),
            Severity::Warn => f.write_str("warn"),
            Severity::Error => f.write_str("error"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub offset: usize,
    pub message: String,
}

#[derive(Debug, Default, Clone)]
pub struct ConversionReport {
    pub diagnostics: Vec<Diagnostic>,
    pub objects_parsed: u32,
    pub objects_unknown: u32,
    pub bytes_skipped: u32,
}

impl ConversionReport {
    pub fn push(&mut self, severity: Severity, offset: usize, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            severity,
            offset,
            message: message.into(),
        });
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
}
