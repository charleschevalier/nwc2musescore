use thiserror::Error;

#[derive(Debug, Error)]
pub enum NwcError {
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("not a NoteWorthy file (header bytes: {header})")]
    NotNwc { header: String },

    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("zlib decompression failed: {0}")]
    Zlib(String),

    #[error("unexpected end of file at offset {offset} (parsing {context})")]
    UnexpectedEof { offset: usize, context: &'static str },

    #[error("unsupported NWC version {major}.{minor:02}")]
    UnsupportedVersion { major: u8, minor: u8 },

    #[error("malformed at offset {offset}: {message}")]
    Malformed { offset: usize, message: String },
}
