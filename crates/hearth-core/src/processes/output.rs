//! A process's output, read from its log in pieces: the page asks again with the offset it got.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::Serialize;

/// The most text one read returns.
const MAX: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Output {
    /// Where the next read starts.
    pub offset: u64,
    pub text: String,
    /// The log was emptied (a restart): `text` starts over and replaces what was shown.
    pub reset: bool,
}

/// Text from `offset` on (or the last 256 KB without one), color codes stripped.
pub fn read(log: &Path, offset: Option<u64>) -> Output {
    let Ok(mut file) = std::fs::File::open(log) else {
        return Output { offset: 0, text: String::new(), reset: offset.is_some_and(|o| o > 0) };
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let reset = offset.is_some_and(|o| o > len);
    let wanted = offset.filter(|_| !reset).unwrap_or(0);
    // A first read (or one far behind) gets only the end, cut at a line start.
    let (start, cut) = if len - wanted > MAX { (len - MAX, true) } else { (wanted, false) };
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_ok() {
        let _ = file.take(len - start).read_to_end(&mut bytes);
    }
    let mut from = 0;
    if cut {
        from = bytes.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1);
    }
    let text = strip_ansi(&String::from_utf8_lossy(&bytes[from..]));
    Output { offset: start + bytes.len() as u64, text, reset }
}

/// Removes terminal escape sequences: CSI (`ESC [ ... final`), OSC (`ESC ] ... BEL/ST`) and
/// two-character ones.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}
