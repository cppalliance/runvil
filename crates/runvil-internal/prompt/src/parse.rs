//! Parses a prompt's source into a [`Prompt`]: frontmatter, H1, and sections.

use std::sync::Arc;

use runvil_types::emitter::{Emitter, EventSink};
use runvil_types::error::{Error, ErrorCode};
use runvil_types::event::{Event, lifecycle};

use crate::frontmatter::SplitSource;
use crate::sections::{self, Heading, SectionWalker};
use crate::{Block, Frontmatter, Prompt, Result, contract, error, fence, frontmatter, position};

impl Prompt {
    /// Parses the full source text of a prompt file into a [`Prompt`] and
    /// returns the events reported during parsing alongside the result.
    ///
    /// The caller supplies `file`, the name of the file the source came
    /// from, such as `agents/research.md`, or `prompt` when it came from no
    /// file. [`Prompt::file`] returns it, so a failure can name the file.
    /// The caller also supplies `execution`, an identifier that every event
    /// carries. Every event is reported under task `0` because parsing happens
    /// before any run starts.
    ///
    /// The events come in this order: `ParseStarted`, then a
    /// `LuaCompilationStarted` event and a `LuaCompilationSucceeded` or
    /// `LuaCompilationFailed` event for each Lua block the parser compiles,
    /// then `ParseSucceeded` or `ParseFailed`. They exist only for the caller
    /// to log.
    ///
    /// # Errors
    /// On failure, the first element of the pair is an [`Error`] whose code
    /// is:
    ///
    /// - `FrontmatterInvalid` when the `---` delimiters are missing, the
    ///   frontmatter fails to decode, a `uses:` entry is neither a Pack id
    ///   nor vendor/name, two entries share a Pack id, or an entry names a
    ///   Pack id the Engine reserves. A refused `uses:` list has the
    ///   `Reason` fact `pack_duplicate` or `pack_reserved` and the Pack id
    ///   as its `Name` fact.
    /// - `StructureInvalid` when the H1 is missing, there is more than one
    ///   H1, the H1 title is empty, a section heading skips a level (such as
    ///   an H4 directly under an H2), a section heading is empty, or two
    ///   sibling sections have the same name.
    /// - `FenceInvalid` when a `lua` fence is left open, or anywhere in the
    ///   body a fence or line breaks the strict Lua fence rule. A `lua`
    ///   fence opens with a line that is exactly three backticks then `lua`
    ///   and closes with a line that is exactly three backticks. Label: a
    ///   fence's label starts with `lua` in any letter case without being
    ///   exactly `lua`, such as `Lua` or `lua extra`. Opener: a fence
    ///   labeled `lua` opens with any other line, such as one that is
    ///   indented, uses tildes or four backticks, or sits in a block quote
    ///   or list item. Closer: a fence with an exact opening line closes
    ///   with any other line. Lookalike: a line outside every fence looks
    ///   like a `lua` opening line, such as one in an indented code block.
    ///   The first violation in source order fails the parse at its line,
    ///   before any Lua compiles, with the rule it breaks as its `Reason`
    ///   fact: `label`, `opener`, `closer`, or `lookalike`. Lines inside the
    ///   body of another fenced code block are never inspected, so a
    ///   document can show a Lua fence as an example.
    /// - `ListEmptyItem` when a section made up only of list items has an
    ///   empty item.
    /// - `LuaCompile` when a Lua block in the H1 or in a section fails to
    ///   compile as Lua.
    /// - `Internal` when the parser fails through no fault of the prompt:
    ///   one of its own invariants breaks, or the Lua compiler cannot start.
    ///
    /// The failure's location, when known, is in `file`. A failure found
    /// past the frontmatter, other than an `Internal` one, has the prompt's
    /// frontmatter name as its `Name` fact.
    pub fn parse(
        input: &str,
        file: &str,
        execution: &str,
    ) -> (std::result::Result<Prompt, Error>, Vec<Event>) {
        let sink = EventSink::default();
        let emitter = Emitter::root(sink.clone(), execution, false);
        emitter.report("Prompt", lifecycle::PARSE_STARTED);
        // Everything past the frontmatter postdates the prompt's name, so a
        // failure from there on names it.
        let (name, result) = match Self::parse_frontmatter(input) {
            Ok((frontmatter, split)) => (
                Some(frontmatter.name().to_owned()),
                Self::parse_body(frontmatter, split, file, &emitter),
            ),
            Err(error) => (None, Err(error)),
        };
        emitter.report(
            "Prompt",
            if result.is_ok() {
                lifecycle::PARSE_SUCCEEDED
            } else {
                lifecycle::PARSE_FAILED
            },
        );
        let result =
            result.map_err(|error| error::with_prompt_context(error, name.as_deref(), file));
        (result, sink.take())
    }

    /// Splits the frontmatter from the body, decodes it, and checks its
    /// `uses:` list.
    fn parse_frontmatter(input: &str) -> Result<(Frontmatter, SplitSource)> {
        let split = frontmatter::split_frontmatter(input)?;
        let frontmatter: Frontmatter = serde_yaml_ng::from_str(&split.yaml)
            .map_err(|cause| error::frontmatter_decode(&cause))?;
        contract::check_distinct_packs(frontmatter.uses())?;
        contract::check_no_reserved_pack(frontmatter.uses())?;
        Ok((frontmatter, split))
    }

    fn parse_body(
        frontmatter: Frontmatter,
        split: SplitSource,
        file: &str,
        emitter: &Emitter,
    ) -> Result<Prompt> {
        let frontmatter_lines = split.frontmatter_lines;
        let shared: Arc<str> = Arc::from(split.body);
        let body: &str = &shared;
        let headings = sections::collect_headings(body)?;

        let h1_positions: Vec<usize> = headings
            .iter()
            .enumerate()
            .filter_map(|(index, heading)| (heading.level == 1).then_some(index))
            .collect();
        let [h1_index] = h1_positions.as_slice() else {
            if h1_positions.is_empty() {
                return Err(error::failure(
                    ErrorCode::StructureInvalid,
                    "prompt requires an H1 title",
                ));
            }
            let lines = h1_positions
                .iter()
                .map(|&index| {
                    position::line_add(frontmatter_lines, headings[index].body_line)
                        .map(|line| line.to_string())
                })
                .collect::<Result<Vec<_>>>()?;
            return Err(error::failure(
                ErrorCode::StructureInvalid,
                format!(
                    "prompt must contain exactly one H1 title, found {} at lines {}",
                    lines.len(),
                    lines.join(", ")
                ),
            ));
        };
        let h1 = &headings[*h1_index];
        if h1.title.is_empty() {
            return Err(error::failure(
                ErrorCode::StructureInvalid,
                "prompt H1 title must not be empty",
            ));
        }
        let title = h1.title.clone();
        let h1_content = h1.content;
        let h1_content_abs_line = position::line_add(frontmatter_lines, h1.content_body_line)?;
        if let Some(violation) = fence::first_fence_violation(body, frontmatter_lines) {
            return Err(violation);
        }

        // Everything before the H1 is preface and has no prompt semantics.
        // Sections are headings after the H1 at level 2 or deeper.
        let section_headings: Vec<Heading<'_>> = headings
            .into_iter()
            .skip(*h1_index + 1)
            .filter(|h| h.level >= 2)
            .collect();
        let mut walker = SectionWalker::new(&shared, &section_headings, frontmatter_lines, emitter);
        let mut h1_blocks = walker.compile_blocks(
            fence::split_section_blocks(h1_content, &title)?,
            h1_content_abs_line,
            &title,
            |_| format!("H1 `{title}` lua"),
        )?;
        if matches!(
            h1_blocks.as_slice(),
            [Block::Prose { text }] if text.as_str().is_empty()
        ) {
            h1_blocks.clear();
        }
        let sections = walker.build_sections(1)?;

        Ok(Prompt {
            file: file.to_owned(),
            frontmatter,
            title,
            h1_blocks,
            sections,
        })
    }
}
