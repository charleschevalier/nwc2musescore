//! Tiny binary-reading helper. NWC files are little-endian, with a mix of
//! fixed-width integers and null-terminated strings.

use crate::error::NwcError;
use byteorder::{ByteOrder, LittleEndian};

pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn peek_bytes(&self, n: usize) -> Option<&'a [u8]> {
        self.data.get(self.pos..self.pos.checked_add(n)?)
    }

    pub fn take(&mut self, n: usize, ctx: &'static str) -> Result<&'a [u8], NwcError> {
        let end = self.pos.checked_add(n).ok_or(NwcError::UnexpectedEof {
            offset: self.pos,
            context: ctx,
        })?;
        if end > self.data.len() {
            return Err(NwcError::UnexpectedEof { offset: self.pos, context: ctx });
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub fn skip(&mut self, n: usize, ctx: &'static str) -> Result<(), NwcError> {
        self.take(n, ctx).map(|_| ())
    }

    pub fn read_u8(&mut self, ctx: &'static str) -> Result<u8, NwcError> {
        Ok(self.take(1, ctx)?[0])
    }

    pub fn read_i8(&mut self, ctx: &'static str) -> Result<i8, NwcError> {
        Ok(self.take(1, ctx)?[0] as i8)
    }

    pub fn read_u16_le(&mut self, ctx: &'static str) -> Result<u16, NwcError> {
        Ok(LittleEndian::read_u16(self.take(2, ctx)?))
    }

    pub fn read_u32_le(&mut self, ctx: &'static str) -> Result<u32, NwcError> {
        Ok(LittleEndian::read_u32(self.take(4, ctx)?))
    }

    /// Read bytes until (and consuming) a NUL terminator. The terminator is
    /// not part of the returned slice.
    pub fn read_cstr(&mut self, ctx: &'static str) -> Result<&'a [u8], NwcError> {
        let start = self.pos;
        let nul = self.data[start..]
            .iter()
            .position(|&b| b == 0)
            .ok_or(NwcError::UnexpectedEof { offset: start, context: ctx })?;
        let slice = &self.data[start..start + nul];
        self.pos = start + nul + 1;
        Ok(slice)
    }

    /// Read a NUL-terminated string and decode as Windows-1252 (with UTF-8
    /// best-effort fallback for clean ASCII / UTF-8 input).
    pub fn read_cstr_lossy(&mut self, ctx: &'static str) -> Result<String, NwcError> {
        let bytes = self.read_cstr(ctx)?;
        Ok(decode_string(bytes))
    }

    pub fn expect_tag(&mut self, tag: &[u8], ctx: &'static str) -> Result<(), NwcError> {
        let got = self.take(tag.len(), ctx)?;
        if got != tag {
            return Err(NwcError::Malformed {
                offset: self.pos - tag.len(),
                message: format!("expected tag {:?}, got {:?}", tag, got),
            });
        }
        Ok(())
    }
}

/// Decode a byte slice as a string, preferring UTF-8 if it parses, else
/// Windows-1252. NWC files are typically Windows-1252 but ASCII-clean files
/// are valid UTF-8 too.
pub fn decode_string(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let (cow, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
    cow.into_owned()
}
