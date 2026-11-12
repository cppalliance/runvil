//! Section paths: titles separated by `/`.
//!
//! [`split`] is the one grammar every path shares: the steps, the
//! escapes, the trimming, and what makes an index. What a path may start
//! with and which forms it may use depend on what takes it, so each taker
//! checks the [`Steps`] it gets back. [`Path`] is the check for a path
//! over parsed text.

use std::fmt;

use crate::index::at_lines;

/// One step of a path: a section's title, and how the step finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// Whether the step was written after `//`, which searches at any
    /// depth.
    pub deep: bool,
    /// The title the step names, with its escapes read and its
    /// surrounding whitespace removed. It is never empty.
    pub title: String,
    /// The `[n]` written after the title, which is never zero.
    pub index: Option<i64>,
}

/// A path split into its steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Steps {
    /// Whether the path starts with `/`.
    pub from_top: bool,
    /// The steps, in order. There is at least one.
    pub steps: Vec<Step>,
}

/// Splits a path into its steps.
///
/// Whitespace around the path and around each step is ignored. `\/` is a
/// slash inside a title, `\\` a backslash, and `\[` a bracket that would
/// otherwise open an index. A bracketed integer at the very end of a step
/// is its index, and an index too large to count with is kept as the
/// largest one, which no match reaches.
///
/// # Errors
/// Returns the message for an empty step, a backslash that starts none of
/// the three escapes, or an index of zero.
pub fn split(given: &str) -> Result<Steps, String> {
    let trimmed = given.trim();
    let (from_top, mut rest) = match trimmed.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    let mut steps = Vec::new();
    loop {
        let (deep, text) = match rest.strip_prefix('/') {
            Some(text) => (true, text),
            None => (false, rest),
        };
        let (step, after) = step(given, deep, text)?;
        steps.push(step);
        match after {
            Some(after) => rest = after,
            None => return Ok(Steps { from_top, steps }),
        }
    }
}

/// Reads one step from the front of `text`. Returns it with the text
/// after the slash that ends it, or `None` when the step ends the path.
fn step<'a>(given: &str, deep: bool, text: &'a str) -> Result<(Step, Option<&'a str>), String> {
    let mut title = String::new();
    // Where the last `[` sits in `title`, and whether it was escaped.
    let mut bracket = None;
    let mut after = None;
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '/' => {
                after = text.get(at + 1..);
                break;
            }
            '\\' => match chars.next() {
                Some((_, escaped @ ('/' | '\\' | '['))) => {
                    if escaped == '[' {
                        bracket = Some((title.len(), true));
                    }
                    title.push(escaped);
                }
                Some((_, other)) => {
                    return Err(format!(
                        "path \"{given}\" has an unknown escape \"\\{other}\"; the escapes are \
                         \"\\/\", \"\\\\\", and \"\\[\""
                    ));
                }
                None => return Err(format!("path \"{given}\" ends in a backslash")),
            },
            _ => {
                if c == '[' {
                    bracket = Some((title.len(), false));
                }
                title.push(c);
            }
        }
    }
    let index = index(given, &mut title, bracket)?;
    let title = title.trim();
    if title.is_empty() {
        return Err(format!("path \"{given}\" has an empty step"));
    }
    let step = Step {
        deep,
        title: title.to_owned(),
        index,
    };
    Ok((step, after))
}

/// Takes the index off the end of a step's text, when it has one: a
/// bracketed integer at the very end whose bracket, at `bracket`, was not
/// escaped.
fn index(
    given: &str,
    title: &mut String,
    bracket: Option<(usize, bool)>,
) -> Result<Option<i64>, String> {
    let Some((open, false)) = bracket else {
        return Ok(None);
    };
    let Some(digits) = title
        .trim_end()
        .get(open + 1..)
        .and_then(|inner| inner.strip_suffix(']'))
        .filter(|inner| is_integer(inner))
    else {
        return Ok(None);
    };
    let n = digits.parse::<i64>().unwrap_or(if digits.starts_with('-') {
        i64::MIN
    } else {
        i64::MAX
    });
    if n == 0 {
        return Err(format!(
            "path \"{given}\" has index 0; an index is a nonzero integer"
        ));
    }
    title.truncate(open);
    Ok(Some(n))
}

/// Whether `text` is digits, with or without a `-` in front.
fn is_integer(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Writes a title as a path step: `\` and `/` escaped, and the bracket of
/// a bracketed integer at its end escaped, so the step reads back as the
/// title.
#[must_use]
pub fn escape_title(title: &str) -> String {
    let mut step = title.replace('\\', "\\\\").replace('/', "\\/");
    if let Some(open) = step.rfind('[')
        && step
            .get(open + 1..)
            .and_then(|inner| inner.strip_suffix(']'))
            .is_some_and(is_integer)
    {
        step.insert(open, '\\');
    }
    step
}

impl fmt::Display for Step {
    /// Writes the step as a path shows it: `//` in front when it searches
    /// at any depth, and its index behind.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.deep {
            f.write_str("//")?;
        }
        f.write_str(&escape_title(&self.title))?;
        match self.index {
            Some(n) => write!(f, "[{n}]"),
            None => Ok(()),
        }
    }
}

/// A path over parsed text: from the top of a list of blocks, with `//`
/// and `[n]` allowed and no `..`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    /// The steps, in order. There is at least one.
    steps: Vec<Step>,
}

impl Path {
    /// Reads a path over parsed text.
    ///
    /// # Errors
    /// Returns the message for a path [`split`] refuses, one with no
    /// leading `/`, or one with a `..` step.
    pub fn parse(given: &str) -> Result<Path, String> {
        let Steps { from_top, steps } = split(given)?;
        if !from_top {
            return Err(format!("a path starts with \"/\", got \"{given}\""));
        }
        if steps.iter().any(|step| step.title == "..") {
            return Err(format!(
                "path \"{given}\" uses \"..\"; parsed text has no parent to step to"
            ));
        }
        Ok(Path { steps })
    }

    /// The steps, in order. There is at least one.
    #[must_use]
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// The message for a path that names no section because step `number`,
    /// counted from 1, kept nothing. `given` is the path as it was
    /// written.
    #[must_use]
    pub fn no_match(&self, given: &str, number: usize) -> String {
        let step = number
            .checked_sub(1)
            .and_then(|at| self.steps.get(at))
            .map(Step::to_string)
            .unwrap_or_default();
        format!(
            "path \"{}\" matches no section; step {number}, \"{step}\", matches nothing",
            given.trim()
        )
    }

    /// The message for a path whose last step kept several sections, one
    /// on each of `lines`. A match that records no line is `None`.
    #[must_use]
    pub fn ambiguous(given: &str, lines: &[Option<i64>]) -> String {
        let given = given.trim();
        format!(
            "path \"{given}\" is ambiguous; it matches {} sections{}; add an index such as \
             \"{given}[1]\"",
            lines.len(),
            at_lines(lines)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Path, Step, Steps, escape_title, split};

    fn step(title: &str) -> Step {
        Step {
            deep: false,
            title: title.to_owned(),
            index: None,
        }
    }

    fn deep(title: &str) -> Step {
        Step {
            deep: true,
            ..step(title)
        }
    }

    fn indexed(title: &str, n: i64) -> Step {
        Step {
            index: Some(n),
            ..step(title)
        }
    }

    /// Asserts that `given` has no leading slash and splits into `steps`.
    #[track_caller]
    fn assert_relative(given: &str, steps: &[Step]) {
        let expected = Steps {
            from_top: false,
            steps: steps.to_vec(),
        };
        assert_eq!(split(given), Ok(expected), "{given:?}");
    }

    /// Asserts that `given` starts with a slash and splits into `steps`.
    #[track_caller]
    fn assert_from_top(given: &str, steps: &[Step]) {
        let expected = Steps {
            from_top: true,
            steps: steps.to_vec(),
        };
        assert_eq!(split(given), Ok(expected), "{given:?}");
    }

    #[test]
    fn a_path_is_steps_separated_by_slashes() {
        assert_relative("Template", &[step("Template")]);
        assert_relative("../Review", &[step(".."), step("Review")]);
        assert_relative("a/b/c", &[step("a"), step("b"), step("c")]);
        assert_from_top("/Guide/Setup", &[step("Guide"), step("Setup")]);
        assert_relative("## Rules", &[step("## Rules")]);
    }

    #[test]
    fn a_doubled_slash_marks_the_step_after_it() {
        assert_from_top("//Extras", &[deep("Extras")]);
        assert_from_top("/Guide//Extras", &[step("Guide"), deep("Extras")]);
        assert_from_top("//A//B", &[deep("A"), deep("B")]);
        assert_relative("a//b", &[step("a"), deep("b")]);
    }

    #[test]
    fn whitespace_around_the_path_and_around_each_step_is_ignored() {
        assert_relative(" .. / Review ", &[step(".."), step("Review")]);
        assert_from_top("\t/ Guide /  Set up \n", &[step("Guide"), step("Set up")]);
        assert_from_top("/Log/ Entry [2] ", &[step("Log"), indexed("Entry", 2)]);
    }

    #[test]
    fn a_bracketed_integer_at_the_end_of_a_step_is_its_index() {
        assert_from_top("/Log/Entry[1]", &[step("Log"), indexed("Entry", 1)]);
        assert_from_top("/Log/Entry[-1]", &[step("Log"), indexed("Entry", -1)]);
        let second = Step {
            index: Some(2),
            ..deep("A")
        };
        assert_from_top("//A[2]/B[12]", &[second, indexed("B", 12)]);
        assert_relative("Topics[1]", &[indexed("Topics", 1)]);
        assert_from_top("/a[1][2]", &[indexed("a[1]", 2)]);
    }

    #[test]
    fn any_other_bracket_is_part_of_the_title() {
        for title in [
            "[Draft] Notes",
            "Notes [old]",
            "a[1]b",
            "a[]",
            "a[1.5]",
            "a[+1]",
            "a[ 1 ]",
            "a[--1]",
            "a]",
            "a[1",
        ] {
            assert_from_top(&format!("/{title}"), &[step(title)]);
        }
    }

    #[test]
    fn the_three_escapes_put_a_slash_a_backslash_and_a_bracket_in_a_title() {
        assert_relative(r"Input\/Output", &[step("Input/Output")]);
        assert_from_top(r"/a\\b/c", &[step(r"a\b"), step("c")]);
        assert_from_top(r"/Appendix \[2]", &[step("Appendix [2]")]);
        assert_from_top(r"/a\[1][2]", &[indexed("a[1]", 2)]);
        assert_from_top(r"/a[1]\[2]", &[step("a[1][2]")]);
        assert_relative(r"\/\/", &[step("//")]);
    }

    #[test]
    fn an_index_too_large_to_count_with_is_kept_as_the_largest() {
        assert_from_top("/a[99999999999999999999]", &[indexed("a", i64::MAX)]);
        assert_from_top("/a[-99999999999999999999]", &[indexed("a", i64::MIN)]);
    }

    #[test]
    fn an_empty_step_is_refused() {
        for path in [
            "", "  ", "/", "//", "Guide/", "../", "/Guide/", "///a", "/a/ /b", "[1]", "/a/[2]",
        ] {
            assert_eq!(
                split(path),
                Err(format!("path \"{path}\" has an empty step")),
                "{path:?}"
            );
        }
    }

    #[test]
    fn any_other_backslash_is_refused() {
        for (path, escape) in [(r"/a\b", r"\b"), (r"a\]", r"\]"), (r"\.\./a", r"\.")] {
            assert_eq!(
                split(path),
                Err(format!(
                    r#"path "{path}" has an unknown escape "{escape}"; the escapes are "\/", "\\", and "\[""#
                )),
                "{path}"
            );
        }
        assert_eq!(
            split(r"/a\"),
            Err(r#"path "/a\" ends in a backslash"#.to_owned())
        );
    }

    #[test]
    fn an_index_of_zero_is_refused() {
        for path in ["/Log/Entry[0]", "/Log/Entry[-0]", "/Log/Entry[000]"] {
            assert_eq!(
                split(path),
                Err(format!(
                    "path \"{path}\" has index 0; an index is a nonzero integer"
                )),
                "{path}"
            );
        }
    }

    #[test]
    fn a_step_is_written_as_a_path_shows_it() {
        assert_eq!(step("Nope").to_string(), "Nope");
        assert_eq!(deep("Nope").to_string(), "//Nope");
        assert_eq!(indexed("Entry", 3).to_string(), "Entry[3]");
        assert_eq!(indexed("Entry", -1).to_string(), "Entry[-1]");
        assert_eq!(step("Input/Output").to_string(), r"Input\/Output");
    }

    #[test]
    fn an_escaped_title_reads_back_as_the_title() {
        for title in [
            "Plain",
            "Input/Output",
            r"back\slash",
            "Appendix [2]",
            "Appendix [-2]",
            "[Draft] Notes",
            "Notes [old]",
            r"a\/b[3]",
            "a[1][2]",
            "## Rules",
        ] {
            assert_from_top(&format!("/{}", escape_title(title)), &[step(title)]);
        }
        assert_eq!(escape_title("Appendix [2]"), r"Appendix \[2]");
        assert_eq!(escape_title("Notes [old]"), "Notes [old]");
        assert_eq!(escape_title(r"C:\Users"), r"C:\\Users");
    }

    #[test]
    fn a_path_over_parsed_text_starts_at_the_top_and_has_no_parent_step() {
        let path = Path::parse("/Guide//Extras[1]").unwrap();
        let last = Step {
            index: Some(1),
            ..deep("Extras")
        };
        assert_eq!(path.steps(), [step("Guide"), last]);
        assert_eq!(
            Path::parse("Guide/Setup"),
            Err(r#"a path starts with "/", got "Guide/Setup""#.to_owned())
        );
        assert_eq!(
            Path::parse("/Guide/../Usage"),
            Err(
                r#"path "/Guide/../Usage" uses ".."; parsed text has no parent to step to"#
                    .to_owned()
            )
        );
        assert_eq!(
            Path::parse("/Guide/"),
            Err(r#"path "/Guide/" has an empty step"#.to_owned())
        );
    }

    #[test]
    fn a_path_that_names_nothing_says_which_step_kept_nothing() {
        for (given, number, message) in [
            (
                "/Guide/Nope",
                2,
                r#"path "/Guide/Nope" matches no section; step 2, "Nope", matches nothing"#,
            ),
            (
                "/Log/Entry[3]",
                2,
                r#"path "/Log/Entry[3]" matches no section; step 2, "Entry[3]", matches nothing"#,
            ),
            (
                " //Nope ",
                1,
                r#"path "//Nope" matches no section; step 1, "//Nope", matches nothing"#,
            ),
        ] {
            let path = Path::parse(given).unwrap();
            assert_eq!(path.no_match(given, number), message);
        }
    }

    #[test]
    fn a_path_that_names_several_sections_asks_for_an_index() {
        assert_eq!(
            Path::ambiguous("//Items", &[Some(5), Some(12)]),
            r#"path "//Items" is ambiguous; it matches 2 sections, at lines 5 and 12; add an index such as "//Items[1]""#
        );
        assert_eq!(
            Path::ambiguous("/Log/Entry", &[Some(3), None, Some(9)]),
            r#"path "/Log/Entry" is ambiguous; it matches 3 sections; add an index such as "/Log/Entry[1]""#
        );
    }
}
