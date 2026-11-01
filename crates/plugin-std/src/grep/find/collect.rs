//! Gathering the entries the answer shows: which entries are kept, and
//! the lines each one keeps.
//!
//! Entries are counted in walk order across all files. The first `offset`
//! are only counted, the next `head_limit` are kept, and one more is
//! counted to show the head limit was reached. A kept line is stored as
//! the text the answer will show, so no decoded file is held after it is
//! searched.

use std::collections::BTreeMap;

use super::Search;
use crate::grep::found::{FileHit, Row};
use crate::grep::matching::each_match_line;
use crate::grep::render::{GAP_COST, row_cost, summary_cost};
use crate::grep::source::Source;
use crate::grep::window::{context_text, match_text};

/// What to do with the next entry.
#[derive(Debug, PartialEq, Eq)]
enum Admission {
    /// It comes before the offset: it is counted and nothing more.
    Skip,
    /// It is kept.
    Keep,
    /// It is the one past the head limit, which is counted and not kept.
    Extra,
}

impl<S: Source> Search<'_, '_, S> {
    /// Decides what the next entry is: one before the offset that is only
    /// counted, one to keep, or the extra one that shows the head limit
    /// was reached.
    fn admit(&mut self) -> Admission {
        let index = self.found.seen;
        self.found.seen += 1;
        let offset = self.request.offset;
        if index < offset {
            return Admission::Skip;
        }
        if self
            .request
            .limit
            .is_some_and(|limit| index - offset >= limit)
        {
            self.found.extra = true;
            self.found.collected += 1;
            self.finished = true;
            return Admission::Extra;
        }
        self.found.collected += 1;
        Admission::Keep
    }

    /// Ends the search when the kept lines already fill the body cap, so
    /// the entries past them, which are never shown, are not gathered.
    fn check_cap(&mut self) {
        if self.cost > self.limits.body_cap {
            self.found.capped = true;
            self.finished = true;
        }
    }

    /// Gathers one file's entry for `files_with_matches` or `count`.
    pub(super) fn collect_file(&mut self, path: &str, count: usize) {
        if self.admit() != Admission::Keep {
            return;
        }
        self.cost += summary_cost(self.request.mode, path, count);
        self.found.hits.push(FileHit {
            path: path.to_owned(),
            count,
            rows: BTreeMap::new(),
        });
        self.check_cap();
    }

    /// Gathers the matching lines of one file for `content` mode.
    pub(super) fn collect_lines(&mut self, path: &str, text: &str, lines: &[&str]) {
        let mut hit: Option<FileHit> = None;
        let request = self.request;
        each_match_line(
            &request.regex,
            request.multiline,
            text,
            lines,
            |line, first| match self.admit() {
                Admission::Skip => true,
                Admission::Extra => false,
                Admission::Keep => {
                    let hit = hit.get_or_insert_with(|| self.begin(path));
                    self.keep(hit, lines, line, first);
                    self.check_cap();
                    !self.finished
                }
            },
        );
        if let Some(hit) = hit {
            self.found.hits.push(hit);
        }
    }

    /// Starts the result of the file at `path`, and counts the lines its
    /// path and the blank line before it take.
    fn begin(&mut self, path: &str) -> FileHit {
        self.cost += path.len() + 1 + usize::from(!self.found.hits.is_empty());
        FileHit {
            path: path.to_owned(),
            count: 0,
            rows: BTreeMap::new(),
        }
    }

    /// Keeps the matching line at index `line` of `lines`, whose first
    /// match starts at byte `first`, with the context lines around it.
    ///
    /// Lines are kept in order, and stop being kept once the cost passes
    /// the body cap, because no line past that point is shown.
    fn keep(&mut self, hit: &mut FileHit, lines: &[&str], line: usize, first: usize) {
        let request = self.request;
        let window = self.limits.window;
        let from = line.saturating_sub(request.before);
        let to = line
            .saturating_add(request.after)
            .min(lines.len().saturating_sub(1));
        for index in from..=to {
            if self.cost > self.limits.body_cap {
                break;
            }
            let number = index + 1;
            let is_match = index == line;
            if !is_match && hit.rows.contains_key(&number) {
                continue;
            }
            let Some(source) = lines.get(index) else {
                break;
            };
            let text = if is_match {
                match_text(source, first, &window)
            } else {
                context_text(source, &window)
            };
            self.cost += row_cost(number, &text);
            if let Some(old) = hit.rows.insert(number, Row { text, is_match }) {
                self.cost -= row_cost(number, &old.text);
            } else if (request.before > 0 || request.after > 0)
                && hit
                    .rows
                    .range(..number)
                    .next_back()
                    .is_some_and(|(previous, _)| number > previous + 1)
            {
                self.cost += GAP_COST;
            }
        }
    }
}
