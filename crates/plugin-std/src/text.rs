//! How the file tools turn a file's bytes into lines of text.
//!
//! Read and Grep both go through this module, so a line number Grep
//! reports is the number Read prints. Nothing here returns an error to
//! the model for an encoding: bytes that are not valid text are replaced
//! and counted, and a file that is not text at all is told apart by
//! [`sniff_binary`].

/// The largest file, in bytes, a file tool reads: 16 MiB.
pub(crate) const MAX_FILE_BYTES: u64 = 16_777_216;

/// The most bytes of tree or match lines a listing or search returns:
/// 15,000. ListDir and Grep share it.
pub(crate) const BODY_CAP: usize = 15_000;

/// How many leading bytes [`sniff_binary`] looks at.
const SNIFF_BYTES: usize = 8_192;

/// A file's bytes decoded to text.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Decoded {
    /// The text, without a leading byte order mark.
    pub(crate) text: String,
    /// `UTF-16LE` or `UTF-16BE` when the file started with that mark,
    /// and `None` for anything else.
    pub(crate) utf16: Option<&'static str>,
    /// How many invalid UTF-8 sequences were shown as U+FFFD. It is `0`
    /// for a UTF-16 file.
    pub(crate) invalid: usize,
}

/// Decodes `bytes` to text.
///
/// A leading UTF-8 mark (`EF BB BF`) is dropped. A file that starts with
/// `FF FE` or `FE FF` is UTF-16, little-endian or big-endian: the mark is
/// dropped and the rest is decoded lossily, and a trailing odd byte is
/// ignored. Anything else is decoded as UTF-8, and each invalid chunk
/// becomes one U+FFFD and one count. No other encoding is guessed.
pub(crate) fn decode(bytes: &[u8]) -> Decoded {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, "UTF-16LE", u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, "UTF-16BE", u16::from_be_bytes);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let mut text = String::with_capacity(bytes.len());
    let mut invalid = 0;
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            text.push(char::REPLACEMENT_CHARACTER);
            invalid += 1;
        }
    }
    Decoded {
        text,
        utf16: None,
        invalid,
    }
}

/// Decodes the UTF-16 `bytes` that follow a mark, reading each unit with
/// `unit`, and names the byte order `name`.
fn utf16(bytes: &[u8], name: &'static str, unit: fn([u8; 2]) -> u16) -> Decoded {
    let (pairs, _odd_byte) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|pair| unit(*pair)).collect();
    Decoded {
        text: String::from_utf16_lossy(&units),
        utf16: Some(name),
        invalid: 0,
    }
}

/// Whether `bytes` look like binary data rather than text.
///
/// Only the first 8,192 bytes count. They are binary when any of them is
/// NUL, or when at least 5% of them are control bytes other than tab,
/// line feed, and carriage return. A file that starts with a UTF-16 mark
/// is text whatever it holds, because UTF-16 text has NUL bytes by
/// design; UTF-16 without a mark is binary.
pub(crate) fn sniff_binary(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return false;
    }
    let sample = bytes.get(..SNIFF_BYTES).unwrap_or(bytes);
    if sample.contains(&0) {
        return true;
    }
    let controls = sample
        .iter()
        .filter(|byte| byte.is_ascii_control() && !matches!(**byte, b'\t' | b'\n' | b'\r'))
        .count();
    !sample.is_empty() && controls * 20 >= sample.len()
}

/// Splits `text` into lines on `\n`.
///
/// The empty piece after a final newline is not a line, and one trailing
/// `\r` is stripped from each line. So `a\nb\n` and `a\nb` are both two
/// lines, an empty text has none, and a lone `\r` inside a line stays in
/// it.
pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    text.split_terminator('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// The part of `text` from byte `start` to byte `end`, cut on character
/// boundaries.
///
/// `start` is rounded up and `end` down to the nearest boundary, and both
/// are clamped to the length of `text`. This is the only function that
/// shortens text inside a line, so no caller slices on a raw byte index.
pub(crate) fn clip_to_boundary(text: &str, start: usize, end: usize) -> &str {
    let length = text.len();
    let mut start = start.min(length);
    let mut end = end.min(length);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(start..end).unwrap_or("")
}

#[cfg(test)]
mod tests;
