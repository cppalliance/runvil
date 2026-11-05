# Definition edits

Each file in this directory except `dir.json` is Cursor's definition of the
tool named by its `wire_name`, snapshot on 2026-10-27. Everything in such a
file is Cursor's text except the edits below, each made because the line
describes behavior this Pack does not have. `dir.json` is this crate's own.

## read.json

- `Reads a file from the local filesystem.` became `Reads a file from the
  run's filesystem.`, because every read goes through the run's virtual
  filesystem, not the real one.
- `Assume this tool is able to read all files on the machine.` became
  `Assume this tool is able to read every file the run can see.`, for the
  same reason.
- `If the User provides a path to a file` became `If the prompt provides a
  path to a file`, because a Runvil run has a prompt rather than a chat
  user.
- Removed: `Image Support`, `PDF Support`, and the paragraph explaining how
  images and PDFs are returned. This build reads text only, so a model told
  otherwise would send a PDF and get an error.
- Removed `Results are returned using cat -n format, with line numbers
  starting at 1`. The description now states the format Read really uses: a
  six-wide right-aligned number, a pipe, and the line's text, where the
  number and the pipe are not part of the line.
- Removed `Any lines longer than 2000 characters will be truncated`. Read
  cuts a line only when that line alone passes the byte cap, and the
  description says so.
- Added lines that state what Read does: it opens any path the call names
  inside what the run has mounted, and reads through a link unless the link
  leaves the run's filesystem; a result holds at most `{byte_cap}` bytes of
  numbered lines, and a read that stops early names the offset to continue
  from; only text is read, so a file over `{file_ceiling}` bytes and a binary
  file are refused. The two placeholders are filled when the definition is
  built, from the constants the code enforces, so the text and the limits
  cannot drift apart.
- The `offset` and `limit` types are `integer`, where Cursor's were
  `number`, because a line count is whole.
- The `offset` text now says that 1 is the first line, that 0 reads as 1, and
  that a negative value counts back from the end with -1 as the last line.

## write.json

- Cursor's `Write` description, which is about creating or overwriting a
  file. Reworded to name `StrReplace` for targeted changes and to require a
  read before overwriting, matching Cursor's own guidance. No behavior was
  added; only the wording is ours.

## replace.json

- Cursor's `StrReplace` definition. The wording is ours; the parameters,
  their types, and the `required` list are Cursor's.

## rm.json

- Verbatim. Cursor's `Delete` description and schema are kept exactly.

## glob.json

- The `target_directory` description's default, `Cursor workspace root`,
  became `the run's filesystem root`.

## dir.json

- This file is new. It is not a Cursor snapshot, so none of its text is
  Cursor's. Cursor's counterpart, `LS`, is deprecated and gated behind a
  setting, so the wire name is `ListDir` rather than Cursor's. The two
  parameters, `target_directory` and `ignore_globs`, keep the names `LS`
  uses.
- The description states what this Pack's tool does: a bullet tree with
  directories ending in `/`, one byte-ordered list for files and
  directories, a breadth-first walk that stops at `{body_cap}` bytes with a
  note, `.git` skipped and every other dotfile listed, links listed and not
  entered, and `ignore_globs` matching at any depth. The placeholder is
  filled when the definition is built, from the constant the code
  enforces, so the text and the limit cannot drift apart.
- Dropped from `LS`: the extension-count summaries, the 5-second timeout,
  the terminal-log special case, and the `Error listing directory:`
  prefix.

## grep.json

- The parameter set is now Cursor's bundle schema plus `literal`: `pattern`,
  `path`, `glob`, `output_mode`, `-B`, `-A`, `-C`, `-i`, `type`, `head_limit`,
  `offset`, `multiline`, and `literal`, in that order. The hyphenated names
  are kept and no alias is added.
- Deleted `-n` and `context`. `-n` is gone because Grep always shows line
  numbers, and a call that still passes `-n: true` is accepted while any
  other `-n` value is refused. `-C` now takes the text `context` had, "Number
  of lines to show before and after each match. Requires output_mode:
  "content", ignored otherwise.", and `context` is refused as an undeclared
  key.
- Added `literal`, a boolean that defaults to false: the pattern is matched as
  plain text, so a pattern such as `foo(` needs no escape.
- `output_mode` keeps Cursor's default, `content`, and Cursor's enum. Its
  text does not mention `-n`.
- `head_limit` has no default and `"minimum": 0`. Cursor's text says it
  defaults to 250 and that 0 is unlimited, and this one says that 0 means no
  limit and so does leaving it out. Its text says it counts matching lines in
  `content` mode, files in `files_with_matches`, and file rows in `count`.
- `path` reads `File or directory to search in, as an absolute path. Defaults
  to the run's filesystem root.`, where Cursor's text says `Cursor workspace
  root`.
- `glob` reads `Glob pattern to filter files, for example "*.js" or
  "**/*.tsx". Only * and ** are special, and matching is case-sensitive. A
  pattern that does not start with "/" is matched at any depth. A pattern
  that starts with "/" is a full path.`, because `{ts,tsx}` and the other
  shell forms are not special in this Pack's matcher.
- The description is new and states what this Pack's Grep does: Rust regex
  syntax with no look-around and no back-references, and how to escape or set
  `literal`; path order, the order Glob uses; output that stops at
  `{body_cap}` bytes; at most 100000 files considered; a file over
  `{file_ceiling}` bytes not searched; binary files, symbolic links, over-size
  files, and unreadable entries skipped and counted in the result; only
  entries named `.git` skipped, with `.gitignore` rules not applied, so
  `target/` and `node_modules/` are searched when they lie under `path`; the
  `N:` and `N-` line prefixes and that `N` is the number Read shows; a line
  over 1000 bytes shown as a window around its first match; and that paths
  are absolute and work as the `path` argument of the other file tools. The
  two placeholders are filled when the definition is built, from the
  constants the code enforces, so the text and the limits cannot drift apart.
- The description names neither `rg` nor the `Shell` tool.