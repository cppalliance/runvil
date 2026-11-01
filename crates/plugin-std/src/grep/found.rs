//! What a search finds and the limits it runs under.
//!
//! [`find`](super::find::find) fills a [`Found`] and writes no text, and
//! [`render`](super::render::render) turns it into the answer.

use std::collections::BTreeMap;

use super::window::Window;
use crate::text::{BODY_CAP, MAX_FILE_BYTES};

/// The most files one search considers.
pub(super) const MAX_FILES: usize = 100_000;

/// The most bytes one search reads: 1 GiB.
const MAX_BYTES: u64 = 1_073_741_824;

/// How many files are considered between two pauses.
const SLICE_FILES: usize = 32;

/// How many bytes are read between two pauses: 2 MiB.
const SLICE_BYTES: u64 = 2_097_152;

/// How many skipped paths the skip line names.
const FIRST_KEPT: usize = 5;

/// The limits a search runs under. Each is a parameter, with the
/// constant as its default, so a test can lower it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    /// The most files considered before the search stops.
    pub(super) max_files: usize,
    /// The most bytes read before the search stops.
    pub(super) max_bytes: u64,
    /// The largest file, by the size its listing gives, that is searched.
    pub(super) max_file_size: u64,
    /// The files considered between two pauses.
    pub(super) slice_files: usize,
    /// The bytes read between two pauses.
    pub(super) slice_bytes: u64,
    /// The most bytes of body in the answer.
    pub(super) body_cap: usize,
    /// How much of a long line is shown.
    pub(super) window: Window,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_files: MAX_FILES,
            max_bytes: MAX_BYTES,
            max_file_size: MAX_FILE_BYTES,
            slice_files: SLICE_FILES,
            slice_bytes: SLICE_BYTES,
            body_cap: BODY_CAP,
            window: Window::default(),
        }
    }
}

/// Why an entry was skipped, in the order the skip line names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reason {
    /// A binary file.
    Binary,
    /// A directory whose listing failed.
    UnreadableDirectory,
    /// A file that could not be read.
    UnreadableFile,
    /// A file whose listed size is over the limit.
    OverSize,
    /// A symbolic link, which is never entered or read.
    SymbolicLink,
    /// An entry whose name has a control character.
    ControlName,
    /// An entry another call in this round holds a claim on.
    Conflict,
    /// An entry the run's policy refuses.
    Denied,
}

impl Reason {
    /// Every reason, in the order the skip line names them.
    pub(super) const ALL: [Reason; 8] = [
        Reason::Binary,
        Reason::UnreadableDirectory,
        Reason::UnreadableFile,
        Reason::OverSize,
        Reason::SymbolicLink,
        Reason::ControlName,
        Reason::Conflict,
        Reason::Denied,
    ];
}

/// The entries a search skipped, counted by reason.
#[derive(Debug, Default)]
pub(super) struct Skips {
    /// How many entries were skipped for each reason in [`Reason::ALL`].
    counts: [usize; 8],
    /// The first printable paths skipped, in walk order.
    first: Vec<String>,
}

impl Skips {
    /// Counts one skip for `reason`. `path` is the entry's path, or `None`
    /// for a name that is never printed.
    pub(super) fn record(&mut self, reason: Reason, path: Option<&str>) {
        self.counts[reason as usize] += 1;
        if let Some(path) = path
            && self.first.len() < FIRST_KEPT
        {
            self.first.push(path.to_owned());
        }
    }

    /// How many entries were skipped for `reason`.
    pub(super) fn count(&self, reason: Reason) -> usize {
        self.counts[reason as usize]
    }

    /// How many entries were skipped for any reason.
    pub(super) fn total(&self) -> usize {
        self.counts.iter().sum()
    }

    /// The first printable paths skipped, in walk order.
    pub(super) fn first(&self) -> &[String] {
        &self.first
    }
}

/// The files whose invalid UTF-8 was shown as U+FFFD.
#[derive(Debug, Default)]
pub(super) struct Replaced {
    /// How many searched files needed a replacement.
    pub(super) count: usize,
    /// The first of their paths, in walk order.
    pub(super) first: Vec<String>,
}

impl Replaced {
    /// Notes that the file at `path` needed a replacement.
    pub(super) fn record(&mut self, path: &str) {
        self.count += 1;
        if self.first.len() < FIRST_KEPT {
            self.first.push(path.to_owned());
        }
    }
}

/// A limit that ended the search before the tree was done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stop {
    /// The search considered the most files it may.
    Files,
    /// The search read the most bytes it may.
    Bytes,
}

/// One line of a file's result: what is shown after its number.
#[derive(Debug)]
pub(super) struct Row {
    /// The text shown, windowed or clipped when the line is long.
    pub(super) text: String,
    /// Whether the line is an entry. A context line is not.
    pub(super) is_match: bool,
}

/// What one file adds to the result.
#[derive(Debug)]
pub(super) struct FileHit {
    /// The file's absolute virtual path.
    pub(super) path: String,
    /// The file's matching-line count, which `count` mode shows.
    pub(super) count: usize,
    /// The lines `content` mode shows, by line number from 1, in order.
    pub(super) rows: BTreeMap<usize, Row>,
}

/// The outcome of a search that did not fail.
#[derive(Debug, Default)]
pub(super) struct Found {
    /// The files that hold a kept entry, in walk order.
    pub(super) hits: Vec<FileHit>,
    /// The entries gathered after the offset: the ones kept, and the one
    /// extra that only shows the head limit was reached.
    pub(super) collected: usize,
    /// Whether the extra entry was found.
    pub(super) extra: bool,
    /// Whether the search stopped collecting because the kept lines
    /// already fill the body cap.
    pub(super) capped: bool,
    /// How many entries the search counted, the ones before the offset
    /// included.
    pub(super) seen: usize,
    /// How many files were searched.
    pub(super) searched: usize,
    /// The entries that were skipped.
    pub(super) skips: Skips,
    /// The files that needed a replacement.
    pub(super) replaced: Replaced,
    /// The limit that ended the search early, when one did.
    pub(super) stop: Option<Stop>,
}
