//! Walks a prompt body's markdown headings and builds its [`Section`] tree.

use std::ops::Range;
use std::sync::Arc;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use runvil_types::error::{Error, ErrorCode};

use crate::fence::{self, LuaRole, RawBlock};
use crate::position::{body_text, file_line_of, line_add, newlines_before, nz_source_line};
use crate::{Block, Result, Section, error};
use runvil_lua::LuaProgram;

/// A heading with its title and the prose/Lua that follows it (before the next
/// heading of any level).
#[derive(Debug, Clone)]
pub(crate) struct Heading<'a> {
    pub(crate) level: u8,
    /// The heading's text, trimmed.
    pub(crate) title: String,
    pub(crate) content: &'a str,
    /// 1-based line number within `body` where `content` begins.
    pub(crate) content_body_line: u32,
    /// 1-based line number within `body` of the heading line itself.
    pub(crate) body_line: u32,
    /// Byte range of the heading within `body`, kept for span diagnostics.
    span: Range<usize>,
}

/// Walks the markdown body and collects every heading with the content that
/// follows it, up to the next heading of any level.
///
/// A heading inside a block quote or list item is not a section boundary; its
/// text stays in the enclosing heading's content.
pub(crate) fn collect_headings(body: &str) -> Result<Vec<Heading<'_>>> {
    // First pass: find each heading's level, title, and source byte range.
    struct Raw {
        level: u8,
        title: String,
        range: Range<usize>,
    }
    let mut raws: Vec<Raw> = Vec::new();
    let mut current: Option<(u8, Range<usize>, String)> = None;
    let mut container_depth: usize = 0;

    for (event, range) in Parser::new_ext(body, Options::empty()).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_) | Tag::Item) => container_depth += 1,
            Event::End(TagEnd::BlockQuote(_) | TagEnd::Item) => {
                container_depth = container_depth.saturating_sub(1);
            }
            Event::Start(Tag::Heading { level, .. }) if container_depth == 0 => {
                current = Some((level as u8, range.clone(), String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, range, title)) = current.take() {
                    raws.push(Raw {
                        level,
                        title: title.trim().to_string(),
                        range,
                    });
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, _, ref mut title)) = current {
                    title.push_str(&t);
                }
            }
            _ => {}
        }
    }

    // Second pass: the content of each heading runs from the end of its
    // heading to the start of the next heading (or the end of the body).
    let mut headings = Vec::with_capacity(raws.len());
    let mut raws = raws.into_iter().peekable();
    while let Some(raw) = raws.next() {
        let start = raw.range.end;
        let end = raws.peek().map_or(body.len(), |next| next.range.start);
        // +1 because newlines_before gives 0-based offset, and we want
        // the 1-based line number of the first content line.
        let content_body_line = line_add(newlines_before(body, start)?, 1)?;
        let body_line = line_add(newlines_before(body, raw.range.start)?, 1)?;
        headings.push(Heading {
            level: raw.level,
            title: raw.title,
            content: &body[start..end],
            content_body_line,
            body_line,
            span: raw.range,
        });
    }
    Ok(headings)
}

/// Compiles a prompt body's H1 and section blocks, and builds its section
/// tree.
///
/// [`SectionWalker::build_sections`] consumes `headings` from `pos` onward, so
/// one walker builds one tree.
pub(crate) struct SectionWalker<'a> {
    /// The prompt body that every heading and block is sliced from.
    shared: &'a Arc<str>,
    /// The section headings, every heading after the H1 at level 2 or deeper,
    /// in document order.
    headings: &'a [Heading<'a>],
    /// The file lines before the body, added to a body line to give its file
    /// line.
    frontmatter_lines: u32,
    /// The file the prompt came from, which a block's compile failure names.
    file: &'a str,
    /// Index in `headings` of the next heading to build.
    pos: usize,
}

impl<'a> SectionWalker<'a> {
    /// Returns a walker positioned at the first of `headings`.
    pub(crate) fn new(
        shared: &'a Arc<str>,
        headings: &'a [Heading<'a>],
        frontmatter_lines: u32,
        file: &'a str,
    ) -> Self {
        Self {
            shared,
            headings,
            frontmatter_lines,
            file,
            pos: 0,
        }
    }

    /// Builds a section tree from the walker's flat, document-ordered headings.
    ///
    /// Recursion consumes headings whose level is deeper than `parent_level`; a
    /// heading at or above `parent_level` belongs to an ancestor and stops the
    /// current level. A heading that skips a level (an orphan deep heading, such
    /// as an H4 directly under an H2, or an H3 top-level section with no parent
    /// H2) is rejected: every heading must be exactly one level deeper than its
    /// parent.
    ///
    /// Each section's checks run in a fixed order that decides which error a
    /// malformed prompt reports: its level, its name, its blocks, its children,
    /// and last its name against earlier siblings.
    ///
    /// # Errors
    /// Returns a `StructureInvalid` failure at the heading for an orphan
    /// heading, an empty heading, or duplicate sibling names, and the
    /// failure for a malformed `lua` fence. Propagates a Lua compile
    /// failure, and an `Internal` failure from invalid or overflowing
    /// source-line calculations, from a fence split that disagrees with the
    /// Markdown parser, or from section text that is not a slice of
    /// `shared`.
    pub(crate) fn build_sections(&mut self, parent_level: u8) -> Result<Vec<Section>> {
        let headings = self.headings;
        let mut result = Vec::new();
        // Parallel to `result`: each sibling's name and its 1-based heading line, so
        // a duplicate can name both the first and the offending location.
        let mut sibling_lines: Vec<(String, u32)> = Vec::new();
        while self.pos < headings.len() {
            let h = &headings[self.pos];
            let level = h.level;
            if level <= parent_level {
                break;
            }
            self.check_child_level(h, parent_level)?;
            let name = self.section_name(h)?;
            let heading_abs_line = line_add(self.frontmatter_lines, h.body_line)?;
            let location = self.section_location(&name, heading_abs_line);
            let blocks = self.build_heading_blocks(h, &name, &location)?;
            self.pos += 1;
            let children = self.build_sections(level)?;
            self.check_unique_sibling(&sibling_lines, &name, h, heading_abs_line)?;
            sibling_lines.push((name.clone(), heading_abs_line));

            let markdown = fence::trim_leading_blank_lines(h.content).trim_end();
            result.push(Section {
                name,
                level,
                blocks,
                children,
                markdown: body_text(self.shared, markdown)?,
                markdown_line: file_line_of(self.shared, markdown, self.frontmatter_lines)?,
            });
        }
        Ok(result)
    }

    /// Rejects an orphan deep heading, one that skips a level.
    ///
    /// Examples are an H4 under an H2 with no intervening H3, or an H3/H4
    /// top-level section with no parent H2. Such a heading has no well-defined
    /// parent, so reject it rather than silently reparenting it to a shallower
    /// ancestor.
    fn check_child_level(&self, heading: &Heading<'_>, parent_level: u8) -> Result<()> {
        let level = heading.level;
        if level > parent_level + 1 {
            return Err(self.make_structure_failure_error(
                heading,
                format!(
                    "section `{}` is an orphan H{level} heading with no parent H{}",
                    heading.title,
                    parent_level + 1
                ),
            ));
        }
        Ok(())
    }

    /// Returns the heading's section name.
    ///
    /// A section's name is its runtime address (jumps, lookups, fanout), so an
    /// empty/whitespace heading is unaddressable and must be rejected at parse.
    fn section_name(&self, heading: &Heading<'_>) -> Result<String> {
        let name = heading.title.clone();
        if name.is_empty() {
            return Err(self.make_structure_failure_error(
                heading,
                format!("an H{} section heading must not be empty", heading.level),
            ));
        }
        Ok(name)
    }

    /// Rejects a section whose name an earlier sibling already took.
    ///
    /// Sections are addressed by name (jumps, lookups), so two siblings sharing a
    /// name would make the target ambiguous. The error names BOTH heading
    /// locations so the author can find each one.
    fn check_unique_sibling(
        &self,
        siblings: &[(String, u32)],
        name: &str,
        heading: &Heading<'_>,
        heading_abs_line: u32,
    ) -> Result<()> {
        if let Some((_, first_line)) = siblings.iter().find(|(n, _)| n == name) {
            return Err(self.make_structure_failure_error(
                heading,
                format!(
                    "duplicate sibling section name `{name}`: first declared at line {first_line}, again at line {heading_abs_line}; sibling section names must be unique"
                ),
            ));
        }
        Ok(())
    }

    /// Builds a structure failure located at `heading`.
    fn make_structure_failure_error(&self, heading: &Heading<'_>, message: String) -> Error {
        error::make_failure_at_error(
            ErrorCode::StructureInvalid,
            message,
            self.shared,
            heading.span.clone(),
            self.frontmatter_lines,
        )
    }

    /// Builds one heading's executable blocks while preserving source positions.
    ///
    /// `location` names the section in each Lua block's location.
    fn build_heading_blocks(
        &self,
        heading: &Heading<'_>,
        name: &str,
        location: &str,
    ) -> Result<Vec<Block>> {
        let content_abs_line = line_add(self.frontmatter_lines, heading.content_body_line)?;
        let raw_blocks = fence::split_section_blocks(heading.content, name, content_abs_line)?;
        self.compile_blocks(raw_blocks, content_abs_line, |role| {
            fence::lua_block_location(location, role)
        })
    }

    /// Compiles split content into blocks, in source order.
    ///
    /// Every prose block must be a slice of `shared`, which it reads from in
    /// place, and its file line is where that slice starts.
    /// `content_abs_line` is the 1-based file line where the content begins,
    /// and `location` gives each Lua block's compile location from its role.
    ///
    /// # Errors
    /// Returns a Lua compile failure for a block that does not compile, and
    /// an `Internal` failure for a compiler that cannot start, for an
    /// overflowing or zero source line, or for prose that is not a slice of
    /// `shared`.
    pub(crate) fn compile_blocks(
        &self,
        raw_blocks: Vec<RawBlock<'_>>,
        content_abs_line: u32,
        location: impl Fn(&LuaRole) -> String,
    ) -> Result<Vec<Block>> {
        let mut blocks = Vec::with_capacity(raw_blocks.len());
        for raw in raw_blocks {
            match raw {
                RawBlock::Prose(text) => blocks.push(Block::Prose {
                    text: body_text(self.shared, text)?,
                    line: file_line_of(self.shared, text, self.frontmatter_lines)?,
                }),
                RawBlock::Lua {
                    source,
                    line_offset,
                    role,
                } => {
                    let abs_line = line_add(content_abs_line, line_offset)?;
                    let program = LuaProgram::compile(
                        &source,
                        self.file,
                        &location(&role),
                        nz_source_line(abs_line)?,
                    )
                    .map_err(|cause| error::make_lua_error(&cause))?;
                    blocks.push(Block::Lua(program));
                }
            }
        }
        Ok(blocks)
    }

    /// Names a section in its Lua block locations.
    ///
    /// The name is the title alone, or the title and its heading line when
    /// another section in `headings` shares the title, so a reader can tell the
    /// two sections' errors apart. The locations are display names; each block's
    /// line-based chunk name, not its location, decides which program maps an
    /// error.
    fn section_location(&self, name: &str, heading_abs_line: u32) -> String {
        if self
            .headings
            .iter()
            .filter(|other| other.title == name)
            .nth(1)
            .is_some()
        {
            format!("section `{name}` (line {heading_abs_line})")
        } else {
            format!("section `{name}`")
        }
    }
}
