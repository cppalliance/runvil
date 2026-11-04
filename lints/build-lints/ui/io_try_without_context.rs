#![allow(dead_code)]

use std::fmt;
use std::io;

#[derive(Debug)]
struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error(error.to_string())
    }
}

fn io_op() -> io::Result<u8> {
    Ok(0)
}

fn spelled_out() -> Result<u8, std::io::Error> {
    Ok(0)
}

// Flagged: `From<io::Error>` converts with no context.
fn into_custom() -> Result<u8, Error> {
    let byte = io_op()?;
    Ok(byte)
}

// Flagged: the alias and the spelled-out type are the same type.
fn spelled_into_custom() -> Result<u8, Error> {
    Ok(spelled_out()?)
}

// Flagged: boxing erases the type but adds no context.
fn into_box() -> Result<u8, Box<dyn std::error::Error>> {
    Ok(io_op()?)
}

// Flagged: inside a closure returning another error type.
fn in_closure() -> Result<u8, Error> {
    let read = || -> Result<u8, Error> { Ok(io_op()?) };
    read()
}

// Not flagged: context added before `?`.
fn with_context(path: &str) -> Result<u8, Error> {
    Ok(io_op().map_err(|e| Error(format!("read {path}: {e}")))?)
}

// Not flagged: io to io is no conversion; the caller adds the context.
fn io_to_io() -> io::Result<u8> {
    Ok(io_op()? + spelled_out()?)
}

// Not flagged: not an io::Result.
fn other_result() -> Result<u8, Error> {
    let parsed: Result<u8, std::num::ParseIntError> = "1".parse();
    Ok(parsed.map_err(|e| Error(e.to_string()))?)
}

// Not flagged: `?` on an Option.
fn option() -> Option<u8> {
    Some(Some(1)?)
}

fn main() {}
