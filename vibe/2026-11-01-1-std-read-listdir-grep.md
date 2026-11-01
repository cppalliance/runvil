---
name: std Read, ListDir, and Grep
overview: Make std Read and Grep real and add a seventh tool, std/dir (wire name ListDir), so a model can read, search, and explore directories with no shell. Write, StrReplace, Delete, and Glob stay stubs. Read needs one new Access method, so this plan also changes the VFS.
todos:
  - id: vfs-read-range
    content: "Add Access::read_range in runvil-vfs (gate, claim_read, fire event, delegate to the existing VfsAccess::read_range) with VFS tests and a regenerated public-api.txt, in its own commit"
    status: completed
  - id: read-tool
    content: "Add the shared plugin-std code (regex workspace dependency, refusal text, argument helpers, plural, text module) and implement Read in src/read/ with the new read.json; dispatch Read and flip its integration tests"
    status: completed
  - id: dir-tool
    content: "Add the glob matcher and implement ListDir in src/dir/ with the new dir.json; embed it in spec.rs, move the tool-count tests from six to seven, dispatch ListDir, and flip its integration tests"
    status: completed
  - id: grep-tool
    content: "Change schemas/grep.json to the bundle schema plus literal, implement Grep in src/grep/ with its yield test, dispatch Grep, flip its integration tests, and finish with the full gate set"
    status: completed
isProject: false
---

# std Read, ListDir, and Grep

<product-contract>

## Product Requirements

The `std` Pack (crate `plugin-std`) ships six file tools that are stubs returning one fixed not-implemented text. This work makes Read and Grep real and adds a seventh tool, ListDir, so a model can explore directories, read files, and search with no shell. Every tool works only through the run's virtual file system, so the run's Mode and policy gate each access. Write, StrReplace, Delete, and Glob stay stubs. Until a shared ignore module exists, the walks skip only `.git`.

- Problem and users:
  - Today all six `std` tools return the same fixed text (`crates/plugin-std/src/pack.rs`, stub added in commit `08801150f`), so a model in a Runvil run cannot read, search, or explore files.
  - Users are models in Runvil runs, Lua authors who offer the tools by Lua id (`std/read`, `std/dir`, `std/grep`), and the maintainer.
- Goals:
  - Read returns line-numbered windows of text files at bounded cost.
  - ListDir (Lua id `std/dir`) returns a bounded directory tree.
  - Grep searches file contents with Rust regex syntax, stays cancellable, and pages its results.
  - Every failure names the path and says what to do next.
- Non-goals:
  - Real Write, StrReplace, Delete, and Glob.
  - A shell Pack, user approval, PDFs, and images.
  - The read record that Write will need, `.gitignore` pruning, and a VFS-level search.
  - Mounting a real directory in Workshop, which mounts empty in-memory storage today (`VfsRef::default()`).
- Success criteria:
  - Read, ListDir, and Grep return the texts in the Functional Specification against the in-memory backend and against a real directory, and the four remaining stubs still return the stub text.
  - Every exit criterion in the Testing Plan passes.
- Constraints:
  - Model-facing (wire) names copy Cursor's, except ListDir, whose Cursor counterpart `LS` is deprecated and gated behind a setting. Lua ids are `std/<name>`.
  - Text only, and no dotfile filtering.
  - The Engine and the Harness do not change, and the VFS changes only through `Access::read_range`.
  - A Plugin crate may name only `runvil-plugin`, `shared-*` crates, and outside libraries (`crates/build-xtask/src/product.rs`), so `plugin-std` never depends on the vfs crate.
  - No Rust file passes 500 lines.
  - No existing file under `vibe/` is edited, because those plans are historical. The run's own copy of this plan and `vibe/ACTIVE` are the only files it adds or changes there.
- Open questions: None

## Functional Specification

Read, ListDir, and Grep take JSON arguments and return text. Read pages through one file by line window, ListDir lists a tree breadth first under a byte cap, and Grep walks a tree in path order and pages its matches. Each refuses bad arguments by name, names the path in every failure, and returns file-derived text as untrusted. No cut ever splits a UTF-8 code point.

- Actors and workflows:
  - A model calls a tool by wire name (`Read`, `ListDir`, `Grep`), and a Lua script can offer or call it by Lua id.
  - The Pack validates the arguments, reads through the call's `Access`, and returns text.
  - A typical flow is ListDir to find a directory, Grep to locate text, Read to open a window of a file, then Read again with the offset from the trailer.
- Inputs and outputs:
  - Read (`std/read`):
    - Arguments are `path` (string, required), `offset` (integer), and `limit` (integer). `limit` below 1 is refused with ``std_Read: `limit` must be at least 1, and the call passed 0``.
    - The tool calls `Access::stat` first and refuses a directory, FIFO, socket, or device by name. It then reads at most 16,777,217 bytes. If more than 16,777,216 bytes come back the answer is `File is too large to read: {path} is over the limit of 16777216 bytes.` Read never truncates a file that is over the ceiling.
    - Binary check: if any of the first 8,192 bytes is NUL, or at least 5% of them are control bytes other than tab, LF, and CR, the answer is `File content is binary data, not text: {path}`. There is no extension list.
    - Decoding drops a UTF-8 mark, decodes UTF-16 by its mark, and otherwise decodes UTF-8 lossily. Lines split on `\n`, the empty element after a final newline is dropped, and one `\r` is stripped from each line, so `a\nb\n` and `a\nb` both have two lines and a zero-byte file has none. A file with no lines answers `File is empty.` as a success.
    - Window: the default offset is 1, offset `0` reads as 1, and a negative offset counts from the end (`-1` is the last line, and with no limit it reads the last `|offset|` lines). An overlapping window is clamped. A window that selects nothing fails with `Offset 11 is beyond the end of /notes.md, which has 10 lines.`
    - Numbering: every line gets a six-wide right-aligned number, a pipe, and no space (`     1|text`), always on, widening past six digits.
    - Cap: the byte cap is 100,000 bytes of numbered lines, counting each prefix and newline and not the trailer. Read stops before the line that would pass it and ends a cut read with `[Showing lines 1-2083 of 5000 (100000-byte limit). Use offset=2084 to continue.]`. The parenthesis is dropped when the caller's `limit` ended the read, and the last page prints no trailer.
    - First-line cut: a first line that alone passes the cap is clipped to the longest prefix that fits the 99,992-byte allowance (the cap less the 7-byte prefix and the newline) and ends on a character boundary. The notice reports the bytes kept, for example `[Line 1 is 150000 bytes and is cut after 99992 bytes. ...]` for ASCII and `cut after 99990 bytes` for 3-byte characters. That is the only cut inside a line. UTF-16 is decoded in full before the cut.
    - Notes: a UTF-16 file adds `[Note: the file is UTF-16LE and was decoded to text.]`, and replaced bytes add `[Note: n invalid UTF-8 sequences in this file were shown as U+FFFD.]`.
    - Conflict: if another call in the same round holds a write claim on the path, the answer is `{path} was changed, or is being changed, by another call in this round, so it cannot be read in this round. Call Read again in your next message.` Read never retries inside the call.
    - Description in `read.json`: remove the `cat -n` and 2,000-character-line text. State the six-wide format and that the prefix is not part of the line, the 100,000-byte cap and trailer, the 16777216-byte ceiling, that binary files are refused, that Read opens any path the call names, and that a link is read through unless it leaves the run's filesystem. The description is built from the same constants the code uses.
  - ListDir (`std/dir`, new `schemas/dir.json`):
    - Arguments are `target_directory` (string, required) and `ignore_globs` (array of strings). A blank `target_directory` is refused before any I/O.
    - Output is a tree: the root line is the absolute virtual path plus `/`, then a two-space indent per depth, `- ` bullets, and a trailing `/` on directories only. Files and directories share one list in byte order of the name with `/` appended to a directory's name. An empty directory prints `... no children found ...`, and a directory shown but not expanded ends in `...`.
    - The walk is breadth first, so it stops at the byte cap instead of walking a whole tree first. Links are listed by name and never entered. Entries named `.git` are skipped, and every other dotfile is listed.
    - The body cap is the shared 15,000 bytes. Over budget the listing ends with `    ...` and `target_directory is too large (either too many children or their names are too long). Immediate children were truncated. Try a more specific target_directory or ignore_globs.`
    - `ignore_globs` use the shared glob matcher, so a pattern with no leading `/` matches at any depth. The description says so.
    - Failures: `Path does not exist: {path}` and `Path is not a directory: {path}`.
    - Dropped from Cursor's `LS`: extension-count summaries, the 5-second timeout, the terminal-log special case, and the `Error listing directory:` prefix.
  - Grep (`std/grep`):
    - Parameters, in this order: `pattern`, `path`, `glob`, `output_mode`, `-B`, `-A`, `-C`, `-i`, `type`, `head_limit`, `offset`, `multiline`, then `literal` (boolean, default false). `-n` and `context` are deleted, and `-C` takes the text that was on `context`. `output_mode` defaults to `content`. `head_limit` has no default, a minimum of 0, and 0 means no limit. `-n: true` is accepted and `-n: false` is refused with ``std_Grep: `-n` must be true or left out, because line numbers are always shown.`` Any other key is refused by name with no aliases.
    - Dialect: the Rust `regex` crate, compiled before the walk, with `-i` as case-insensitive, multi-line anchors always on, and dot-matches-newline and CRLF handling only with `multiline`. Outside multiline mode matching is line by line, and in it the whole text is one subject. `literal` matches the escaped pattern text. A bad pattern fails with ``std_Grep: Invalid regular expression "<pattern>": <reason>. Grep matches with Rust regex syntax, which has no look-around and no back-references. To match the text literally, set literal to true.``
    - Search root: `path` is statted first. A directory is walked and a file is searched alone.
    - Order: each directory's entries are visited in byte order of the name with `/` appended to a directory's name, which is the same order as `Access::glob`. For example `/B`, `/a-b`, `/a.txt`, `/a/x`, `/b`. No clock is read.
    - Limits: stop at 100,000 files considered or 1 GiB read. A single file over 16 MiB is a counted skip, decided from the size in the directory listing before any read.
    - Skips: no single entry fails the search. Each skip is counted by reason (binary, unreadable directory, unreadable file, over 16 MiB, symbolic link, name with a control character, another call in this round, denied by policy), the first five are named, and a zero-match result says how many files were searched: `No matches found. Searched 412 files.`
    - Counting: `head_limit` and `offset` are global across files and counted in entries (a matching line, a file, or a count row), and context lines are never entries. `head_limit` rounds up and `offset` rounds down. The search collects one extra entry to decide whether a notice is due, so an exact fit prints none.
    - Paging: there is no cache, and each call re-runs from the start. A cut ends with `[Showing 1-50 of at least 51 matching lines. Use offset=50 to see more, or narrow the search.]`, the last page shows an exact total, and an offset past the end says `No more results at offset=N.` A stop caused by a limit offers no `offset`.
    - Output cap: 15,000 bytes of body in every mode, cut on a line boundary, with notice lines not counted. A matching line over 1,000 bytes becomes a window of at most 1,000 bytes that contains the first match and starts up to 200 bytes before it, with `[... N bytes omitted]` markers. A context line over 1,000 bytes is clipped the same way.
    - Render: there is no envelope. Each file's absolute virtual path appears once, then `  N:text` for a match and `  N-text` for context, a blank line between files and none at the end, and a gap line (two spaces, then two hyphens) between non-adjacent groups whenever a context flag is above 0. `-A` and `-B` each override their side after `-C` sets both, and overlapping context merges. `files_with_matches` prints one path per line, and `count` prints `path:count`.
    - Text: files are read with `Access::read` and decoded through the shared text rules, so a Grep line number names the line Read prints. Binary files are skipped in every output mode with the `binary` reason, so one file never appears in one mode and not in another. Replaced bytes add `[Note: invalid UTF-8 in 2 files was shown as U+FFFD: /a.txt, /b.txt]`.
    - Description in `grep.json`: names the dialect, order, limits, skips, and the ignore rule. It names `Rust regex`, `literal`, `15000`, `16777216`, and `skipped`, and does not name `ripgrep`, `rg `, `several thousand`, or `Shell`.
    - `type` uses a built-in table of about fifteen common names.

- States and validation:
  - Every argument error has the shape ``std_{Wire}: `{name}` {problem}``. `null` reads as absent, and numeric strings are not coerced. In Grep an empty `path`, `glob`, or `type` reads as absent.
  - Undeclared keys are refused by name with the tool's parameter list, because the shared argument check lets them through (`crates/plugin-std/src/check.rs`). An empty or whitespace-only `path` or `target_directory` is refused before any I/O.
  - Grep builds its argument errors in this order: empty pattern (``Pass "." to match every line``), negative `-A`, `-B`, or `-C`, negative `head_limit` or `offset`, unknown `type` (``Unrecognized file type "foo". Use a type such as js, ts, py, rust, go or java, or filter with glob.``), invalid `glob`, the pattern compile, then the `path` stat.
  - Glob grammar for Grep's `glob` and ListDir's `ignore_globs`: only `*` and `**` are special. `*` matches zero or more bytes except `/` inside one segment and does match dotfiles. `**` as a whole segment matches zero or more whole segments. Every other byte is literal, including `?`, `[`, `{`, `!`, and `.`, and matching is case-sensitive. A pattern with no leading `/` matches at any depth, which differs from `Access::glob`, where a relative pattern is anchored at the access root (`crates/runvil-internal/vfs/src/glob.rs`). A pattern is refused when it is empty, has more than two `*` in a row, uses `**` as less than a whole segment, contains a backslash or a control byte, or is longer than 1,024 bytes, which are the patterns `Access::glob` refuses.
- Errors and recovery:
  - One shared function turns every `VfsError` into one model-readable sentence, so one variant reads the same in every tool. Texts it owns: `File not found: {path}`, `Path is a directory, not a file: {path}`, `Path is neither a file nor a directory: {path}`, `Path does not exist: {path}` for a missing search root, the conflict sentence, and `I/O failure on {path} ({kind}): {message}`.
  - A message echoes the path as the model wrote it, never the canonical path, and names no other tool. `PermissionDenied` and `ApprovalRequired` reasons pass through unchanged, and a `Display` fallback keeps a variant added later readable.
  - Recovery is in the text: Read's trailer names the next offset, Grep's footer names the next offset or says to narrow the search, and the conflict sentence says to call Read again in the next message.
  - A cancel or stop drops a running call at its next await point. Grep yields so a long search can be dropped promptly.
- Security and privacy behavior:
  - Text derived from files is returned as untrusted output. The one exception is Grep's bare `No matches found` line, which may be trusted.
  - Every access goes through the call's `Access`, so the run's Mode and policy gate it. The Pack never touches the real filesystem directly.
  - Read opens any path the call names inside what the run has mounted. ListDir and Grep skip entries named `.git` and apply no other ignore rule, and no tool filters dotfiles.
- Acceptance criteria:
  - Each text in this section is returned byte for byte, and the four remaining stubs return the stub text unchanged.
  - Against a real directory, a model can list it with ListDir, find text with Grep, and read a window of the file with Read, then read the next window using the offset from the trailer.
  - No Read, ListDir, or Grep output contains a split UTF-8 code point at any cut, for any cap value.

</product-contract>
<implementation-contract>

## Technical Design

The Pack stays a thin layer over the call's `Access`: each tool validates its arguments, reads through the VFS, and returns text. Three shared modules (refusal text, text decoding, and the glob matcher) plus the argument helpers keep the three tools consistent. The VFS gains `Access::read_range`, which changes the public API file. Grep yields cooperatively so a running search can be cancelled. Plugin tests cannot choose their own `VfsRef`, so tests lean on `TestCall`'s in-memory storage.

- Architecture:
  - `Pack::call` returns a boxed future that the Harness polls inside its effect loop (`crates/runvil-plugin/src/pack.rs`, `crates/harness-internal/runner/src/host-run.rs`). A future that wakes its own task and returns `Pending` once hands control back to the Host's executor before the call completes. A cancel or stop drops the future, and nothing applies a time limit to a running call (`crates/harness-internal/runner/src/effect_loop.rs`).
  - Grep yields after every 32 files or 2 MiB read, through a small hand-written future that wakes its own task and returns `Pending` once. Read and ListDir do not yield, because the 16 MiB ceiling and the 15,000-byte cap bound their work.
  - Grep is a pure `find(access, request)` that returns what it found, and a separate `render` that turns that into text. It walks with `Access::list` and reads with `Access::read`. There is no VFS-level search.
  - `Access::list` promises no order. The stock backends sort by name, but the trait does not say so (`crates/runvil-internal/vfs/src/traits/backend.rs`), so the Pack sorts each listing itself by name with `/` appended to a directory's name. That order equals the order of `Access::glob`, which sorts full path strings bytewise.
- Modules and interfaces:
  - Each tool's source lives in a directory named after its Lua id: `src/read/`, `src/dir/`, and `src/grep/`. The entry file sits beside its directory (`src/read.rs` next to `src/read/`), and there is no `mod.rs`. A tool's directory never reaches into another tool's directory.
  - Write, StrReplace, Delete, and Glob keep their stub in `src/pack.rs` and get a directory when they get real code.
  - Shared code stays in `src/`: `refusal.rs` (one function from a tool name, the path as written, and a `VfsError` to a sentence), `text.rs`, and `matcher.rs`, plus the argument helpers in `check.rs`.
  - The text module has `decode`, `sniff_binary`, `split_lines`, and `clip_to_boundary`. `clip_to_boundary` is the only function that shortens text inside a line. It takes a `&str`, returns a `&str`, steps to the nearest character boundary with `str::is_char_boundary`, and is never bypassed by slicing on a raw byte index.
  - The argument helpers are `refuse_unknown` (builds the undeclared-key error), a blank-path helper, and `plural(n, noun)` for every count in a message.
  - Caps and limits (the 100,000-byte cap, the file, byte, and size limits, the 32-file slice) are parameters of the rendering and search functions with the constants as defaults, so tests can lower them.
  - A module's unit tests are `tests.rs` inside its own directory. The existing `pack-tests.rs`, `spec-tests.rs`, and `check-tests.rs` siblings stay as they are, and no new `#[path]` attribute is added for layout.
- File and public API changes:
  - `Access::read_range(path, offset, len)` is added in `crates/runvil-internal/vfs`. It takes the same four steps as `read`: gate by policy, admit a read claim, fire the read event, and delegate to the backend's existing `VfsAccess::read_range` (`crates/runvil-internal/vfs/src/traits/backend.rs`). `RealBackend` overrides that method with a bounded read. The trait's default reads the whole file and slices it, so a backend that does not override it is not bounded.
  - The `Access::read_range` addition is its own commit with a VFS test and a regenerated `crates/runvil/public-api.txt`.
  - If a new `Access` method is refused, the fallback is a `stat` ceiling for regular files kept in the Pack, which accepts that a link to a huge file looks small. Confidence: medium.
  - Schemas in `crates/plugin-std/schemas/`: `read.json` (integer `offset` and `limit`, new description), `grep.json` (new parameters and description), and a new `dir.json`. Each change is recorded in `schemas/EDITS.md`. `spec.rs` embeds seven files, and the tool-count tests move from six to seven.
  - `plugin-std` adds `regex` from the workspace (`regex = "1"` in the root `Cargo.toml`). `globset`, `sha2`, and `ignore` are not added.
- Data, persistence, failure, security, and privacy constraints:
  - Nothing persists. Grep has no cache and reads no clock, so a call's result depends only on its arguments and the VFS contents.
  - Read refuses a file over 16 MiB and never truncates it. Grep skips a file over 16 MiB from the size in the listing, and never fails on a single entry.
  - A claim conflict arrives as `VfsError::Conflict` (`crates/runvil-internal/vfs/src/handle/claims.rs`) and is shown as the conflict sentence, never as a not-found message.
  - File text is returned as untrusted output through `ToolOutput::untrusted`, and every access goes through `Access` so Mode and policy apply.

</implementation-contract>
<verification-contract>

## Testing Plan

Unit tests pin every text, boundary, and constant. Integration tests flip the stub expectations for Read, ListDir, and Grep to real output over `TestCall`'s in-memory store, and the yield check polls the Grep future by hand. No test in `plugin-std` needs the vfs crate. The exit criteria are the full gate set.

- Unit:
  - Read: the numbering window table; the cap and a page-by-page rebuild across 1, 2, 5, 37, 2,000, and 2,001 lines with and without a final newline; the first-line cut on lines of 1-, 2-, 3-, and 4-byte characters (99,992 or 99,990 kept bytes, always valid UTF-8); a cap sweep from 12 to 60 bytes over mixed-width text; a file that ends mid-sequence; the offset table on a 10-line file; line totals; carriage returns; the decode ladder; the binary sniff boundaries (NUL at index 8,191 versus 8,192, 5% versus 4.9%); every failure text byte for byte; the argument rules; a trust test; and the description pin, which asserts the text contains `100000` and `16777216` and not `cat -n`.
  - ListDir: the POSIX tree example in the parent design report (`runvil-design/specs/cursor/agent-file-tools.md`), an empty directory, a file as target, a missing path, `.git` skipped while `.env` and `.gitignore` are listed, sort order against `Access::glob`'s for the same tree, the byte cap and its note, and `ignore_globs` matching at any depth.
  - Grep: full-path order on `/B`, `/a-b`, `/a.txt`, `/a/x`, `/b`; `head_limit` global across files; page joins at 25 matches; `-C` overridden per side; merged context; the window around a match at position 1,001; skips with their counts; each regex failure; every argument string; UTF-8 safety of the long-line window and the context clip at every start offset over emoji and CJK text; and the description pin.
  - Matcher: each grammar rule, dotfiles, any-depth matching, and each refusal. The cases the matcher shares with `Access::glob` are pinned against that function's own cases, and a test records the one difference, which is any-depth matching.
  - Refusal text: every `VfsError` variant, including `Conflict`, built directly with no second call.
  - Worked examples to reuse as cases are in `specs/cursor/master-read.md` and `specs/cursor/master-grep.md` in the runvil-design repository. Where one disagrees with a rule in this plan, this plan governs.
  - VFS, in the vfs crate: `read_range` is denied by a read-denying policy, takes a read claim, fires the read event, and passes a `len` of 16,777,217 to a counting backend.
- Integration and end-to-end:
  - Flip Read, ListDir, and Grep in `crates/plugin-std/tests/it/tools.rs` from the stub text to real output. The four remaining stubs keep their stub tests.
  - Yield: seed 1,000 small files through the call context's `Access`, poll the Grep future by hand, assert it returns `Pending` at least 31 times before it completes, and assert that dropping it after the first `Pending` is safe. `TestCall` cannot supply a chosen `VfsRef` or an event sink (`crates/runvil-plugin/src/testing.rs`), so the test counts `Pending` polls and not reads.
  - Real directory: one test over a directory mounted with `RealBackend::rooted`. It cannot live in `plugin-std`, so it goes in an existing test suite whose crate may depend on both `plugin-std` and the vfs crate. The executing agent picks that suite.
- Regression, security, and performance:
  - The four stub tools still return the stub text.
  - File text is untrusted output, and Grep's bare `No matches found` line is the one trusted exception.
  - Cuts are checked over text of 1-, 2-, 3-, and 4-byte characters for Read's first line, Grep's long-line window, and Grep's context clip, so no cut splits a code point.
  - Limits (files considered, bytes read, per-file size, slice size) are lowered through their parameters so tests reach each limit without large fixtures.
- Exit criteria:
  - `cargo nextest run -p plugin-std` passes, then the full set as listed in `AGENTS.md`: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features` and `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`.
  - Both clippy runs pass with `CARGO_BUILD_WARNINGS=deny`: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`. `cargo fmt --all --check` passes.
  - The rustdoc run (`cargo doc --workspace --no-deps --all-features` with the workshop crates excluded and `RUSTDOCFLAGS="-D warnings"`) passes, and so does `cargo test -p build-xtask`.
  - `node --test crates/workshop/ui/test/docs-claims.mjs` passes.
  - `cargo +<pinned nightly> xtask api --check` passes with `CARGO_BUILD_WARNINGS` unset, where the nightly is the one named in `crates/build-xtask/src/api/toolchain.rs`, because the `Access` additions change the facade.

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - Name and ids. The Pack is `std` (crate `plugin-std`), Lua ids are `std/<name>`, and model-facing names copy Cursor's. Rationale: one namespace for the file tools with the names models already know. The maintainer said: "what about std instead of core?", "std yes; the Lua names would be maybe std/read, std/write, std/grep, std/replace, std/rm ? the model facing names should match Cursor".
  - Access only through the VFS, with no shell and no user approval yet. Rationale: Mode and policy then gate every access for free. The maintainer said: "VFS for sure; leave out shell, we can decide it later; no user approval, we will add that later".
  - Text only. The maintainer said: "text only of course".
  - Read and Grep behavior is the behavior in this plan, which follows the Read and Grep design reports in the runvil-design repository (`specs/cursor/master-read.md` and `specs/cursor/master-grep.md`) over Cursor's behavior. Rationale: the reports depart from Cursor on purpose (Read's cap is no longer an error, and Grep takes the bundle schema plus `literal`). The maintainer attached them as the context for this plan: "context: @runvil-design/specs/cursor/master-grep.md @runvil-design/specs/cursor/master-read.md". Where a report disagrees with this plan, this plan governs.
  - No cut splits a UTF-8 code point. Rationale: a split sequence turns into replacement characters or an invalid string. The maintainer said: "Read has to make sure not to split a UTF code point. Just update the design files".
  - Dotfiles are listed and searched, and only `.git` is skipped. Rationale: a model often needs to edit `.gitignore` and similar files. The maintainer said: "for the record, filtering dot files is stupid. one of the most useful things a model can do is stuff like edit .gitignore, and so on."
  - ListDir is added as `std/dir` with the shape in the Functional Specification. Rationale: Cursor explores directories through its shell and keeps a directory tool only behind a deprecated setting, so a Runvil model with no shell needs one. The maintainer asked "what about std/dir" and said "cursor has ListDir, and I am working on adding it to the report".
  - ListDir's wire name is `ListDir`, not Cursor's `LS`. Rationale: `LS` is deprecated and gated behind the setting `enableDeprecatedLsTool` (default false) in Cursor, so there is no live convention to match, and `ListDir` says what it does. This is the one exception to copying Cursor's wire names. The maintainer chose it when asked which name governs: "1. ListDir".
  - `Access::canonical_path` is not added in this work. Rationale: nothing here calls it, because the read record that would use it ships with the real Write tool, and it would add public API with no caller. The maintainer said: "2. defer canonical_path".
  - Grep is a pure `find` in the Pack over `Access::list` and `Access::read`, with no VFS search, no cache, no clock, no `<workspace_result>` envelope, and no newest-first sort. Rationale: deterministic results, and a search that can be dropped between slices.
  - Read uses a bounded `read_range` call after a `stat`. Rationale: the file's real size is never read when it is over the ceiling.
  - The glob matcher is the Pack's own, with the any-depth rule for relative patterns and the same refusals as `Access::glob`. Rationale: Grep's `glob` and ListDir's `ignore_globs` need any-depth matching, and `Access::glob` anchors at the root.
  - The Pack sorts each listing itself. Rationale: `Access::list` promises no order.
  - Ignore rules ship as option B: skip any path with a segment named `.git` and apply no `.gitignore`. Rationale: the shared ignore module is separate work, and nothing real is mounted yet, so nothing is exposed. The maintainer said: "3. option B".
  - Each tool's source lives in its own directory named after its Lua id, with the entry file beside the directory. Rationale: the repository retired `mod.rs` files in commit `128ff3a35`. The maintainer wrote a crate rule that each tool's source goes in its own directory, and then asked "is the three-file rule doing any actual work? ... maybe we should delete the three-file rule and just let everything accumuluate into the one dir?" and said "yes, make the replacement wording for AGENTS.md right now".
  - Existing historical plans under `vibe/` are not edited. The maintainer said: "no, do not touch the vibe/ plan. that is historical data."
  - Execution is four steps, one commit each: (1) `Access::read_range` with its VFS tests and regenerated `crates/runvil/public-api.txt`; (2) the shared `plugin-std` code and real Read; (3) the glob matcher and real ListDir; (4) real Grep with the full gate set. Rationale: few large steps keep the run short. The `read_range` commit stays separate because it regenerates the public API file, and the last step's full verification replaces a separate gates step. The maintainer said: "keep the number of steps low, dont bloat the plan with too many steps".
  - Tests inside `plugin-std` use `TestCall`'s in-memory store. Rationale: a Plugin crate may not depend on the vfs crate, and `TestCall` cannot take a chosen `VfsRef`.
- Rejected alternatives:
  - Cursor's Read cap as an error. Rejected because a cut read with a next-offset trailer lets the model continue. Revisit: never planned.
  - A binary-extension list for Read. Rejected in favor of the content sniff. Revisit if the sniff misclassifies real files in a deployed run.
  - The live Grep schema with `context` and `-n` and defaults `files_with_matches` and 250. Rejected for the bundle schema plus `literal`.
  - A regex-translated glob matcher with braces and case folding. Rejected for the four-token grammar. Revisit if models need brace expansion.
  - Grep through `Access::glob` plus whole-file `read_string`, or through a VFS search. Rejected for the pure `find`. Revisit when a deployed run shows searches over 2 seconds, which would justify a VFS search with a resumable step.
  - `mod.rs` entry files. Rejected because the repository retired them.
  - An entry file inside the tool directory with `#[path = "read/read.rs"] mod read;`. Rejected for the standard layout. Revisit if the maintainer prefers it.
  - Cursor `LS` extras (extension-count summaries, the 5-second timeout, the terminal-log special case, the `Error listing directory:` prefix). Rejected for the shared cap and refusal text.
  - Prepending `**/` to each `ignore_globs` pattern. Rejected because the matcher already matches relative patterns at any depth.
  - `LS` as ListDir's wire name. Rejected for `ListDir`. Revisit if Cursor stops deprecating `LS`.
  - Option A for ignore rules, which pulls the shared ignore module forward and first chooses between the `ignore` crate and a matcher of our own. Rejected for option B. Revisit when a real repository is mounted.
  - A yield test and a two-call conflict test built over a chosen `VfsRef` with an event sink inside `plugin-std`. Rejected because the dependency boundary forbids the vfs crate there. Revisit if `TestCall` gains a way to share a `VfsRef`.
- Assumptions, risks, and notes:
  - The numbers (100,000 and 15,000 bytes, 16 MiB, 100,000 files, 1 GiB, 32 files per slice, 2 MiB per slice, the 8,192-byte sniff, the 1,000-byte window) are judgments, not measurements. Each ships as one constant with a test.
  - ListDir uses the shared 15,000-byte cap, Cursor's tree text, Runvil's sort, and the shared refusal wording instead of `LS`'s 2500 and 10,000 character budgets. No design report covers it. Confidence: medium.
  - Grep's `type` table is about fifteen common names, not ripgrep's 224. Confidence: medium.
  - A Grep future that returns `Pending` once reaches the Host's executor, by reading `crates/harness-internal/runner/src/host-run.rs` and `effect_loop.rs`. The re-poll behavior of the collection that holds the future was assumed from standard behavior, not read, and the hand-polling test exercises only the future. If a yield proves not to work, Grep needs the VFS search with a resumable step, and this plan changes.
  - Without `.gitignore` pruning, a mounted real repository lets `target/` and `node_modules/` spend Grep's 100,000-file budget. The descriptions say `.gitignore` rules are not applied, and the zero-match line says no ignore rules ran.
  - The Grep design report says "files considered" in its limit rule and "after reading" in its notice. This plan uses "considered" for both.
  - A cross-scope claim conflict can be built in the vfs crate with two `acquire` calls on one `VfsRef` (`crates/runvil-internal/vfs/src/handle/tests/claims.rs`). A same-round sibling conflict needs an Engine-only function, so neither is reachable from `plugin-std` tests.
  - The `Access::read_range` addition touches the public API file, which needs the pinned nightly to regenerate.
  - The root `AGENTS.md` carries an edit that replaces the three-file layout rule with a looser one. The maintainer commits it before the run starts, because the run requires a clean worktree, and it belongs to no step.
  - Existing `#[path = "..."]` attributes stay. A count today is about 252 in `src/` trees (185 point at test files), and cleaning them is not worth a dedicated change. Confidence: high - no behavior gain for about 200 touched files.
  - Cursor's directory tool is named `LS` and is gated by `enableDeprecatedLsTool` (default false). This is from Cursor's own tool context and is not a repository fact.

### Deferred and Out of Scope

- Deferred: the read record (hash, shown range, total, whole flag), its per-run service, and its `needs` entry ship with the real Write tool.
- Deferred: the shared ignore module (the `ignore` crate versus a matcher of our own, which also decides whether `ignore` and `globset` join the lock file) and real Glob ship together. Revisit when a real repository is mounted, because until then `.gitignore` rules are not applied.
- Deferred: `Access::canonical_path`, which canonicalizes a path against the access root with no policy check, claim, or event. Revisit when the read record ships with the real Write tool.
- Deferred: a two-call conflict test (a Read against another call's write claim in one round). Revisit when Write is real or when `TestCall` can share a `VfsRef`.
- Deferred: a VFS search for Grep. Revisit when a deployed run shows searches over 2 seconds.
- Deferred: real Write, StrReplace, Delete, and Glob, and user approval.
- Deferred: the separate VFS todos `vfs-glob-gaps`, `vfs-mode-text`, `vfs-write-mode`, and `vfs-plan-subtree`, and the Engine todo `engine-cut-guard`, all listed in `vibe/2026-10/2026-10-27-2-plugin-std-file-tools.md`.
- Deferred: mounting a real directory in Workshop, which is a product decision about which directory to mount.
- Out of scope: a shell Pack, PDFs, images, and any change to the Engine or the Harness.
- Out of scope: any edit to an existing file under `vibe/`.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p <crate>`; a plain `cargo build` builds only the default members (`runvil`, `runvil-plugin`, `harness`, `harness-gateway-client`). The desktop app is opt-in with `cargo build -p workshop`. Cargo aliases in `.cargo/config.toml`: `cargo workshop`, `cargo xtask`, `cargo vibelight`. The pinned toolchain is Rust 1.99 (`rust-toolchain.toml`), edition 2024, MSRV 1.95.
- Focused test command pattern: `cargo nextest run --locked -p <crate> <test-name-substring>` (cargo-nextest 0.9.128 is installed; nextest profiles live in `.config/nextest.toml`, with a `ci` profile).
- Component test command pattern: `cargo nextest run --locked -p <crate>` for a whole crate, or `cargo nextest run --locked -p <crate> --test it` for a crate's integration target (`it` for plugin crates, `suite` for `harness` and `runvil`). For `plugin-std` that is `cargo nextest run --locked -p plugin-std`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`; the Workshop crates run separately with `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`. Structural checks: `cargo test -p build-xtask`; wording scan: `node --test crates/workshop/ui/test/docs-claims.mjs`; facade surface: `cargo +<pinned nightly> xtask api --check` (nightly named in `crates/build-xtask/src/api/toolchain.rs`; run it with `CARGO_BUILD_WARNINGS` unset).
- Linter command: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, run with `CARGO_BUILD_WARNINGS=deny`; Workshop crates: `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`. Clippy covers `cargo check`, so never run a standalone `cargo check --workspace`. The workspace lints (`[workspace.lints]` in the root `Cargo.toml`) deny `unwrap_used`, `expect_used`, `todo`, `allow_attributes`, and `unsafe_code`, and warn on `missing_docs`; tests may unwrap and expect (`clippy.toml`).
- Formatter check command: `cargo fmt --all --check` (`rustfmt.toml` sets `style_edition = "2024"`; the pre-commit hook runs this).
- Docs command: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`; facade docs: `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps` (CI also runs `cargo doc -p harness --no-deps`). Rustdoc lints are not covered by clippy.
- Test placement and naming conventions:
  - Unit tests sit in a sibling file named `<module>-tests.rs`, wired with `#[cfg(test)] #[path = "<module>-tests.rs"] mod tests;` (`plugin-std` has `check-tests.rs`, `pack-tests.rs`, `spec-tests.rs`). New code adds no `#[path]` attribute: a module with more than one file lives in a directory named after it (`foo.rs` beside `foo/bar.rs`) and its unit tests are `foo/tests.rs`. Larger groups use a `tests/` directory under `src` (for example `runvil-vfs/src/handle/tests/*.rs`), and shared helpers sit in `test_support.rs`.
  - Integration tests are a single Cargo target in a directory: `tests/it/main.rs` plus topic modules (`plugin-std/tests/it/{main,tools}.rs`), or `tests/suite/main.rs` for `harness` and `runvil`.
  - A test file that panics on setup uses `#![expect(clippy::expect_used, reason = "...")]`; every `allow` or `expect` attribute needs a `reason`.
  - Every `src`, `tests`, `benches`, and `examples` Rust file must stay at or under 500 lines (each crate's `build.rs` runs the `build-ceiling` check, which fails the build otherwise).
  - Engine crates (`runvil` and `runvil-*`) never sleep, read the wall clock, wait with a timeout, or draw OS entropy in tests, use only seeded pseudo-random input, and name temp directories from the test name, process id, and a counter. `plugin-std` is not an Engine crate but reaches files only through the call's `Access`.
- Directory map:
  - `crates/`: the workspace. Top level is the public and shared layer (`runvil`, `runvil-plugin`, `harness`, `harness-gateway-client`, `plugin-mcp`, `plugin-std`, `plugin-user-input`, `plugin-web`, `shared-error-source`, and `build-*` tooling crates). Private families live in manifestless containers: `runvil-internal/` (Engine: `engine`, `lua`, `model-client`, `parser`, `types`, `vfs`), `harness-internal/` (`runner`, `plugin-loader`, `test-plugin`), and `workshop/` (Workshop crates plus `ui`).
  - `.cargo/`, `.config/`, `.githooks/`: Cargo aliases and Windows linker flags, nextest profiles, and the pre-commit (fmt) and pre-push (docs-claims, clippy, `cargo deny`) hooks.
  - `.github/`: workflows (`ci.yml`, `nightly.yml`, `release-workshop.yml`, `site.yml`, `workshop-installer-smoke.yml`) and fixtures.
  - `.cursor/rules/`: Workshop architecture and SPA rules. `.vibelight/`: generated per-crate outline files (md and html), configured by `vibelight.toml`.
  - `guide/`: mdBook sources (`books/language`, `books/workshop`) and landing page. `prompts/`: example prompt files. `images/`: banner images.
  - `tools/`: Node `.mjs` scripts (with `.test.mjs` files) that pin, fetch, and stage the Gateway sidecar. `gateway-pin/`: `pin.json` for the Gateway release Workshop uses.
  - `vibe/`: dated plan and work notes, grouped by month. `local/`: untracked scratch and logs (gitignored). `target/`: build output (gitignored).
  - Root files: `AGENTS.md` (definitions, principles, verification gates), `Cargo.toml`, `Cargo.lock`, `clippy.toml`, `deny.toml`, `rustfmt.toml`, `rust-toolchain.toml`, `README.md`, `LICENSE` (BSL-1.0).
- Component boundaries:
  - Dependency directions (from each crate's `[dependencies]`): `runvil-types` and `runvil-vfs` depend on nothing in the workspace; `runvil-model-client` depends on `runvil-types`; `runvil-lua` on `runvil-model-client`, `runvil-types`, `runvil-vfs`; `runvil-parser` on `runvil-lua`, `runvil-types`; `runvil-engine` on all of those; the `runvil` facade on the Engine crates; `runvil-plugin` on `runvil-types` and `runvil-vfs`.
  - Plugin crates (`plugin-std`, `plugin-mcp`, `plugin-user-input`, `plugin-web`) depend only on `runvil-plugin` (plus `shared-*` and outside libraries); they may name no `build-*` crate, which is why `plugin-std/build.rs` includes the `build-ceiling` source by path.
  - `harness-runner` and `harness-plugin-loader` sit under `harness`; `harness` depends on `runvil` and `runvil-plugin`. Workshop crates depend on the Harness, the plugin crates, and `runvil`; code outside the Engine reaches `runvil-internal` only through `runvil` and `runvil-plugin`. `cargo test -p build-xtask` enforces these boundaries, the Workshop tier graph, the `## Invariants` marker, and lint inheritance.
- Conventions summary:
  - Every crate inherits `version`, `edition`, `license`, and `repository` from `[workspace.package]`, takes dependencies with `.workspace = true`, sets `[lints] workspace = true`, and carries a `build.rs` (file-size check). Comments in `Cargo.toml` explain each pin.
  - Crate-level docs live in `lib.rs` (`//!`) and end with a `## Invariants` list; docs name `Engine`, `Harness`, `Host`, `Pack`, `Plugin`, `Static Set`, and `Dynamic Set` with one meaning each, and Engine crate docs say "the caller", never the Host. The wording rules in `AGENTS.md` are enforced by `docs-claims.mjs`.
  - A tool's source goes in its own directory named after its Lua name (`plugin-std/AGENTS.md`); tool definitions are JSON under `schemas/`, embedded at build time, with edits recorded in `schemas/EDITS.md`.
  - Behavior changes ship with tests in the same change; error messages are written for a model to read (concise, factual, required versus actual); no unsafe code outside an owned boundary; process-global installers are banned outside binary entry points (`clippy.toml`).
  - JSON that reaches a recorder round-trips exactly (sorted keys, finite numbers, `float_roundtrip`). Cargo features gate only real constraints. A change to the facade's public surface regenerates `crates/runvil/public-api.txt`.
  - Code must build and test on Windows and Unix: CI runs Windows and Linux jobs, and `AGENTS.md` gives the PowerShell form for unsetting `CARGO_BUILD_WARNINGS`.

</project-survey>
<execution-plan>

## Execution Instructions

<step-1>

### Step 1: Add Access::read_range to the VFS [completed]

- Component: `vfs-read-range`
- Tests:
  - `cargo nextest run --locked -p runvil-vfs`
  - `cargo fmt --all --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`
  - `cargo +nightly-2026-09-05 xtask api --check`

Run the api check with `CARGO_BUILD_WARNINGS` unset (PowerShell: `Remove-Item Env:CARGO_BUILD_WARNINGS`). The nightly name must match `PINNED` in `crates/build-xtask/src/api/toolchain.rs`; if that file names another nightly, use it.

Placement: this is the first component because Read in step 2 calls `Access::read_range` and nothing else in this plan changes the VFS. It is one piece built in one step: the method, its four tests, and the regenerated API listing are one behavior. It is its own commit because it changes `crates/runvil/public-api.txt`, which needs the pinned nightly.

Build:

- `crates/runvil-internal/vfs/src/handle/operations.rs`: add `pub fn read_range(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>, VfsError>` on `Access`, right after `read_string`. It takes the same four steps as `read`, in this order: `let path = self.gate(Op::Read, path)?;`, `self.admit(Claims::claim_read, &path)?;`, `self.fire(Op::Read, &path);`, then `self.inner().read_range(&path, offset, len)`. The doc comment says it returns up to `len` bytes from byte `offset`, fewer at the end of the file, and nothing at or past the end. It says `RealBackend` seeks and reads only the requested bytes, and that a backend without an override reads the whole file and slices it, so it is not bounded there. Its `# Errors` section matches `read`. The doc text must not name an internal crate, because `cargo xtask api --check` refuses that.
- Nothing else in the VFS changes. `RealBackend::read_range` (`real.rs`), the forwarding impls (`handle/forward.rs`), and `router-access.rs` already implement or forward `VfsAccess::read_range`. Keep `operations.rs` at or under 500 lines.
- `crates/runvil/public-api.txt`: regenerate with `cargo +nightly-2026-09-05 xtask api --bless` (`CARGO_BUILD_WARNINGS` unset). The diff must be exactly one new line, between the `Access::read` and `Access::read_string` lines: `pub fn runvil::vfs::Access::read_range(&self, path: &str, offset: u64, len: u64) -> core::result::Result<alloc::vec::Vec<u8>, runvil::vfs::VfsError>`.

VFS tests, in a new file `crates/runvil-internal/vfs/src/handle/tests/read_range.rs` declared as a module in `handle/tests.rs` (no `#[path]` attribute). Model each on the named existing test:

- Denied by policy: with a policy that denies `Op::Read` and gives a reason that names the path, `read_range("/f.txt", 0, 10)` returns `VfsError::PermissionDenied` with that reason, and the backend call log (`TestFs` in `test_support.rs`) holds no read. Model: `a_policy_denied_operation_never_fires_the_sink` in `handle/tests/sink.rs`.
- Takes a read claim: after one identity calls `read_range("/f.txt", 0, 4)`, a write to `/f.txt` by another identity in another scope fails with `VfsError::Conflict`, and two identities that both call `read_range` on one path never conflict. Model: `a_write_conflicts_with_another_identitys_read_claim` and `reads_by_two_identities_never_conflict` in `handle/tests/claims.rs`.
- Fires the read event: a handle with an `on_op` sink receives one `OpEvent` with `Op::Read` and the canonical path for a `read_range`, and a denied `read_range` fires none. Model: `a_sink_receives_events_in_order_with_op_path_and_label`.
- Passes the length through: a small test backend, defined in the test file, whose `VfsAccess::read_range` override records its `offset` and `len`. `read_range("/f.txt", 3, 16_777_217)` makes it record `(3, 16_777_217)`, so the call is neither clamped nor turned into a whole-file read. Do not change `TestFs`, whose session deliberately implements only the required methods.
- Slicing on `MemoryBackend`, through the handle: on `hello world`, `(6, 5)` returns `world`, `(6, 100)` returns `world`, and `(100, 5)` returns nothing.

Do not touch the Engine, the Harness, `plugin-std`, or any file under `vibe/`.

Commit: one commit holding the method, its tests, and `crates/runvil/public-api.txt`. Suggested subject: `Add Access::read_range to the VFS`.

</step-1>

<step-2>

### Step 2: Shared plugin-std code and real Read [completed]

- Component: `read-tool`
- Tests:
  - `cargo nextest run --locked -p plugin-std`
  - `cargo clippy --locked -p plugin-std --all-targets -- -D warnings`
  - `cargo fmt --all --check`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

Placement: second, because Read calls `Access::read_range` from step 1, and because ListDir and Grep reuse the shared code this step adds (refusal text, argument helpers, `plural`, the text module). The component has two pieces, the shared foundation and the Read tool, built jointly in one step: the foundation has no caller but Read until step 3, and Read's failure texts and decoding are only testable through the foundation.

Rules for every step in this plan: warnings are denied and clippy runs `pedantic`, so add no item before something uses it (an unused `pub(crate)` item fails the build) and give every item a doc comment. Tests may use `unwrap` and `expect`. New code adds no `#[path]` attribute: a module is `foo.rs` beside a `foo/` directory, with its unit tests in `foo/tests.rs` (split into a `tests/` directory by topic when a file nears 500 lines). Every Rust file stays at or under 500 lines. Do not touch the Engine, the Harness, the vfs crate, or any file under `vibe/`. Do not make `plugin-std` depend on the vfs crate: import `Access`, `VfsError`, `Entry`, `FileType`, and `Stat` from `runvil_plugin`.

Dependency and lock file:

- `crates/plugin-std/Cargo.toml`: add `regex.workspace = true` with a one-line comment (Grep compiles its pattern with Rust regex syntax). The root `Cargo.toml` already holds `regex = "1"`, so it does not change. Build once without `--locked` so `Cargo.lock` records the new edge, and commit `Cargo.lock`. Nothing uses `regex` until step 4, and no unused-dependency check exists.

Shared code, all in `crates/plugin-std/src/`:

- `refusal.rs` (unit tests in `refusal/tests.rs`). `pub(crate) fn refusal(tool: &str, path: &str, error: &VfsError) -> String`, where `tool` is the wire name and `path` is the path as the model wrote it, never the canonical path. It returns one model-readable sentence per `VfsError`:
  - `NotFound` gives `File not found: {path}`.
  - `IsADirectory` gives `Path is a directory, not a file: {path}`.
  - `NotADirectory` gives `Path is not a directory: {path}`.
  - `Conflict` gives `{path} was changed, or is being changed, by another call in this round, so it cannot be read in this round. Call {tool} again in your next message.` Nothing retries inside a call.
  - `Io { kind, message, .. }` gives `I/O failure on {path} ({kind}): {message}`, with `kind` printed through `Display`.
  - `PermissionDenied { reason, .. }` and `ApprovalRequired { reason, .. }` give the reason unchanged.
  - Every other variant, and any variant added later (the enum is `#[non_exhaustive]`, so keep a wildcard arm), gives the variant's `Display` text with its leading label dropped: the text after the first `": "`, or the whole text when it has none. For example `OutsideRoot` reads as its `detail`.
  - A message names no other tool.
  - Also add `pub(crate) fn neither_file_nor_directory(path: &str) -> String` giving `Path is neither a file nor a directory: {path}`, and `pub(crate) fn missing_root(path: &str) -> String` giving `Path does not exist: {path}` (step 3 and step 4 use it for a missing search root; add `missing_root` only when step 3 first calls it, to avoid dead code).
  - Tests: build every `VfsError` variant directly as a struct literal, with no second call, and assert the sentence byte for byte, including `Conflict`. Assert that a relative path `notes/a.md` is echoed as written and not as `/notes/a.md`, that a denied read passes a reason that names the path through unchanged, and that no message names `Write`, `StrReplace`, `Delete`, `Glob`, `Grep`, or `Shell`.
- `check.rs` (keep the existing `check` and its `check-tests.rs` sibling; add tests there). Add three helpers:
  - `refuse_unknown(tool, schema, args)` refuses a key the schema's `properties` does not declare, in the shape ``std_{Wire}: `{key}` is not a parameter of this tool. The parameters are `a`, `b` and `c`.`` The names are sorted bytewise (serde_json keeps no insertion order here, so sort explicitly rather than rely on feature unification), joined with `, ` and with ` and ` before the last, and a single name has no `and`. The error kind is `InvalidArguments`. It runs after `check` and before any tool logic. The four stub tools do not call it.
  - A blank-path helper: ``std_{Wire}: `{name}` is empty or only whitespace, and the call passed "{value}"`` (kind `InvalidArguments`), returned when the value is empty or whitespace-only. It runs before any I/O.
  - `plural(n: usize, noun: &str) -> String` returns `{n} {noun}` for any `n` but 1, with `s` added, and `1 {noun}` for 1. A noun ending in a consonant plus `y` takes `ies` (`directory` gives `directories`, and `entry` gives `entries`). Examples: `1 line`, `10 lines`, `0 lines`.
- `text.rs` (unit tests in `text/tests.rs`). Public to the crate: `decode`, `sniff_binary`, `split_lines`, `clip_to_boundary`, and the constant `MAX_FILE_BYTES: u64 = 16_777_216`. Grep calls the same code in step 4, so a Grep line number names the line Read prints.
  - `decode(bytes: &[u8])` drops a leading UTF-8 mark (`EF BB BF`). A file that starts with `FF FE` or `FE FF` is decoded with `String::from_utf16_lossy` after dropping the mark (little-endian for `FF FE`, big-endian for `FE FF`), and the result records `UTF-16LE` or `UTF-16BE`. Anything else is decoded as UTF-8 lossily, and the result counts the invalid sequences (one per invalid chunk from `<[u8]>::utf8_chunks`, so a file that ends mid-sequence counts one). It returns the text, the UTF-16 name if any, and the invalid count. Never use `read_string` and never return `VfsError::NotUtf8` to the model. Do not guess any other encoding.
  - `sniff_binary(bytes: &[u8]) -> bool` looks only at the first 8,192 bytes. It is true when any of them is NUL, or when at least 5% of them are control bytes (`u8::is_ascii_control`, which is below 0x20 and 0x7f) other than tab, LF, and CR; compare with integers, `controls * 20 >= sampled_len`. A file that starts with a UTF-16 mark skips the sniff, because UTF-16 text holds NUL bytes by design; UTF-16 without a mark is binary. Boundary tests: a NUL at index 8,191 is binary and at index 8,192 is not; in a 1,000-byte sample 50 control bytes (5%) is binary and 49 (4.9%) is not; tab, LF, and CR never count.
  - `split_lines(text: &str) -> Vec<&str>` splits on `\n`, drops the empty element after a final newline, and strips one trailing `\r` from each line. So `a\nb\n` and `a\nb` both give two lines, an empty text gives none, and `a\r\nb\r\n` gives `a` and `b`; a lone `\r` inside a line is kept, so `first\rNEEDLE\rthird` is one line.
  - `clip_to_boundary(text: &str, start: usize, end: usize) -> &str` is the only function that shortens text inside a line. It rounds `start` up and `end` down to the nearest `str::is_char_boundary` position, clamps both to the length, and returns the sub-slice. It never slices by a raw byte index without that check. A prefix cut is `clip_to_boundary(text, 0, max)`.

Read, in `crates/plugin-std/src/read.rs` (entry) and `src/read/` (`args.rs`, `window.rs`, `render.rs`, `tests.rs` and a `tests/` directory as needed):

- Entry: `pub(crate) fn call(access: &Access, definition: &Definition, args: &Value) -> Result<ToolOutput, ToolError>`. Read does no yielding. Constant `pub(crate) const BYTE_CAP: usize = 100_000` in `read.rs`. The caps are parameters of `window` and `render` with the constants as defaults, so tests can lower them.
- Arguments: `path` (string, required), `offset` (integer), `limit` (integer). A `null` reads as absent and numeric strings are not coerced (the shared `check` already does both). Order: `check::check`, then `refuse_unknown`, then the blank-path helper, then `limit`, then I/O. A JSON integer that does not fit in `i64` is refused as ``std_Read: `offset` must be a JSON integer that fits in 64 bits, and the call passed {value}`` (same shape for `limit`). A `limit` below 1 is refused with ``std_Read: `limit` must be at least 1, and the call passed 0`` (the passed value is printed as given).
- Read order: (1) `access.stat(path)`; a stat failure becomes `refusal(...)`. A `Directory` gives `Path is a directory, not a file: {path}`. A `Fifo`, `Socket`, `CharDevice`, or `BlockDevice` gives `neither_file_nor_directory`. A `File` or `Symlink` goes on (the VFS reads through a link, and refuses it with `OutsideRoot` when it leaves the run's filesystem). Put the file-type decision in a pure function over `FileType` so a test reaches every variant. (2) `access.read_range(path, 0, 16_777_217)`; a failure becomes `refusal(...)`. If more than 16,777,216 bytes come back, fail with `File is too large to read: {path} is over the limit of 16777216 bytes.` Read never truncates a file over the ceiling. (3) `sniff_binary`; if true, fail with `File content is binary data, not text: {path}`. There is no extension list. (4) `decode`, then `split_lines`.
- Failures are `ToolError` values: argument errors with kind `InvalidArguments`, every other failure above with `ToolError::message(...)`. An empty result is a success: when there are no lines (a zero-byte file, or a file of only a byte order mark) the answer is `File is empty.` for any `offset` or `limit`. Every successful Read, including `File is empty.`, returns `ToolOutput::untrusted`.
- Window, in `window.rs`, over `N` lines with `N` at least 1: the default offset is 1, and offset `0` reads as 1. A positive offset above `N` fails with `Offset {offset} is beyond the end of {path}, which has {plural(N, "line")}.` (for example `Offset 11 is beyond the end of /notes.md, which has 10 lines.` and `Offset 2 is beyond the end of /one.md, which has 1 line.`). A negative offset starts at `max(1, N + offset + 1)` and never fails, so `-1` is the last line and with no limit it reads the last `|offset|` lines. The window ends at `N` with no limit, and at `min(N, start + limit - 1)` with one, so an overlapping window is clamped. On a 10-line file: no offset gives 1-10, `0` gives 1-10, `10` gives 10-10, `-2` gives 9-10, `-100` gives 1-10, `-2` with `limit` 5 gives 9-10, `limit` 3 gives 1-3, offset 4 with `limit` 3 gives 4-6, offset 9 with `limit` 5 gives 9-10, and `11` fails.
- Render, in `render.rs`. Each window line becomes `{number:>6}|{text}` plus `\n`: a six-wide right-aligned number, a pipe, no space, always on, widening past six digits. The cap is 100,000 bytes of those numbered lines, counting each prefix, the text, and its newline, and not the trailer or notes. Stop before the line that would pass the cap. After the numbered lines come the notes, then the trailer, each on its own line, joined by `\n` with no newline after the last one. The trailer is the last line of a cut read:
  - Cut by the cap: `[Showing lines {first}-{last} of {total} (100000-byte limit). Use offset={last + 1} to continue.]`, for example `[Showing lines 1-2083 of 5000 (100000-byte limit). Use offset=2084 to continue.]`.
  - Ended by the caller's `limit` before the last line of the file: the same text without the parenthesis, `[Showing lines {first}-{last} of {total}. Use offset={last + 1} to continue.]`.
  - A read that reaches the last line of the file prints no trailer, which includes the last page and every negative-offset tail read.
  - First-line cut: when the window's first line alone passes the cap, keep `clip_to_boundary(text, 0, cap - prefix_len - 1)` (99,992 bytes for a six-wide prefix and the default cap), print it numbered, show no other line, and make the trailer `[Line {n} is {bytes} bytes and is cut after {kept} bytes. No parameter reads the rest of a line.` followed by ` Use offset={n + 1} to continue.` when a later line exists in the file, then `]`. `bytes` is the line's full byte length and `kept` is the bytes kept, so ASCII gives `cut after 99992 bytes` and 3-byte characters give `cut after 99990 bytes`. This is the only cut inside a line. UTF-16 is decoded in full before the cut.
  - Notes, in this order, before the trailer: a UTF-16 file adds `[Note: the file is UTF-16LE and was decoded to text.]` (or `UTF-16BE`), and replaced bytes add `[Note: {n} invalid UTF-8 sequences in this file were shown as U+FFFD.]` using `plural(n, "invalid UTF-8 sequence")` for the count and noun, so one reads `1 invalid UTF-8 sequence`.
- Worked checks to reuse: 5,000 lines of 40 `x` give 2,083 numbered lines, the cap trailer above, and a length of at most 100,120 bytes. More worked examples are in `runvil-design/specs/cursor/master-read.md`; where one disagrees with this step, this step governs.

Definition and wiring:

- `schemas/read.json`: `offset` and `limit` become `"type": "integer"`. Rewrite the description: remove the `cat -n` line and the 2000-character-line line, and state the six-wide `{number}|{text}` format and that the prefix is not part of the line, the cap of `{byte_cap}` bytes of numbered lines and the trailer that names the next offset, the `{file_ceiling}`-byte ceiling, that binary files are refused, that Read opens any path the call names inside what the run has mounted, and that a link is read through unless it leaves the run's filesystem. Keep Cursor's batching line and the `File is empty.` line. Update the `offset` text: 1 is the first line, 0 reads as 1, negative values count from the end, and `-1` is the last line. The description may name other tools but must not name PDFs, `Shell`, or the Cursor workspace (an existing test refuses those).
- The description is built from the constants the code uses. Keep the placeholders `{byte_cap}` and `{file_ceiling}` in `read.json`, and have `spec.rs` fill them when it builds each `Definition`, from `crate::read::BYTE_CAP` and `crate::text::MAX_FILE_BYTES` (a small function keyed by the tool's segment, so steps 3 and 4 add their own placeholders). The built description holds `100000` and `16777216`.
- `schemas/EDITS.md`: replace the `read.json` section so it records the new description, the removed `cat -n` and 2000-character lines, and that `offset` and `limit` are integers.
- `src/pack.rs`: in `Std::call`, after `crate::check::check(...)`, dispatch on `definition.segment`: `"read"` calls `crate::read::call(cx.access(), definition, &args)`, and every other segment keeps the stub. Add `mod read;`, `mod refusal;`, and `mod text;` in `lib.rs`. Update the crate docs in `lib.rs` and the doc comments in `pack.rs` that say every tool is a stub: Read is real, and Write, StrReplace, Delete, and Glob are still stubs that answer `NOT_IMPLEMENTED`. Keep the wording rules `docs-claims.mjs` enforces.

Tests:

- Unit, `text/tests.rs`: the decode ladder (plain UTF-8; a UTF-8 mark hidden; UTF-16LE and UTF-16BE with a mark decoded and named; UTF-16 without a mark is binary; `61 FF 62 0A` gives `a`, U+FFFD, `b` and one invalid sequence; a file that ends mid-sequence counts one), the sniff boundaries above, `split_lines` (`a\nb`, `a\nb\n`, empty, `a\r\nb\r\n`, `first\rNEEDLE\rthird`), and `clip_to_boundary` over 1-, 2-, 3-, and 4-byte characters at every start and end offset, always valid UTF-8 and always inside the range asked for.
- Unit, `refusal/tests.rs` and `check-tests.rs`: as listed under the shared code, plus `refuse_unknown` (names the key, lists the sorted parameters with ` and ` before the last), the blank-path text with `"   "` as the value, and `plural` (`1 line`, `10 lines`, `0 lines`, `1 directory`, `2 directories`).
- Unit, `read/tests.rs` and its topic files (seed files through `TestCall::new(id).context().access()` with `Access::write`):
  - The numbering window table above, line totals, and carriage returns.
  - The cap and a page-by-page rebuild across files of 1, 2, 5, 37, 2,000, and 2,001 lines, each with and without a final newline: follow `Use offset=N` under caps of 100,000, 300, and 90 bytes and limits of none, 50, and 7, then assert the rebuilt lines equal the original and no page is empty.
  - The first-line cut on a 150,000-byte ASCII line (99,992 bytes kept), 50,000 CJK characters (99,990 kept, `cut after 99990 bytes`), a line of 2-byte characters (99,992 kept), and a line of 4-byte emoji (99,992 kept, 24,998 characters), always valid UTF-8.
  - A cap sweep from 12 to 60 bytes over a first line that mixes 1-, 2-, 3-, and 4-byte characters: at every cap the kept bytes are valid UTF-8, a prefix of the line, within the allowance, and the notice's count equals the kept bytes.
  - A file that ends mid-sequence, the offset table on a 10-line file, a binary file (`File content is binary data, not text: {path}`), the file-type function over every `FileType`, and the empty-file answer with `offset: 500`.
  - Every failure text byte for byte: not found for a missing path (also a relative `notes/a.md` echoed as written), a directory, too large (a file of 16,777,217 bytes), offset past the end, and the blank path.
  - The argument rules: `limit` 0 and `limit` -3, a string `offset`, an undeclared key, and `null` read as absent.
  - A trust test: every successful read is `OutputTrust::Untrusted`.
  - The description pin, in `spec-tests.rs`: the built `read` description contains `100000` and `16777216`, does not contain `cat -n` or `2000 characters`, and holds no unfilled `{byte_cap}` or `{file_ceiling}`.
- Integration, `tests/it/tools.rs`: add a helper that seeds files through `call.context().access()` before the call. Remove `("read", "Read")` from `every_tool_answers_the_not_implemented_text_naming_itself` (the other five stay). Add Read tests: a seeded `/notes.md` of `hello\nworld\n` answers `     1|hello\n     2|world\n` as untrusted output, a zero-byte file answers `File is empty.`, and a missing path fails with `File not found: /nope.md`.
- Not testable here and not required: a two-call claim conflict (the conflict text is covered by the refusal test). Do not add a test that needs the vfs crate.

Commit: one commit holding the shared code, the Read tool, `read.json`, `EDITS.md`, `Cargo.toml`, `Cargo.lock`, the doc updates, and every test. Suggested subject: `Add shared std file-tool code and a real Read`.

</step-2>

<step-3>

### Step 3: Glob matcher and real ListDir [completed]

- Component: `dir-tool`
- Tests:
  - `cargo nextest run --locked -p plugin-std`
  - `cargo clippy --locked -p plugin-std --all-targets -- -D warnings`
  - `cargo fmt --all --check`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

Placement: third. ListDir needs the shared refusal text, argument helpers, and `plural` from step 2, and Grep in step 4 needs the matcher, so the matcher lands here and step 4 starts with a tested matcher. ListDir is smaller than Grep and is the matcher's first caller, which keeps the matcher free of dead code. The component has two pieces, the matcher and the ListDir tool, built jointly in one step: the matcher's only caller this step is ListDir's `ignore_globs`, and its behavior is only visible through that call.

Rules for every step in this plan: warnings are denied and clippy runs `pedantic`, so add no item before something uses it (an unused `pub(crate)` item fails the build) and give every item a doc comment. Tests may use `unwrap` and `expect`. New code adds no `#[path]` attribute: a module is `foo.rs` beside a `foo/` directory, with its unit tests in `foo/tests.rs` (split into a `tests/` directory by topic when a file nears 500 lines). Every Rust file stays at or under 500 lines. Do not touch the Engine, the Harness, the vfs crate, or any file under `vibe/`. Do not make `plugin-std` depend on the vfs crate: import `Access`, `VfsError`, `Entry`, `FileType`, and `Stat` from `runvil_plugin`. The ListDir tool is the seventh tool, appended after `grep`.

Code from step 2 that this step uses, already in the tree: `src/refusal.rs` (`refusal(tool, path, &VfsError) -> String`, `neither_file_nor_directory`), `src/check.rs` (`check`, `refuse_unknown`, the blank-path helper, `plural(n, noun)`), and `src/text.rs` (`decode`, `sniff_binary`, `split_lines`, `clip_to_boundary(text, start, end)`, `MAX_FILE_BYTES`). `spec.rs` has a small fill function that replaces `{name}` placeholders in a tool's description with numbers taken from the code's constants.

Matcher, `crates/plugin-std/src/matcher.rs` with tests in `matcher/tests.rs`. The crate cannot import the vfs matcher, so port its four-token grammar from `crates/runvil-internal/vfs/src/glob.rs` (read it first) and keep the same refusals:

- API: `pub(crate) struct Matcher`, `Matcher::new(pattern: &str) -> Result<Matcher, PatternError>`, and `Matcher::is_match(&self, path: &str) -> bool`. `path` is an absolute virtual path with a leading `/`. `PatternError` has a `reason()` text for each refusal.
- Grammar: only `*` and `**` are special. `*` matches zero or more bytes except `/` inside one segment and does match dotfiles. `**` as a whole segment matches zero or more whole segments. Every other byte is literal, including `?`, `[`, `{`, `!`, and `.`, and matching is case-sensitive. A pattern with no leading `/` matches at any depth (match it as if `**/` came first); a pattern with a leading `/` is a full path from the root. This differs from `Access::glob`, where a relative pattern is anchored at the access root.
- Refusals, with these reason texts: empty (`pattern is empty`), longer than 1,024 bytes (`pattern is longer than 1024 bytes`), a control byte below 0x20 or 0x7f (`pattern contains a control character`), a backslash (`pattern does not support backslash escapes`), more than two `*` in a row (`more than two consecutive '*' are not supported`), and `**` as less than a whole segment (`'**' must occupy a whole path segment`). The argument error is ``std_{Wire}: `{name}` is invalid: "{pattern}" does not parse: {reason}.`` with `{name}` the parameter (`ignore_globs` here), and the echoed pattern cut to 80 bytes with `clip_to_boundary` and `...` added when it was cut. Build this text in `check.rs` beside the other argument helpers.
- Tests: each grammar rule, dotfiles matched by `*`, any-depth matching, and each refusal. Pin the cases it shares with `Access::glob` against that function's own cases from `glob.rs`, run anchored with a leading `/` on the pattern and on each path: the validation table (1,025 `a` is too long; `a\u{0}b` is a control byte; `a\b` is a backslash; `a/***/b` and `a/**b` are wildcard refusals; `ok/*.txt` is fine) and the match table over the paths `a.txt`, `src/a.rs`, `src/b.rs`, `src/deep/c.rs`, `src/deep/deeper/d.rs`, `notes/today.md`: `*.txt` gives true, false, false, false, false, false; `src/*.rs` gives false, true, true, false, false, false; `src/**/*.rs` gives false, true, true, true, true, false; `**/*.md` gives false, false, false, false, false, true; `src/**` gives false, true, true, true, true, false; `no*match` gives all false. Add one test that records the single difference: unanchored `*.txt` also matches `/x/y/a.txt`, which `Access::glob("*.txt")` would not.

ListDir, in `crates/plugin-std/src/dir.rs` (entry) and `src/dir/` (`args.rs`, `walk.rs`, `render.rs`, `tests.rs` and a `tests/` directory as needed):

- Entry: `pub(crate) fn call(access: &Access, definition: &Definition, args: &Value) -> Result<ToolOutput, ToolError>`. No yielding: the byte cap bounds the work. Add `pub(crate) const BODY_CAP: usize = 15_000` to `text.rs`, the cap Grep shares in step 4. The cap is a parameter of the walk and render functions with this constant as the default. The walk takes its directory lister as a parameter (a closure over `Access::list`), so a test can supply a `Symlink` entry that the in-memory backend cannot make.
- Arguments: `target_directory` (string, required) and `ignore_globs` (array of strings). Order: `check::check`, `refuse_unknown`, the blank-path helper (a blank `target_directory` is refused before any I/O: ``std_ListDir: `target_directory` is empty or only whitespace, and the call passed "{value}"``), then compile every `ignore_globs` entry with `Matcher::new` (a bad one is an `InvalidArguments` error), then `access.stat(target_directory)`.
- Stat failures: `NotFound` gives `Path does not exist: {path}` through `refusal::missing_root` (add it now). Any other stat failure goes through `refusal(...)`. A `Directory` or `Symlink` goes on to the walk (a link named as the target is walked through, and if listing it fails with `NotADirectory` the answer is `Path is not a directory: {path}`). Any other file type gives `Path is not a directory: {path}`. Failures are `ToolError::message(...)`, `path` is as the model wrote it.
- Output is a tree, lines joined with `\n` and no trailing newline. The root line is the absolute virtual path plus `/`: the target as written, with a leading `/` added when it lacks one (no canonical path is available), and `/` appended unless it already ends in `/` (root `/` stays `/`). Each child is indented two spaces per depth with a `- ` bullet and a trailing `/` on directories only. Files and directories share one list in byte order of the name with `/` appended to a directory's name (the Pack sorts each listing itself, because `Access::list` promises no order). A link is listed by its name with no trailing `/` and is never entered. Entries named `.git`, file or directory, are skipped at any depth. Every other dotfile is listed, so `.env` and `.gitignore` appear. An entry that matches an `ignore_globs` pattern is left out, and a matching directory is not entered; match against the entry's absolute virtual path (the root as written plus the names), with a directory's path having no trailing `/`. A root with no children to show prints `{root line}` then `\n... no children found ...`, with no indent, for example `/home/dev/proj/empty/` then `... no children found ...`.
- The walk is breadth first, so it stops at the byte cap instead of walking a whole tree first. Count the bytes of every tree line, each with its newline, against `BODY_CAP`. When the next line would pass the cap, stop everything: the directory being listed keeps the children already added, and every directory shown but not expanded ends in `...` (for example `  - src/...`). Then the output ends with `\n    ...` (four spaces, then three dots), a blank line, and `target_directory is too large (either too many children or their names are too long). Immediate children were truncated. Try a more specific target_directory or ignore_globs.` These two do not count against the cap. Printing walks the finished tree depth first in the sorted order.
- Every successful ListDir returns `ToolOutput::untrusted`. Dropped from Cursor's `LS`: extension-count summaries, the 5-second timeout, the terminal-log special case, and the `Error listing directory:` prefix.

Definition and wiring:

- New `schemas/dir.json`: `wire_name` `ListDir`; parameters `target_directory` (string, the absolute path of the directory to list) and `ignore_globs` (array of strings, globs of entries to leave out; a pattern with no leading `/` matches at any depth, and only `*` and `**` are special); `required` is `["target_directory"]`. The description says: a bullet tree of the directory, directories end in `/`, files and directories share one byte-ordered list, the walk is breadth first, output stops at `{body_cap}` bytes with a note, entries named `.git` are skipped and every other dotfile is listed, links are listed and not entered, and `ignore_globs` match at any depth. Fill `{body_cap}` from `crate::text::BODY_CAP` with the fill function in `spec.rs`. The description names no PDFs, no `Shell` tool, and no Cursor workspace.
- `schemas/EDITS.md`: add a `dir.json` section that says this file is new, is not a Cursor snapshot, and why (Cursor's counterpart `LS` is deprecated and gated behind a setting, so the wire name is `ListDir`).
- `src/spec.rs`: `SOURCES` becomes seven entries with `("dir", include_str!("../schemas/dir.json"))` last; update its module doc ("seven"). `src/pack.rs`: add `"dir"` to the dispatch. `lib.rs`: add `mod dir;` and `mod matcher;`, add `| std/dir | ListDir |` to the table in the crate docs, say seven tools, and say Read and ListDir are real while Write, StrReplace, Delete, and Glob are stubs.
- Move the tool-count tests from six to seven: `the_specs_are_the_six_the_crate_names_in_order` in `pack-tests.rs` becomes seven with `["read", "write", "replace", "rm", "glob", "grep", "dir"]`; `WIRE_NAMES` in `spec-tests.rs` becomes `[(&str, &str); 7]` with `("dir", "ListDir")` last; in `tests/it/tools.rs`, `construct_names_six_tools_under_the_pack_id_it_receives` becomes seven and lists `files/dir` last, and the wire-name test adds `("std/dir", Some("ListDir"))` last. Update the `tools.rs` module doc that says six.

Tests:

- Unit, `dir/tests.rs` and topic files:
  - The POSIX tree. Seed `/home/dev/proj` with `README.md`, `package.json`, `docs/guide.md`, `src/app.tsx`, `src/index.ts`, and `src/utils/math.ts`, call with `{"target_directory": "/home/dev/proj"}`, and assert exactly this text (the parent report `runvil-design/specs/cursor/agent-file-tools.md` shows `package.json` before `README.md` because Cursor sorts with `localeCompare`; this plan's byte order governs, so `README.md` comes first):
    ```text
    /home/dev/proj/
      - README.md
      - docs/
        - guide.md
      - package.json
      - src/
        - app.tsx
        - index.ts
        - utils/
          - math.ts
    ```
  - An empty directory (`/home/dev/proj/empty/` then `... no children found ...`), a root holding only `.git`, a file as the target (`Path is not a directory: {path}`), a missing path (`Path does not exist: {path}`), and a blank target.
  - `.git` skipped while `.env` and `.gitignore` are listed, and a `.git` that is a file skipped too.
  - Sort order against `Access::glob`'s for the same tree: seed `/B`, `/a-b/x`, `/a.txt`, `/a/x`, `/b` and assert the tree lists `B`, `a-b/`, `a.txt`, `a/`, `b` in that order, the same order `access.glob("/**")` gives for the files (`/B`, `/a-b/x`, `/a.txt`, `/a/x`, `/b`).
  - A `Symlink` entry through the lister parameter is listed by name with no `/` and is never entered.
  - The byte cap and its note: lower the cap, assert the `...` suffix on unexpanded directories, the `\n    ...` line, the blank line, and the too-large sentence byte for byte; sweep the cap over a tree of multi-byte names and assert the output is valid UTF-8 and every cut sits on a line boundary.
  - `ignore_globs` matching at any depth (`node_modules` and `*.log` leave out entries at every level, `/build` only the one at the root), and a refused pattern (`a***`, `a/**b`, an empty string) as an `InvalidArguments` error with the text above.
  - A trust test: every successful ListDir is `OutputTrust::Untrusted`.
  - The description pin, in `spec-tests.rs`: the built `dir` description contains `15000` and no unfilled `{body_cap}`.
- Integration, `tests/it/tools.rs`: add a ListDir test that seeds a small tree through the call's access and asserts the tree text as untrusted output.

Commit: one commit holding the matcher, ListDir, `dir.json`, `EDITS.md`, `spec.rs`, the dispatch, the doc updates, the six-to-seven test edits, and every test. Suggested subject: `Add the std glob matcher and a real ListDir`.

</step-3>

<step-4>

### Step 4: Real Grep, the real-directory test, and the full gate set [completed]

- Component: `grep-tool`
- Tests:
  - `cargo nextest run --locked -p plugin-std`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo fmt --all --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo +nightly-2026-09-05 xtask api --check`

Run both clippy commands with `CARGO_BUILD_WARNINGS=deny` (PowerShell: `$env:CARGO_BUILD_WARNINGS = "deny"`), run the doc command with `RUSTDOCFLAGS` set to `-D warnings` (PowerShell: `$env:RUSTDOCFLAGS = "-D warnings"`), and run the api check with `CARGO_BUILD_WARNINGS` unset (PowerShell: `Remove-Item Env:CARGO_BUILD_WARNINGS`). The nightly name must match `PINNED` in `crates/build-xtask/src/api/toolchain.rs`.

Placement: last, because Grep needs the text module and refusal text from step 2 and the matcher from step 3, and because the full gate set and the real-directory test need all three tools real. The component has three pieces. The Grep tool and its yield are built jointly, because the walk and the yield are one loop and the yield is tested by polling that same future. The real-directory test comes after them in sequence, because it drives ListDir, Grep, and Read together. The full gate set runs last and replaces a separate gates step. This is the largest step and it stays one commit, per the four-step decision.

Rules for every step in this plan: warnings are denied and clippy runs `pedantic`, so add no item before something uses it, give every item a doc comment, and put `#[expect(..., reason = "...")]` on the casts that turn JSON numbers into `usize`. Tests may use `unwrap` and `expect`. New code adds no `#[path]` attribute: a module is `foo.rs` beside a `foo/` directory, with its unit tests in `foo/tests.rs`. Every Rust file stays at or under 500 lines. Do not touch the Engine, the Harness, the vfs crate (this step changes no file in it), or any file under `vibe/`. Do not make `plugin-std` depend on the vfs crate: import `Access`, `VfsError`, `Entry`, `FileType`, and `Stat` from `runvil_plugin`.

Code from steps 2 and 3 that this step uses, already in the tree: `src/refusal.rs` (`refusal(tool, path, &VfsError) -> String`, `missing_root`), `src/check.rs` (`check`, `refuse_unknown`, the blank-path helper, `plural(n, noun)`, and the matcher-refusal text builder), `src/text.rs` (`decode`, `sniff_binary`, `split_lines`, `clip_to_boundary(text, start, end)`, `MAX_FILE_BYTES`, `BODY_CAP`), `src/matcher.rs` (`Matcher::new`, `Matcher::is_match`), and the fill function in `spec.rs` that replaces `{name}` placeholders in a description with numbers from the code's constants. The shapes this step relies on: the unknown-key text is ``std_{Wire}: `{key}` is not a parameter of this tool. The parameters are `a`, `b` and `c`.`` with the names sorted bytewise; the blank-path text is ``std_{Wire}: `{name}` is empty or only whitespace, and the call passed "{value}"``; `plural(n, noun)` gives `1 line` and `10 lines`, and `directory` becomes `directories`.

Definition, in `crates/plugin-std/schemas/grep.json`:

- Replace the property set with the bundle schema plus `literal`, in this order: `pattern` (string, required), `path`, `glob`, `output_mode`, `-B`, `-A`, `-C`, `-i`, `type`, `head_limit`, `offset`, `multiline`, `literal`. Types: strings for `pattern`, `path`, `glob`, `type`; `output_mode` is a string enum `["content", "files_with_matches", "count"]` in that order, default `content`; `-B`, `-A`, `-C`, `head_limit`, and `offset` are `number`; `-i`, `multiline`, and `literal` are `boolean`. `-n` and `context` are deleted. `-C` takes the text that was on `context`: `Number of lines to show before and after each match. Requires output_mode: "content", ignored otherwise.` `head_limit` has no default, `"minimum": 0`, and its text says `0` means no limit. `path` reads `File or directory to search in, as an absolute path. Defaults to the run's filesystem root.` `glob` reads `Glob pattern to filter files, for example "*.js" or "**/*.tsx". Only * and ** are special, and matching is case-sensitive. A pattern that does not start with "/" is matched at any depth. A pattern that starts with "/" is a full path.` `literal` is a boolean, default false, whose text says the pattern is matched as plain text. Keep Cursor's wording for the other parameters, with `output_mode` now defaulting to `content`. Keep the hyphenated names and add no aliases. The existing integration tests for a missing `pattern` and for `output_mode: "lines"` must still pass with the same texts.
- Description: name the dialect (`Rust regex` syntax, with no look-around and no back-references; escape `( [ { .` or set `literal`), the order (path order, the order Glob uses), the limits (output stops at `{body_cap}` bytes, filled from `crate::text::BODY_CAP`; at most 100,000 files are considered; a file over `{file_ceiling}` bytes is not searched, filled from `crate::text::MAX_FILE_BYTES`), what is skipped (binary files, symbolic links, over-size files, and unreadable entries are skipped and the result says how many were skipped and why), the ignore rule (only entries named `.git` are skipped and `.gitignore` rules are not applied, so output such as `target/` and `node_modules/` is searched when it lies under `path`), that a line over 1000 bytes is shown as a window around its first match, that paths are absolute and work as the `path` argument of the other file tools, that `N:` and `N-` are not part of the line and `N` is the line number Read shows, and that the model should read the file before changing it. The text does not contain `ripgrep`, `rg ` (check that no word ends in `rg` before a space), `several thousand`, or `Shell`. Fill the two placeholders with the fill function in `spec.rs`. Record every change in `schemas/EDITS.md`: the new bundle parameters, the deleted `-n` and `context`, the new `literal`, the `content` default, the new description, and that the `rg` and `Shell` removals from the earlier snapshot still hold.

Arguments, in `src/grep/args.rs`. `null` reads as absent and numeric strings are not coerced (the shared `check` does both). An empty `path`, `glob`, or `type` reads as absent, and a whitespace-only `path` is refused with the blank-path text. Every error below is a `ToolError` of kind `InvalidArguments`. Check in this order and stop at the first failure:

1. `check::check`, then `refuse_unknown` run on a copy of the arguments with `-n` removed (so `-n` is the one undeclared key that is accepted, and the shared helper stays unchanged). The refusal names the key and lists the thirteen parameters, sorted, in the shape above.
2. `-n`: `true` or absent is accepted, and any other value is refused with ``std_Grep: `-n` must be true or left out, because line numbers are always shown.``
3. Empty pattern: ``std_Grep: `pattern` is empty. Pass "." to match every line.``
4. A negative `-A`, `-B`, or `-C`, checked in that order: ``std_Grep: `-A` is -1. Pass 0 or more context lines.`` with the key and the passed value filled in.
5. A negative `head_limit` or `offset`, in that order: ``std_Grep: `head_limit` is -5. Pass 0 or more.``
6. An unknown `type`: ``std_Grep: Unrecognized file type "foo". Use a type such as js, ts, py, rust, go or java, or filter with glob.``
7. An invalid `glob` (`Matcher::new` fails), using the shared matcher-refusal text builder in `check.rs` with `{name}` set to `glob`: ``std_Grep: `glob` is invalid: "{glob}" does not parse: {reason}.``
8. A whitespace-only `path`: ``std_Grep: `path` is empty or only whitespace, and the call passed "   "``.
9. The pattern compile (below), then the `path` stat.

Rounding: `head_limit` and the context values round up, and `offset` rounds down. Absent `head_limit` and `head_limit: 0` both mean no limit. Effective context: `-C` sets both sides, then `-A` sets the after side and `-B` the before side, each overriding `-C` even when it is 0. Context applies only in `content` mode and is ignored in the other two modes.

Pattern compile, before any filesystem call, with `regex::RegexBuilder`: `case_insensitive` from `-i`, `multi_line(true)` always, and `dot_matches_new_line(true)` and `crlf(true)` only with `multiline`. `literal` compiles `regex::escape(pattern)`. A failure returns ``std_Grep: Invalid regular expression "<pattern>": <reason>. Grep matches with Rust regex syntax, which has no look-around and no back-references. To match the text literally, set literal to true.`` where `<reason>` is the text after the first `error: ` in the crate's message, or the whole message trimmed when it has none. Outside `multiline` matching is line by line; with it the whole decoded text is one subject. The default is case-sensitive.

Type table, in `src/grep/types.rs`, matched by file-name extension (the text after the last `.` when that dot is not the first character), case-sensitive: `js` (js, mjs, cjs, jsx), `ts` (ts, tsx, mts, cts), `py` (py, pyi), `rust` (rs), `go` (go), `java` (java), `c` (c, h), `cpp` (cc, cpp, cxx, hh, hpp, hxx, h), `cs` (cs), `rb` (rb), `php` (php), `md` (md, markdown), `json` (json), `yaml` (yaml, yml), `toml` (toml), `html` (html, htm).

Search, in `src/grep/find.rs`. `find` is a pure async function over a small `Source` trait that `Access` implements through a thin adapter (`stat`, `list`, `read`, all synchronous), and returns what it found; `render` turns that into text. The seam exists because the in-memory backend cannot produce a symbolic link, a control-character name, an over-size entry, a denied read, or a conflict, and the tests need them. `Limits` is a struct of parameters with the constants as defaults: `max_files` 100,000, `max_bytes` 1 GiB, `max_file_size` 16,777,216, `slice_files` 32, `slice_bytes` 2 MiB, and the output cap `BODY_CAP`; tests lower each.

- Root: `path` defaults to `/`. A root without a leading `/` is made absolute by adding one. `Access::stat` runs first. `NotFound` fails the call with `Path does not exist: {path}` through `refusal::missing_root`, and any other stat failure fails through `refusal(...)`, with `path` as the model wrote it. A `Directory` is walked, and a failure to list the root itself fails the call through `refusal(...)`. A `File` is searched alone, with no ignore rule, and `glob` and `type` do not filter it, because the model named it. A `Symlink` is tried as a directory and then as a file, so a `path` that names a link is searched through it.
- Order: depth first, each directory's entries in byte order of the name with `/` appended to a directory's name (sorted by the Pack, same order as `Access::glob`). For example the files `/B`, `/a-b`, `/a.txt`, `/a/x`, `/b` come in exactly that order. No clock is read. Result lines print the absolute virtual path with its leading `/`: the root as written plus the names.
- Entries inside the walk: an entry named `.git` is skipped silently; any entry whose name has a character below 0x20 or 0x7f is a counted skip (`name with a control character`) and its name is never printed; a `Symlink` entry is a counted skip (`symbolic link`) and is never entered or read; a `Directory` is entered; every other entry is a file. For each file: count it as considered (when a 100,001st file would be considered, stop with the files notice below), apply `glob` (the step-3 matcher on the absolute path) and `type` (a file that fails a filter is neither searched nor a skip), then skip it as `over 16 MiB` when its size in the directory listing is above 16,777,216 bytes, decided before any read. Then read it with `Access::read` (never `read_string`), `sniff_binary` (a binary file is a counted skip `binary`, in every output mode), `decode`, and `split_lines`, so a Grep line number names the line Read prints. Searching a file counts it as searched.
- No single entry fails the search. Skip reasons and how a `VfsError` maps: `Conflict` is `another call in this round`; `PermissionDenied` and `ApprovalRequired` are `denied by policy`; any other listing failure is `unreadable directory`; any other read failure is `unreadable file` (a reason this plan adds to the seven above, so a failed read is never silent). Each skip is counted by reason, and the first five printable skips are kept in walk order for the skip line.
- Limits: stop at 100,000 files considered, or when another file would be read after 1 GiB has been read. A limit stop prints its notice and offers no `offset`, because a re-run stops at the same file.
- Yield: after every 32 files considered or 2 MiB read since the last yield, `.await` a hand-written `Pause` future in `src/grep/pause.rs` (a struct whose `poll` wakes its own task through `cx.waker().wake_by_ref()` and returns `Pending` once, then `Ready`; name no async runtime; do not call the module `yield`, which is a reserved word). Read and ListDir do not yield. A cancel or stop drops the future at its next await point.
- Collecting: counting is global across files and in entries (a matching line in `content` mode, a file in `files_with_matches`, a file row in `count`), and context lines are never entries. Skip the first `offset` entries, collect `head_limit` more plus one extra entry that only decides whether a notice is due, then stop the walk; with no head limit collect all. Also stop collecting once the entries after `offset` already fill the body cap and one more exists, because entries past that point are never shown. Keep memory bounded: store only the lines a kept entry shows (the match line as its windowed text, and its context lines clipped), never a whole decoded file after it is processed. In `multiline` mode every line a match touches is a match line and one entry (a match covers the lines from the one holding its first byte to the one holding its last byte; an empty match touches the line it starts on); map byte offsets to line numbers over the decoded text, and strip the one trailing `\r` only from the displayed text.

Render, in `src/grep/render.rs` and `src/grep/window.rs`. There is no envelope.

- `content` mode: each file's absolute virtual path appears once, then its lines as two spaces, the line number with no padding, `:` for a match or `-` for context, and the text (`  N:text`, `  N-text`). A blank line separates files and none ends the output. Between two non-adjacent groups of one file print a gap line, two spaces then two hyphens, whenever the effective before or after context is above 0. Overlapping or touching context merges and no line prints twice. `files_with_matches` prints one path per line, and `count` prints `path:count` with the matching-line count of that file (the model reads the count after the last colon). Worked example for `const` with `-A: 1`, where `/src/a.ts` matches on lines 3 and 9 of 10 and `/src/b.ts` matches on line 1:
  ```text
  /src/a.ts
    3:const alpha = 1
    4-let beta = 2
    --
    9:const gamma = 3
    10-export default gamma

  /src/b.ts
    1:const first = 0
  ```
- Long lines, over 1,000 bytes. A matching line becomes a window of at most 1,000 bytes that contains the first match and starts up to 200 bytes before it: `start = first_match_start - 200` (not below 0), then `clip_to_boundary(text, start, start + 1000)`. Put `[... {n} bytes omitted]` directly before the window when its start is above 0 and directly after it when its end is short of the line, so the two counts plus the window equal the line's byte length. In `multiline` mode the first match on a line is the first byte of the line a match covers. A context line over 1,000 bytes keeps `clip_to_boundary(text, 0, 1000)` and a trailing `[... {n} bytes omitted]`. These two cuts, in `window.rs`, are the only cuts inside a line and both go through `clip_to_boundary`, so no cut splits a code point. The 1,000-byte figure and the 200-byte lead are parameters with these defaults.
- Output cap: 15,000 bytes of body in every mode (`BODY_CAP`), counting each line with its newline, cut on a line boundary: stop before the line that would pass it. The paging, skip, note, and limit lines described below are not counted. The cap is a parameter, so tests can lower it.

Output assembly, in `src/grep/render.rs`:

- The text is the body, then, when there is anything to add, a blank line and then the extra lines, each on its own line and joined by `\n`, with no newline at the end. Extra lines, in this order: the replacement note, the skip line, then the paging or limit notice. A zero-match result puts `No matches found. Searched {plural(n, "file")}.` where the body would be (for example `No matches found. Searched 412 files.` and `No matches found. Searched 1 file.`), followed by the extras in this order: the replacement note, the skip line, the line `[No ignore rules ran: .gitignore is not applied, and only entries named .git are skipped.]`, and, when `glob` holds `{`, `[`, or `?`, the line `{, [ and ? in glob match themselves; there is no alternation or character class. Call Grep once per alternative, or filter with type.`
- Replacement note: for each searched file that needed a replacement, `[Note: invalid UTF-8 in {plural(n, "file")} was shown as U+FFFD: {path}, {path}]`, which reads `invalid UTF-8 in 2 files was shown as U+FFFD: /a.txt, /b.txt` and `invalid UTF-8 in 1 file was shown as U+FFFD: /a.txt`. Name the first five paths and end the list with `, and {n - 5} more` when there are more.
- Skip line: `[Skipped {plural(total, "entry")}: {phrases}. First: {paths}]`, for example `[Skipped 5 entries: 3 binary files, 1 directory that could not be read, 1 file over 16777216 bytes. First: /a.bin, /b.bin, /c.bin, /d, /big.log]` and `[Skipped 1 entry: 1 binary file. First: /a.bin]`. The phrases come in this fixed order, each with its count and the `plural` helper: binary (`{n} binary file(s)`), unreadable directory (`{n} director(y/ies) that could not be read`), unreadable file (`{n} file(s) that could not be read`), over 16 MiB (`{n} file(s) over 16777216 bytes`), symbolic link (`{n} symbolic link(s)`), name with a control character (`{n} name(s) with a control character`), another call in this round (`{n} entr(y/ies) held by another call in this round`), and denied by policy (`{n} entr(y/ies) denied by policy`). `First:` lists up to five printable skipped paths in walk order; a control-character name is never printed, and when no printable skip remains the ` First: ...` part is dropped.
- Paging notice. There is no cache, and each call re-runs from the start. The unit word is `matching lines` in `content` mode and `files` in the other two. Print no notice when `offset` is 0 and nothing was cut, so an exact fit prints none. A cut by `head_limit` or by the body cap ends with `[Showing {offset + 1}-{offset + shown} of at least {offset + collected} {unit}. Use offset={offset + shown} to see more, or narrow the search.]`, where `shown` is the entries printed and `collected` is the entries gathered including the extra one; for example `[Showing 1-50 of at least 51 matching lines. Use offset=50 to see more, or narrow the search.]`. When the body cap caused the cut, insert `Output stopped at 15000 bytes.` after the `of at least` clause: `[Showing 1-120 of at least 121 matching lines. Output stopped at 15000 bytes. Use offset=120 to see more, or narrow the search.]`. A page that reaches the end after the first page shows an exact total, `[Showing 51-60 of 60 matching lines.]`. An `offset` past the end prints only `No more results at offset={offset}.` A limit stop prints `[Search stopped after considering 100000 files. Narrow path, glob or type to search the rest.]` or `[Search stopped after reading 1073741824 bytes. Narrow path, glob or type to search the rest.]` and offers no `offset`.
- Trust: every Grep result is `ToolOutput::untrusted`, except a zero-match result that has no replacement note and no skip line, which holds only fixed text and is `ToolOutput::trusted`. `No more results at offset=N.` is untrusted.

Files for Grep: `src/grep.rs` (entry: `pub(crate) async fn call(access: &Access, definition: &Definition, args: &Value) -> Result<ToolOutput, ToolError>`), and under `src/grep/`: `args.rs`, `types.rs`, `find.rs`, `pause.rs`, `render.rs`, `window.rs`, and `tests.rs` with a `tests/` directory split by topic (arguments, find and skips, render, window, paging) so no file passes 500 lines.

Wiring and docs:

- `src/pack.rs`: `"grep"` dispatches to `crate::grep::call(cx.access(), definition, &args).await`. Only Write, StrReplace, Delete, and Glob keep the stub. `lib.rs`: add `mod grep;`, and update the crate docs: Read, ListDir, and Grep are real, the other four are stubs that answer `NOT_IMPLEMENTED`, and the invariant about undeclared keys now says the three real tools refuse them by name while the stubs let them through.
- `tests/it/tools.rs`: remove `("grep", "Grep")` from `every_tool_answers_the_not_implemented_text_naming_itself`, leaving the four stubs (`write`, `replace`, `rm`, `glob`) with their stub tests. Add a Grep test that seeds `/src/a.ts` and asserts the `content` output above as untrusted text, and keep the missing-`pattern` and bad-`output_mode` tests as they are. Add new integration modules (for example `tests/it/grep.rs`, declared in `tests/it/main.rs`) if `tools.rs` nears 500 lines.

Tests:

- Unit, arguments: every string above byte for byte, in order, the empty-string rules (`path: ""`, `glob: ""`, and `type: ""` read as absent; `path: "   "` refused), undeclared-key refusals (`context: 3` is refused naming `context` and listing the sorted parameters; `-n: false` is refused with the `-n` text; `-n: true` is accepted), the rounding (`head_limit` 2.2 gives 3, `offset` 2.9 gives 2), and the `-C` with `-A` and `-B` override.
- Unit, regex: send `(?=x)y`, `(?<=a)b`, `(a)\1`, and `foo(` and assert a `std_Grep: Invalid regular expression "<pattern>": ` error for each that names the reason and the Rust dialect, as an error and not as success text. `a.b` with `literal: true` over files holding `axb` and `a.b` gives one match. `^(a|aa)+$` over a 1,600,000-character line completes. `multiline` with `a.b` over `a\nb` matches, and without `multiline` it does not. `alpha` over `Alpha`, `alpha`, `ALPHA` gives one match, and three with `-i`.
- Unit, search and order: full-path order on `/B`, `/a-b`, `/a.txt`, `/a/x`, `/b` in all three modes; `head_limit` global across files (`/a.ts` with matches on lines 12 and 40 and `/b.ts` with three: `head_limit: 2` shows only `/a.ts` with both lines and a notice saying `at least 3`; three files of three matches with `offset: 4, head_limit: 2` return the 2nd and 3rd matches of the second file; `head_limit: 0` with 40 matches prints all 40; `head_limit: 2` with exactly two matches prints no notice; `-A: 5, head_limit: 2` gives two entries and their context lines).
- Unit, paging: seed 25 matches and run `head_limit: 10` at offsets 0, 10, and 20: the three pages joined equal the unpaged result, and the notices are `[Showing 1-10 of at least 11 matching lines. Use offset=10 to see more, or narrow the search.]`, `[Showing 11-20 of at least 21 matching lines. Use offset=20 to see more, or narrow the search.]`, and `[Showing 21-25 of 25 matching lines.]`. Offset 30 gives `No more results at offset=30.` A write to a seeded file between two page calls shows in page 2 (no cache). A lowered file limit gives the files notice with no `offset`.
- Unit, render: the worked example above; `-C: 3, -A: 1` with matches at lines 5 and 20 of a 30-line file gives lines 2-6 and 17-21; `-C: 2` with matches at lines 5 and 7 gives lines 3-9 once with no gap line; no context flag gives no gap line; the last 30 bytes of a two-file result end at `const first = 0` with no trailing newline; `count` mode prints `path:count` (also for a file name that holds a colon, if the memory backend takes the name); the byte cap with 400 matching lines of 100 bytes gives a body of at most 15,000 bytes and the notice with `Output stopped at 15000 bytes.`.
- Unit, windows: a 2,000-character line with `NEEDLE` at position 1,001 shows `NEEDLE`, both markers, and omitted counts that plus the window equal the line's byte length; a line of 600 CJK characters, and a line of 4-byte emoji with the match in the middle, give valid UTF-8 and whole characters at both ends; a context line of 600 CJK characters is clipped to at most 1,000 bytes of valid UTF-8; sweep the window's start over every byte position inside one 3-byte character, and sweep the body cap and the window sizes over emoji and CJK text, asserting valid UTF-8 and line-boundary cuts everywhere.
- Unit, skips and limits, through the `Source` seam with lowered limits: a binary file, an over-size file, a denied directory (`PermissionDenied` on list), a denied file, a `Conflict` on a file, a symbolic link, a control-character name, an unreadable file (an `Io` error), and one clean match. Assert the clean match, the skip line with the counts and the first five printable paths in walk order, the zero-match line with the same skip line and the right searched count (`Searched 1 file.` for one), and that a missing `path` fails with `Path does not exist: /x`. Reach each limit (files considered, bytes read, per-file size, slice size) by lowering its parameter. A `[Skipped 1 entry: 1 binary file. First: /a.bin]` case.
- Unit, text (through the shared `text` module, so a Grep line number is the number Read prints): `caf\xE9 NEEDLE` matches on line 1 with U+FFFD and the replacement note; a file that starts with a byte order mark and `NEEDLE first` matches `^NEEDLE` on line 1 with no mark in the output; `a\r\nNEEDLE\r\n` matches on line 2 with no `\r`; `first\rNEEDLE\rthird` is one match on line 1; UTF-16LE with a mark and `NEEDLE` on line 2 matches on line 2; UTF-16LE without a mark is one binary skip; 100 bytes with 5 control bytes and no NUL is a binary skip and 4 control bytes is a match; `é` as one code point and as two each match only their own file; two replaced files give `[Note: invalid UTF-8 in 2 files was shown as U+FFFD: /a.txt, /b.txt]`.
- Unit, filters: `glob: "*.js"` matches at any depth, `glob: "/src/**/*.rs"` is a full path, `type: "rust"` keeps `.rs` files, a named single file ignores both filters, and a zero-match search with `glob: "*.{ts,tsx}"` adds the hint line.
- Unit, trust: results are `OutputTrust::Untrusted`, and the bare zero-match result is `OutputTrust::Trusted`.
- Description pin, in `spec-tests.rs`: the built `grep` description contains `Rust regex`, `literal`, `15000`, `16777216`, and `skipped`, and does not contain `ripgrep`, `rg `, `several thousand`, or `Shell`; no placeholder is left unfilled; the `glob` parameter text holds `"*.js"` and `"**/*.tsx"` and no `{`; the `path` text says `absolute`.
- Yield check, in the integration target: seed 1,000 small files through the call context's `Access` and poll the Grep future by hand with a no-op waker, counting `Pending` results. Assert at least 31 `Pending` before it completes with the right result, and that dropping a second copy of the future after its first `Pending` is safe (nothing hangs or panics and the access still works). `TestCall` cannot supply a chosen `VfsRef` or an event sink, so the test counts `Pending` polls and not reads.
- Real directory: one test over a directory mounted with `RealBackend::rooted`. It cannot live in `plugin-std`, so put it in `crates/workshop/server/tests/it/` as a new module (for example `std_real_directory.rs`, declared in `tests/it/main.rs`): that crate already depends on `plugin-std`, `runvil`, `runvil-plugin`, and `tempfile`. Use another existing suite only if this one cannot build, and keep the crate allowed to depend on both `plugin-std` and the vfs crate (`cargo test -p build-xtask` checks it). Build the context directly with `ToolContext::new(&tool, &access, &origin, &services)`, using `VfsRef::builder().mount("/", RealBackend::rooted(dir)?).build()`, `vfs.acquire(Origin::new(...))`, `runvil::effect::ToolCallOrigin` and `ToolCaller::Script`, and `HostServices::new()`, and drive the Pack that `plugin_std::install` inserts. Name the temp directory from the test name, the process id, and a counter. Seed a file whose 5,000 lines are 40 `x` each except line 3,000, which holds a unique word. Then: ListDir on `/` lists the file; Grep for the word returns that file and `3000:` with the line; Read of the file with no arguments returns a first page ending with a trailer that names `offset=2084`; Read with `offset: 2084` starts at `  2084|` (six-wide number, pipe, no space) and continues. This is the acceptance flow: list, find, read a window, read the next window by the trailer offset.
- Regression: the four stub tools still return the stub text unchanged, and every existing `plugin-std` test that this plan did not change passes.

Exit: the whole gate set in Tests passes, including the Workshop crates, both clippy runs, the rustdoc run, `cargo test -p build-xtask`, the wording scan, and the facade check (the `Access` additions from step 1 change the facade listing, which step 1 regenerated). If a gate fails for a reason outside this work, stop and report it instead of editing unrelated files, except for edits this plan makes (`Cargo.lock`, the docs and test updates above).

Commit: one commit holding Grep, `grep.json`, `EDITS.md`, the dispatch, the doc updates, the yield and real-directory tests, and every other test. Suggested subject: `Add a real Grep and a real-directory test for the std file tools`.

</step-4>

</execution-plan>
