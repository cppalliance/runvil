//! The glob matcher that ListDir's `ignore_globs` use, and that Grep's
//! `glob` filter uses after it.
//!
//! The grammar is the one `Access::glob` accepts, ported because this
//! crate cannot import the vfs matcher: only `*` and `**` are special, and
//! every other byte is literal and matched case-sensitively. One thing
//! differs. `Access::glob` anchors a pattern with no leading `/` at the
//! access root, and this matcher takes such a pattern at any depth, as if
//! `**/` came first, because it filters paths that are already known.
//!
//! The match is a bounded iterative sweep, with no recursion, so a hostile
//! pattern cannot drive exponential time or blow the stack.

/// The longest pattern, in bytes, the matcher takes.
pub(crate) const MAX_PATTERN_BYTES: usize = 1024;

/// Why a pattern was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatternError {
    /// The pattern is empty.
    Empty,
    /// The pattern is longer than [`MAX_PATTERN_BYTES`].
    TooLong,
    /// The pattern holds a byte below 0x20, or 0x7f.
    Control,
    /// The pattern holds a backslash, and there is no escape syntax.
    Backslash,
    /// The pattern holds three or more `*` in a row.
    StarRun,
    /// A `**` that is not a whole path segment.
    PartialDoubleStar,
}

impl PatternError {
    /// The text that says what rule the pattern broke.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            PatternError::Empty => "pattern is empty",
            PatternError::TooLong => "pattern is longer than 1024 bytes",
            PatternError::Control => "pattern contains a control character",
            PatternError::Backslash => "pattern does not support backslash escapes",
            PatternError::StarRun => "more than two consecutive '*' are not supported",
            PatternError::PartialDoubleStar => "'**' must occupy a whole path segment",
        }
    }
}

/// One unit of a valid pattern.
#[derive(Debug, Clone, Copy)]
enum Token {
    /// A literal byte that must match exactly.
    Literal(u8),
    /// `*`: zero or more bytes, none of them `/`.
    Star,
    /// `**` at the end of the pattern, or alone: zero or more bytes of any
    /// kind.
    DoubleStar,
    /// `**/`: zero or more whole path segments, which is nothing, or a run
    /// that ends in `/`.
    DoubleStarSlash,
}

/// A valid pattern, ready to match absolute virtual paths.
#[derive(Debug)]
pub(crate) struct Matcher {
    /// The pattern's units, with a leading [`Token::DoubleStarSlash`] when
    /// the pattern has no leading `/`.
    tokens: Vec<Token>,
}

impl Matcher {
    /// Compiles `pattern`.
    ///
    /// A pattern with a leading `/` is a full path from the root. Any
    /// other pattern matches at any depth, as if `**/` came first.
    ///
    /// # Errors
    /// Returns the rule the pattern breaks: empty, longer than
    /// [`MAX_PATTERN_BYTES`], holding a control byte or a backslash,
    /// holding more than two `*` in a row, or holding a `**` that is not a
    /// whole path segment. The rules are checked in that order.
    pub(crate) fn new(pattern: &str) -> Result<Matcher, PatternError> {
        validate(pattern)?;
        let mut tokens = Vec::with_capacity(pattern.len() + 1);
        if !pattern.starts_with('/') {
            tokens.push(Token::DoubleStarSlash);
        }
        tokenize(pattern.as_bytes(), &mut tokens);
        Ok(Matcher { tokens })
    }

    /// Whether the whole of `path`, an absolute virtual path with a leading
    /// `/`, matches.
    ///
    /// The sweep tracks which prefixes of `path` the tokens read so far can
    /// consume, so it takes `O(tokens * path length)` time.
    pub(crate) fn is_match(&self, path: &str) -> bool {
        let text = path.as_bytes();
        let len = text.len();
        // `reachable[j]` is true when the tokens read so far match exactly
        // `text[..j]`.
        let mut reachable = vec![false; len + 1];
        if let Some(start) = reachable.first_mut() {
            *start = true;
        }
        let mut next = vec![false; len + 1];
        for token in &self.tokens {
            next.fill(false);
            match *token {
                Token::Literal(byte) => {
                    for (j, here) in reachable.iter().take(len).enumerate() {
                        if *here
                            && text.get(j) == Some(&byte)
                            && let Some(after) = next.get_mut(j + 1)
                        {
                            *after = true;
                        }
                    }
                }
                Token::Star => {
                    // Zero or more bytes that are not `/`: carry
                    // reachability forward across each such byte.
                    let mut carry = false;
                    for (j, (slot, here)) in next.iter_mut().zip(&reachable).enumerate() {
                        let reached = *here || carry;
                        *slot = reached;
                        carry = reached && text.get(j).is_some_and(|byte| *byte != b'/');
                    }
                }
                Token::DoubleStar => {
                    // Zero or more bytes of any kind: once a position is
                    // reachable, every later one is too.
                    let mut seen = false;
                    for (slot, here) in next.iter_mut().zip(&reachable) {
                        seen |= *here;
                        *slot = seen;
                    }
                }
                Token::DoubleStarSlash => {
                    // Nothing, or a run that ends in `/`.
                    let mut seen = false;
                    for (j, (slot, here)) in next.iter_mut().zip(&reachable).enumerate() {
                        let after_slash = j
                            .checked_sub(1)
                            .and_then(|before| text.get(before))
                            .is_some_and(|byte| *byte == b'/');
                        *slot = *here || (seen && after_slash);
                        seen |= *here;
                    }
                }
            }
            std::mem::swap(&mut reachable, &mut next);
        }
        reachable.last().copied().unwrap_or(false)
    }
}

/// Checks `pattern` against the rules, in a fixed order: empty, length,
/// control bytes, backslash, then the `*` grammar.
fn validate(pattern: &str) -> Result<(), PatternError> {
    if pattern.is_empty() {
        return Err(PatternError::Empty);
    }
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(PatternError::TooLong);
    }
    if pattern.bytes().any(|byte| byte < 0x20 || byte == 0x7f) {
        return Err(PatternError::Control);
    }
    if pattern.contains('\\') {
        return Err(PatternError::Backslash);
    }
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes.get(index) != Some(&b'*') {
            index += 1;
            continue;
        }
        let run_start = index;
        while bytes.get(index) == Some(&b'*') {
            index += 1;
        }
        match index - run_start {
            1 => {}
            2 => {
                let before_ok = run_start
                    .checked_sub(1)
                    .is_none_or(|before| bytes.get(before) == Some(&b'/'));
                let after_ok = bytes.get(index).is_none_or(|byte| *byte == b'/');
                if !(before_ok && after_ok) {
                    return Err(PatternError::PartialDoubleStar);
                }
            }
            _ => return Err(PatternError::StarRun),
        }
    }
    Ok(())
}

/// Turns the valid pattern `pattern` into tokens, pushed onto `tokens`.
fn tokenize(pattern: &[u8], tokens: &mut Vec<Token>) {
    let mut index = 0;
    while let Some(byte) = pattern.get(index) {
        if *byte != b'*' {
            tokens.push(Token::Literal(*byte));
            index += 1;
        } else if pattern.get(index + 1) == Some(&b'*') {
            if pattern.get(index + 2) == Some(&b'/') {
                tokens.push(Token::DoubleStarSlash);
                index += 3;
            } else {
                tokens.push(Token::DoubleStar);
                index += 2;
            }
        } else {
            tokens.push(Token::Star);
            index += 1;
        }
    }
}

#[cfg(test)]
mod tests;
