//! Splits a prompt's YAML frontmatter from its body and models what it declares.

use std::num::NonZeroU32;

use runvil_types::error::ErrorCode;
use runvil_types::models::ModelRole;
use runvil_types::packs::PackRef;

use crate::contract::RoleMap;
use crate::{ArgsDecl, Result, RoleRequirements, error};

/// A declared input or output file in a prompt's frontmatter.
///
/// The `path` is the file's name in the store, which the prompt reads or
/// writes. The `description` documents the file's purpose for the caller.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileDecl {
    /// The store-internal path (e.g. `"paper.md"`).
    path: String,
    /// Human-readable purpose of this file.
    description: String,
}

impl FileDecl {
    /// Returns the file's path in the store, such as `paper.md`.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the human-readable description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// The parsed frontmatter of a prompt file.
///
/// Parsing accepts only the keys this schema defines. Any other key, such as
/// a misspelled or extra one, is a prompt authoring error and fails the parse.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frontmatter {
    /// The prompt's identifier, from the `name:` key.
    name: String,
    /// The prompt's one-line summary, from the `description:` key.
    description: String,
    /// The runvil Engine major this file targets, when declared.
    ///
    /// Its presence marks the file as a runvil prompt; `None` means the
    /// file is not one.
    #[serde(default)]
    runvil: Option<u32>,
    /// Maximum model round trips a section's tool-call loop may take.
    ///
    /// An absent value leaves the runtime's own cap in force. A declared
    /// value is a positive, bounded count: deserialization rejects `0`,
    /// negatives, and values above [`MAX_TOOL_ITERATIONS`], so no invalid
    /// cap can reach execution.
    #[serde(default, deserialize_with = "max_tool_iterations")]
    max_tool_iterations: Option<NonZeroU32>,
    /// A file the prompt expects to find in the store when it starts.
    #[serde(default)]
    input: Option<FileDecl>,
    /// A file the prompt will leave in the store when it finishes.
    #[serde(default)]
    output: Option<FileDecl>,
    /// Packs the prompt declares, each a bare Pack id or vendor/name, in
    /// declaration order: each is required, and its prelude runs in every
    /// section.
    #[serde(default)]
    uses: Vec<PackRef>,
    /// The typed args declaration; an absent `args:` key yields the default
    /// [`ArgsDecl`].
    #[serde(default)]
    args: ArgsDecl,
    /// The requirements declared per model role.
    #[serde(default)]
    models: RoleMap,
}

/// The largest explicit `max_tool_iterations` a prompt may declare.
pub(crate) const MAX_TOOL_ITERATIONS: u32 = 1000;

/// Deserializes the `max_tool_iterations` cap: absent, or a positive count no
/// larger than [`MAX_TOOL_ITERATIONS`].
///
/// Deserialization reads an `i64` so negatives and values above `u32` are
/// rejected instead of wrapped, and rejects `0` so every accepted value is a
/// [`NonZeroU32`] and no invalid cap can reach execution.
fn max_tool_iterations<'de, D>(deserializer: D) -> std::result::Result<Option<NonZeroU32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(raw) = <Option<i64> as serde::Deserialize>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if raw <= 0 {
        return Err(serde::de::Error::custom(format!(
            "max_tool_iterations must be a positive integer (>= 1), got {raw}"
        )));
    }
    u32::try_from(raw)
        .ok()
        .and_then(NonZeroU32::new)
        .filter(|v| v.get() <= MAX_TOOL_ITERATIONS)
        .map(Some)
        .ok_or_else(|| {
            serde::de::Error::custom(format!(
                "max_tool_iterations must be <= {MAX_TOOL_ITERATIONS}, got {raw}"
            ))
        })
}

impl Frontmatter {
    /// Returns the prompt's identifier, read from the `name:` key.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the prompt's one-line summary.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the Engine major version that the `runvil:` key declares,
    /// when present.
    #[must_use]
    pub fn runvil(&self) -> Option<u32> {
        self.runvil
    }

    /// Returns the declared cap on model round trips in each section's
    /// tool-call loop.
    ///
    /// Returns `None` when the prompt omits the cap, so the caller's default
    /// applies.
    #[must_use]
    pub fn max_tool_iterations(&self) -> Option<NonZeroU32> {
        self.max_tool_iterations
    }

    /// Returns the declared input file, when present.
    ///
    /// The prompt expects to find this file in the store when it starts.
    #[must_use]
    pub fn input(&self) -> Option<&FileDecl> {
        self.input.as_ref()
    }

    /// Returns the declared output file, when present.
    ///
    /// The prompt leaves this file in the store when it finishes.
    #[must_use]
    pub fn output(&self) -> Option<&FileDecl> {
        self.output.as_ref()
    }

    /// Returns the `uses:` entries, in declaration order.
    ///
    /// Every declared Pack is required, and no two entries share a Pack
    /// id.
    #[must_use]
    pub fn uses(&self) -> &[PackRef] {
        &self.uses
    }

    /// Returns the typed args declaration.
    ///
    /// A prompt that omits the `args:` key yields the default [`ArgsDecl`].
    #[must_use]
    pub fn args(&self) -> &ArgsDecl {
        &self.args
    }

    /// Returns the requirements the prompt declares for `role`, or `None`
    /// when it does not name the role.
    #[must_use]
    pub fn role_requirements(&self, role: ModelRole) -> Option<&RoleRequirements> {
        self.models.get(role)
    }
}

/// A prompt file split at its frontmatter by [`split_frontmatter`].
#[derive(Debug)]
pub(crate) struct SplitSource {
    /// The YAML text between the two `---` delimiters.
    pub(crate) yaml: String,
    /// The markdown body after the closing `---` line.
    pub(crate) body: String,
    /// The number of lines consumed by the frontmatter block (both `---`
    /// delimiters and everything between them).
    pub(crate) frontmatter_lines: u32,
}

/// Splits a file into its YAML frontmatter and its markdown body.
///
/// The file must open with a `---` line and close the frontmatter with another
/// `---` line starting at column 0; an indented `---` is YAML block scalar
/// text. `str::lines` handles both `\n` and `\r\n`. Whitespace before or after
/// the dashes of the opening line is ignored.
pub(crate) fn split_frontmatter(input: &str) -> Result<SplitSource> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input); // drop BOM
    let mut lines = input.lines();
    match lines.next() {
        Some(l) if l.trim() == "---" => {}
        _ => {
            return Err(error::failure(
                ErrorCode::FrontmatterInvalid,
                "file must begin with a --- frontmatter delimiter",
            ));
        }
    }
    let mut yaml = String::new();
    let mut closed = false;
    let mut line_count: u32 = 1; // opening ---
    for line in lines.by_ref() {
        line_count += 1;
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err(error::failure(
            ErrorCode::FrontmatterInvalid,
            "frontmatter was not closed with ---",
        ));
    }
    let body = lines.collect::<Vec<_>>().join("\n");
    Ok(SplitSource {
        yaml,
        body,
        frontmatter_lines: line_count,
    })
}
