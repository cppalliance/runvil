---
name: json global with empty markers
overview: Add an always-on Lua `json` global with strict `json.encode` (plus an opt-in debug mode), `json.decode`, `json.encode_lines` and `json.decode_lines` for JSON Lines, `json.null`, and `json.array()` for lists that must stay lists when empty. `var` and frozen `argv` keep empty lists as lists.
todos:
  - id: marker-storage
    content: Keep mlua's array marker on list data in var and frozen argv storage (sys.rs, argv.rs)
    status: pending
  - id: json-module
    content: "Add the json module (strict and debug encode, encode_lines, decode, decode_lines, json.array, json.null) and install it in the same step: install_engine_globals, RESERVED_NAMES, VISIBLE_GLOBALS, and the lib.rs and prelude.rs docs"
    status: pending
  - id: tests
    content: Add the unit, integration, and regression tests under Testing Plan
    status: pending
  - id: docs
    content: "Update every affected chapter in runvil-docs/src/language: 04, 05, 06, 07, 08, 09, 11, 12, 13, 14, 15, and 17, as listed under Execution Instructions"
    status: pending
  - id: verify
    content: Run the exit criteria under Testing Plan
    status: pending
isProject: false
---

# Lua json global

<product-contract>

## Product Requirements

Prompt authors need to turn JSON text into Lua values and back inside the sandboxed section VM, without loading anything. This plan adds an always-on `json` Engine global with strict encoding, decoding, JSON Lines in both directions, an explicit null, a way to keep empty lists as lists, and an opt-in debug dump. Output and error text are identical on every run, and `var` and frozen `argv` stop turning empty lists into empty objects.

- Problem and users:
  - Section Lua runs with only the `string`, `table`, and `math` libraries plus the safe base functions (`crates/runvil-internal/lua/src/vm.rs`, `SectionVm::new`), so there is no way to parse or produce JSON text. The users are prompt authors and Plugin prelude authors.
  - Tool output such as a `web/fetch` JSON body reaches Lua as text (`runvil-docs` repository, `src/language/13-web-fetch-and-search.md`, around line 742), and store files and model prompts often need JSON. A web tool's text arrives inside the untrusted envelope after a header block, so an author cuts the JSON out with `string.match` before decoding it. Chapter 13 shows how.
- Goals:
  - `json` is available in every section VM with no `require`.
  - Encode and decode JSON, and encode and decode JSON Lines.
  - Authors can put an explicit null in a table and can keep an empty list as `[]`, including when the value goes to `tools.call` arguments, `var`, `item`, or message records.
  - A debug dump never fails on the value.
  - Output and error text are identical on every run.
- Non-goals:
  - Pretty printing, streaming, or schema validation.
  - Any change to the Rust facade crate's public API.
  - Making the other Lua-to-JSON paths as strict as `json.encode`.
- Success criteria:
  - The acceptance criteria under Functional Specification hold, and the exit criteria under Testing Plan pass.
- Constraints:
  - The sandbox gains no new standard library or capability.
  - Engine code may not use wall clocks, sleeps, or `RandomState` hash ordering (`crates/runvil-internal/clippy.toml`, lines 14-31).
  - Workspace lints deny `unwrap`, `expect`, and bare `allow` attributes (`Cargo.toml`, `[workspace.lints.clippy]`).
  - No Rust file may exceed 500 lines (`crates/build-ceiling/src/lib.rs`).
  - Each section VM has a 64 MiB Lua heap by default (`crates/runvil-internal/lua/src/vm.rs`, `DEFAULT_LUA_MEMORY_BYTES`).
- Open questions: None

## Functional Specification

`json` is a table with six members: `encode`, `encode_lines`, `decode`, `decode_lines`, `null`, and `array`. Encoding is strict by default and names the exact location of any value that isn't JSON data. Decoding returns writable tables, marks every list, and represents null with `json.null`. A debug option replaces every encoding failure with a readable string. Every error is a `lua`-kind error value that `pcall` can catch.

- Actors and workflows:
  - A prompt author decodes a fetched JSON body, edits it, and encodes it again.
  - A prompt author builds a list that may end up empty, wraps it with `json.array`, and passes it to `tools.call` or stores it in `var`.
  - A prompt author writes records with `store.append('rows.jsonl', json.encode_lines(rows))` and reads them back with `json.decode_lines` (`store.append` is documented in the `runvil-docs` repository, `src/language/09-the-store.md`, around line 67).
  - A prompt author dumps a table with `json.encode(t, { debug = true })` and writes it to the store or returns it, because `log` accepts at most 256 characters (`crates/runvil-internal/lua/src/engine_globals.rs`, `LUA_LOG_CHARACTER_LIMIT`).
  - A Plugin prelude calls `json` functions.
- Inputs and outputs:
  - `json.encode(value [, options])` returns compact JSON text with no spaces. `options` is a table whose only key is `debug`, a boolean.
  - `json.encode_lines(list [, options])` returns each item encoded by the `json.encode` rules, each followed by `\n`, and an empty list returns `""`. The list may be a marked list, an empty table, a table with keys 1 to n, or a sealed view of one, such as `var.rows`. The newline after the last record lets the result be appended to a file that already ends in a record.
  - `json.decode(text)` returns the Lua value of one JSON document.
  - `json.decode_lines(text)` returns a marked list of every JSON value in `text`, in order.
  - `json.null` is mlua's null value (`lua.null()`), a light userdata, so `type(json.null)` is `userdata`. Decoded nulls are this value, and it encodes as `null`. It is truthy like any userdata, so code tests for it with `v == json.null`, not `if v then`.
  - A marked list is a table that has mlua's array metatable. `json.array()` returns a new empty marked list. `json.array(t)` marks table `t` as a list and returns `t`. A table that is already a marked list, such as a decoded list, is returned unchanged.
  - There is no `json.object`, because `{}` encodes as an object.
- States and validation:
  - Encoding rules:
    - `nil` at the top level and `json.null` encode as `null`.
    - Integers encode as integers. A whole float in the i64 range encodes as an integer, so `2.0` is `2` and `-0.0` is `0`. Other finite floats use the shortest text that reads back exactly. NaN and infinity are errors.
    - Strings and object keys must be valid UTF-8.
    - A marked list must have only the keys 1 to n, where n may be 0, and encodes as a list.
    - An unmarked empty table encodes as `{}`. An unmarked table whose keys are exactly 1 to n encodes as a list. An unmarked table with only string keys encodes as an object. Any other table is an error.
    - A table's key rules are checked before any of its values are encoded, so an error path only ever steps through string and integer keys.
    - A marked list with a key that is not an integer raises `is marked as a list but has a {type} key`, and a marked list with a gap raises `is missing list item [{n}]`. When an unmarked table breaks more than one rule, a key that is neither an integer nor a string is reported first, then a mix of integer and string keys, then a missing list item.
    - For the missing-list-item message, n is the table's count of integer keys, and the reported item is the first of 1 to n that is absent. One always exists when the integer keys are not exactly 1 to n, so a gap, a key of 0, and a negative key are all covered. `{ [1] = 'a', [3] = 'c' }` reports `[2]`, `{ [0] = 'a' }` reports `[1]`, and `{ [-1] = 'a', [1] = 'b' }` reports `[2]`.
    - A sealed view is a table with no entries of its own whose metatable has both `__index` and `__metatable`. This covers `var` tables, frozen `argv`, sealed Plugin globals, and fanout results. If `__index` is a table, that table is encoded in the view's place. If `__index` is a function, as in `sys`, the view cannot be read. If `__index` is any other value, the table is not a sealed view and encodes as its own entries.
    - Functions, threads, and any userdata other than `json.null` are errors. That includes the message-record views that `msgs[i]` returns, even though mlua can serialize them (`crates/runvil-internal/lua/src/messages-view.rs`), because encoding serializable userdata is deferred. Other metatables are ignored, and the table's own entries are encoded.
    - A table that contains itself is an error, while the same table in two places is allowed. Nesting more than 127 tables deep is an error, the same limit `json.decode` has. The top-level table is level 1, and reading through a sealed view to its data table does not add a level.
    - Cycle detection keeps the tables on the current path from the top-level value and compares each new table to them by identity. Addresses are compared but never printed. The cycle check runs before the depth check at each table, so a loop of at most 127 tables gets the cycle message, even when its repeated table falls at level 128. A longer loop reaches the depth limit before it repeats, so it gets the depth message. A sealed view is checked by the data table it reads through, and `json.encode_lines` keeps the list on the path while it encodes the items. A key that is itself a table is never stepped into, so it cannot form a cycle.
    - Keys are visited in a fixed sorted order, so the first error found is the same on every run: booleans, then numbers by value, then strings bytewise, the order `collection::sort_key` defines. A string key that is not valid UTF-8 sorts bytewise among the other strings, and an infinite number key sorts by value among the other numbers, so `-math.huge` comes first and `math.huge` last. Keys whose order is not stable between runs (tables, functions, userdata, and threads) come last, ranked by type name, so a table with several such keys always reports the same one. Object keys in the output are sorted.
    - Compact output never contains a raw newline, because a newline inside a string is written as `\n`, so every encoded value is one line.
  - Debug mode follows the encoding rules but writes a string in place of each failure:
    - A function becomes `"[function]"`, a thread becomes `"[thread]"`, and every userdata other than `json.null` becomes `"[userdata]"`, whether or not it defines a `__tostring`.
    - A table that contains itself becomes `"[cycle]"`, and a table more than 127 levels deep becomes `"[too deep]"`.
    - NaN and infinity become `"NaN"`, `"Infinity"`, and `"-Infinity"`. Invalid UTF-8 is replaced with replacement characters.
    - Any table that breaks the key rules, marked or not, becomes an object. A number key becomes the text it would encode as, such as `2`, `2.5`, or `1e300`, with infinite keys as `Infinity` and `-Infinity`. Boolean keys become `true` and `false`. When two keys produce the same text, the later one in visiting order wins. Table, function, userdata, and thread keys are left out, because their order is not stable between runs.
    - A sealed view with an `__index` function becomes `"[cannot read: <label>]"`, where `<label>` is its `__metatable` string, or `"[cannot read]"` when `__metatable` is not a string.
    - Option errors and `json.encode_lines` argument errors still raise, including a list argument that is a sealed view that cannot be read. The output never contains a memory address.
  - Decoding rules:
    - Input must be a string of at most 8,388,608 bytes, the same cap the web fetch tool applies to JSON bodies (`runvil-docs` repository, `src/language/13-web-fetch-and-search.md`, around line 921).
    - Nesting more than 127 arrays and objects deep is an error. This is serde_json's own limit, so the message is the parser's, as in `json.decode: recursion limit exceeded at line 1 column 128`.
    - Every decoded list is a writable marked list, so a decoded list emptied with `table.remove` still encodes as `[]`. Objects are plain writable tables, and JSON null is `json.null`.
    - Integers in the i64 range become Lua integers, and all other numbers become floats. An integer above the i64 range, such as `9223372036854775808`, becomes a float, because mlua converts a `u64` it cannot fit into an integer to a float rather than raising (mlua 0.12.0, `src/conversion.rs`, `lua_convert_int`).
    - `json.decode_lines` treats blank lines and `\r\n` as whitespace, and it also accepts several values on one line or one value across several lines.
  - Marked lists from `json.array` or `json.decode` return `false` from `getmetatable` and refuse `setmetatable`, because mlua's array metatable sets `__metatable` to `false` (mlua 0.12.0, `src/serde/mod.rs`, `init_metatables`). Lists in tool results already behave this way, because they convert through the same mlua function (`crates/runvil-internal/lua/src/protocol/render.rs`, around line 142).
  - An empty list stored in `var` or in frozen `argv` reads back as `[]` instead of `{}`.
- Errors and recovery:
  - Every error is raised as a `lua`-kind error value that `pcall` catches. `{path}` starts at `value` and adds `.name` for a string key and `[n]` for any other key, as in `value.items[3]`, which is the format `key_path` in `crates/runvil-internal/lua/src/proxy.rs` already produces.
  - `{type}` is mlua's type name (`Value::type_name`), as in every other Engine argument error, such as `tools.get takes a tool id, got integer`. So an integer reads `integer`, a float `number`, and `json.null` `lightuserdata`, as in `json.decode expects a string, got integer`.
  - Each function checks its arguments, options included, before it encodes or decodes anything, so a bad option is reported even when the value is also bad.
  - `json.encode: {path} must be JSON data, got {type}`
  - `json.encode: {path} is not a finite number`
  - `json.encode: {path} is not valid UTF-8`
  - `json.encode: {path} mixes list items and named keys`
  - `json.encode: {path} is missing list item [{n}]`
  - `json.encode: {path} has a {type} key; object keys must be strings`
  - `json.encode: {path} is marked as a list but has a {type} key`
  - `json.encode: {path} contains itself`
  - `json.encode: {path} is nested more than 127 levels deep`
  - `json.encode: {path} cannot be read ({label}); copy the fields you need into a plain table`. The parenthetical appears only when `__metatable` is a string.
  - `json.encode: options must be a table, got {type}`, `json.encode: unknown option {key}`, and `json.encode: option debug must be a boolean, got {type}`, with `{key}` rendered by `key_text` (`crates/runvil-internal/lua/src/proxy.rs`). `json.encode_lines` raises the same three with the `json.encode_lines:` prefix.
  - `json.encode_lines expects a list, got {type}` for a value that is not a table, and `json.encode_lines expects a list, got a table with a {type} key` for a table with no list items, naming the first key in visiting order, such as `got a table with a string key` for an object. A table with list items that is still not a list raises the list messages above with the `json.encode_lines:` prefix. A list argument that is a sealed view that cannot be read raises the cannot-read message with the `json.encode_lines:` prefix, as in `json.encode_lines: value cannot be read (sys is sealed); copy the fields you need into a plain table`. An item error names the item, as in `json.encode_lines: value[2].name must be JSON data, got function`.
  - `json.decode expects a string, got {type}`, `json.decode: input exceeds the 8388608-byte limit`, and `json.decode: {parser message}`, such as `json.decode: expected value at line 1 column 1`
  - The same three messages for `json.decode_lines`, with line and column counted across the whole text.
  - `json.array expects a table or no argument, got {type}`, and `json.array: the table already has a metatable` for a table whose metatable is anything other than the list marker.
  - A Lua memory-limit error raised while building a result passes through unchanged, without a `json.*` prefix, so it keeps the behavior every other memory-limit error has.
- Security and privacy behavior:
  - Decoded strings are plain Lua strings, not wrapped in the run's untrusted envelope. An author who sends decoded model or web text to a model wraps it with `untrusted`.
  - The byte cap and both depth limits bound the work a single call does. Nothing new reaches files, the network, or code loading.
- Acceptance criteria:
  - `json.encode({})` is `{}`, `json.encode(json.array())` is `[]`, and `json.encode(json.decode('[]'))` is `[]`.
  - `json.encode({ b = 1, a = 2 })` is `{"a":2,"b":1}`.
  - `json.decode('[1,null,3]')` has 3 items, and the second is `json.null`.
  - A `tools.call` argument table containing `json.array()` sends `[]`.
  - After `var.items = json.array()`, `var` reads back as `{"items":[]}`.
  - `json.encode(var.items)` and `json.encode(argv)` encode the stored data, and `json.encode(sys)` raises the cannot-read error.
  - The same invalid table, built in two different key orders, raises the same message.
  - `json.decode_lines(json.encode_lines(rows))` equals `rows` for a list of objects.
  - `json.encode_lines({ { a = 1 }, { a = 2 } })` is `{"a":1}` and `{"a":2}`, each followed by a newline.
  - `json.encode({ f = function() end }, { debug = true })` is `{"f":"[function]"}`.
  - `json.array(t)` on a decoded list returns that same table.
  - A Plugin prelude can call `json.encode`.

</product-contract>
<implementation-contract>

## Technical Design

`json` is an Engine global installed by the shared section VM setup, so the H1 pass, walked sections, fanout arms, and spawned tasks all get it before Plugin preludes run. Encoding uses a dedicated walker that produces a `serde_json::Value`, because mlua's own conversion silently drops keys from mixed tables and reports no paths. Decoding parses with serde_json and converts with mlua's `to_value`, the conversion tool results already use. The list marker is mlua's array metatable, which every existing Lua-to-JSON path already honors, and `var` and frozen `argv` storage start keeping it.

- Architecture:
  - Every section VM runs `SectionVm::install_engine_globals`, then `ui`, `item`, the control globals, the suspending calls, `models.loop`, Plugin preludes, `require`, and the store's suspending calls, in that order (`crates/runvil-internal/engine/src/execute/section_vm.rs`, `install_after_injection`). `json` installs inside `install_engine_globals` (`crates/runvil-internal/lua/src/vm/install.rs`), after `log` and `store`.
  - Plugin preludes see only the names in `VISIBLE_GLOBALS` (`crates/runvil-internal/lua/src/prelude.rs`), which gains `json`.
  - Every existing Lua-to-JSON path calls mlua's `from_value` with default options: `var` writes and read-back (`crates/runvil-internal/lua/src/sys.rs`), prose globals and `argv` (`crates/runvil-internal/lua/src/vm/state.rs`, `crates/runvil-internal/lua/src/prose.rs`), tool-call arguments (`crates/runvil-internal/lua/src/protocol/parse/tools.rs`), task requests (`crates/runvil-internal/lua/src/protocol/parse/tasks.rs`), message records (`crates/runvil-internal/lua/src/messages.rs`), and fanout collections (`crates/runvil-internal/lua/src/collection.rs`). With default options, mlua encodes a table with its array metatable as a list, and encodes any other table with a positive length as a list of that length, dropping its other keys (mlua 0.12.0, `src/table.rs`, `encode_as_array`).
  - `var` tables are empty proxies whose `__index` is a hidden data table and whose `__metatable` is `var is guarded` (`crates/runvil-internal/lua/src/sys.rs`, `guarded_table`). Frozen `argv` tables and sealed Plugin globals are `read_only_proxy` tables over a data table (`crates/runvil-internal/lua/src/proxy.rs`, `crates/runvil-internal/lua/src/argv.rs`, `crates/runvil-internal/lua/src/prelude.rs`). Fanout results keep their fields in a hidden `__index` table behind a decoy `__metatable` (`crates/runvil-internal/lua/src/__impl_fanout.lua`, `fanout_result`). `sys` uses a function `__index` (`crates/runvil-internal/lua/src/sys.rs`, `seal_sys`). The docs already say `pairs`, `#`, and `next` see nothing in `var` tables, and `pairs` and `next` see nothing in `sys` (`runvil-docs` repository, `src/language/05-lua-environment.md`, lines 1124-1129 and 1356).
  - Lua's hash traversal order can differ between VMs, which is why the VM replaces `pairs` and `next` with a sorted walk (`crates/runvil-internal/lua/src/iteration.rs`, module docs). The encoder visits keys in that same order through `collection::sort_key`. `sort_key` rejects a non-UTF-8 string key (`KeyError::NotUtf8`) and an infinite number key (`KeyError::NotFinite`) as well as table, function, userdata, and thread keys (`crates/runvil-internal/lua/src/collection-order.rs`). For the first two, the encoder builds `SortKey::Text` from the string's bytes and `SortKey::Float` from the infinity itself, which `SortKey::compare` already orders correctly. Only the last four go in the unstable group.
- Modules and interfaces:
  - A new `json` module in `runvil-lua` exposes one crate-internal installer, which `install_engine_globals` calls. It reuses `key_path` (`crates/runvil-internal/lua/src/proxy.rs`) for error paths, `sort_key` (`crates/runvil-internal/lua/src/collection.rs`) for key order, and `crate::Error::Lua` raised through `mlua::Error::external`, as `crates/runvil-internal/lua/src/messages.rs` raises its refusals.
  - `json.decode_lines` runs serde_json's stream deserializer over the whole text, so error positions count lines and columns across the whole input, and any whitespace, newlines included, may separate values.
  - The workspace pins mlua 0.12.0 with the `lua55`, `vendored`, `serialize`, and `send` features, and serde_json 1.0.151 with `float_roundtrip` (`Cargo.toml`, `[workspace.dependencies]`, and `Cargo.lock`). Both crates are already dependencies of `runvil-lua` (`crates/runvil-internal/lua/Cargo.toml`).
  - The `json` table is a plain table, like `string`. Null and list detection rely on the values themselves, not on fields of the `json` table, so reassigning a field cannot break encoding.
  - The encoder never calls back into Lua. It reads with `raw_get`, `raw_len`, `Table::pairs` (which skips `__pairs`), and `Table::metatable` (which ignores `__metatable`), and compares tables with `==` or `Table::to_pointer`, never `Table::equals`, which runs `__eq` (mlua 0.12.0, `src/table.rs`, `equals`). mlua's own `has_array_metatable` is private, so a marked list is detected as `t.metatable() == Some(lua.array_metatable())`. The cycle path is a `Vec` of table pointers, at most 127 long.
  - `json.null` is `Value::LightUserData` with a null pointer (`LuaSerdeExt::null`). Any other light userdata is not JSON data.
- File and public API changes:
  - In the `runvil` repository, a new `crates/runvil-internal/lua/src/json.rs` with child modules `json-encode.rs` and `json-decode.rs`, declared with `#[path]` as `crates/runvil-internal/lua/src/engine_globals-store.rs` is. If `json-encode.rs` nears 500 lines, the key classification and ordering move to a third child. Because the repository's flat-directory rule (`AGENTS.md`, Structural Rules) turns a three-file sibling group into a subdirectory, the module then becomes `json.rs` with `json/encode.rs`, `json/decode.rs`, and `json/keys.rs` in standard module layout, with no `#[path]` attributes.
  - `crates/runvil-internal/lua/src/vm/install.rs`: install `json` and update the `install_engine_globals` doc comment.
  - `crates/runvil-internal/lua/src/globals.rs`: `RESERVED_NAMES` gains `("json", Reserved::EngineGlobal)` between `item` and `jump`, growing from 61 to 62 entries.
  - `crates/runvil-internal/lua/src/prelude.rs`: `VISIBLE_GLOBALS` gains `"json"` after `"untrusted"`, growing from 19 to 20 entries. The module docs and the `environment` doc comment, which list what a prelude sees, gain `json`.
  - `crates/runvil-internal/lua/src/sys.rs`: `guarded_json_value` sets the array metatable on the data table of a JSON array, and `materialize_guarded_value` copies it onto the plain copy. Prose substitution reads `var` through `var_to_json` (`crates/runvil-internal/lua/src/prose.rs`), so `{{ var.items }}` for an empty list also renders `[]` instead of `{}`.
  - `crates/runvil-internal/lua/src/argv.rs`: `frozen_json_value` sets the array metatable on the data table of a JSON array.
  - `crates/runvil-internal/lua/src/lib.rs`: declare the module and mention `json` in the crate docs.
  - The Rust facade is unchanged, so `crates/runvil/public-api.txt` stays as it is.
  - In the `runvil-docs` repository, every chapter under `src/language/` that the change makes wrong or incomplete, as listed under Execution Instructions: `04`, `05`, `06`, `07`, `08`, `09`, `11`, `12`, `13`, `14`, `15`, and `17`. Chapters `01`, `02`, `03`, `10`, and `16` were checked and need no change.
- Data, persistence, failure, security, and privacy constraints:
  - The run reads `var` back as JSON at fixed points (`runvil-docs` repository, `src/language/05-lua-environment.md`, "When the run reads `var` back"). Empty lists in that JSON change from `{}` to `[]`, including empty lists that came from tool results.
  - Decoding builds a serde_json tree on the Rust heap, outside the Lua memory limit, before converting it to Lua values. Encoding also builds one before writing text.
  - Sorted output relies on serde_json's map being a `BTreeMap`. No crate in `Cargo.lock` enables serde_json's `preserve_order` feature, since serde_json's dependencies there are `itoa`, `memchr`, `serde`, `serde_core`, and `zmij`.
  - serde_json's parser starts its depth counter at 128, subtracts one for each array or object it opens, the top level included, and fails when the counter reaches 0 (serde_json 1.0.151, `src/de.rs`, `check_recursion`). So it accepts 127 levels and rejects the 128th, which is why both limits are 127.

</implementation-contract>
<verification-contract>

## Testing Plan

Unit tests in `runvil-lua` cover every rule and every message under Functional Specification, plus the `var`, `argv`, and Plugin prelude changes. The existing engine, parser, facade, and Harness suites confirm the new global is reserved and that nothing else moved. Determinism has its own checks, because output and error text must not depend on the order a table was built in. The work is done when the listed commands pass with warnings denied.

- Unit:
  - A new `crates/runvil-internal/lua/src/tests/json.rs`, declared in `crates/runvil-internal/lua/src/tests.rs`, with `tests/json/<topic>.rs` children from the start (such as `encode`, `errors`, `debug`, `decode`, `lines`, and `views`), because the cases below will not fit in 500 lines. The children are declared the way `crates/runvil-internal/lua/src/tests/require.rs` declares `tests/require/prose.rs`.
  - Round trips of scalars, nested data, and non-ASCII text.
  - `{}`, `json.array()`, `json.array(t)`, a decoded `[]`, a decoded list emptied with `table.remove`, and writes to decoded values.
  - `json.null` in objects and lists, including `[1,null,3]` keeping 3 items.
  - Numbers: `2.0`, `-0.0`, `1.5`, `2^63` (stays a float), `-2^63` (becomes an integer), NaN, infinity, and decoding `9223372036854775808` to a float (`math.type` reads `float`).
  - Every strict error message, the precedence between key rules, and a table with both a table key and a function key always reporting the function key.
  - Key ordering: a non-UTF-8 string key and an infinite number key report the same error however the table was built, and debug mode orders them the same way every time.
  - The missing-list-item rule for a gap, a key of 0, and a negative key, as in the examples under Encoding rules.
  - Key rules before values: `{ [1] = function() end, a = 1 }` reports the mix of list items and named keys, not the function.
  - Type names: `json.decode(5)` reports `got integer`, `json.decode(1.5)` reports `got number`, and `json.array(json.null)` reports `got lightuserdata`.
  - An options error is reported before a value error, as in `json.encode(function() end, 5)`.
  - A loop of 127 tables whose repeat falls at level 128 getting the cycle message rather than the depth message, a self-containing table at the top, a shared table in two places, a list containing itself in `json.encode_lines`, and 127 levels of nesting passing and 128 failing on both encode and decode. Also a `var` value 127 levels deep encoding, because reading through each view to its data table adds no level.
  - The byte cap, and a string containing `\n` encoding on one line.
  - `json.decode_lines` with blank lines, `\r\n`, and an error on line 3.
  - `json.encode_lines` with a list of objects, an empty list, `var.rows`, an object reporting `got a table with a string key`, a mixed table, `sys` raising the cannot-read message with the `json.encode_lines:` prefix, an option error with that prefix, an argument error still raising in debug mode, an item error naming `value[2]`, debug mode, and a round trip through `json.decode_lines`.
  - Sealed views: `var` tables, `var` itself, frozen `argv`, a sealed Plugin global, and fanout results read through, and `sys` raises.
  - Debug mode for each replaced case, including a userdata that defines `__tostring` still rendering as `"[userdata]"`, number keys rendering as `2`, `2.5`, `1e300`, and `Infinity`, and a key `1` colliding with a key `"1"`, where the string key wins because it is visited later.
  - `json.encode(msgs[1])` on a message-record view raises the userdata error, and debug mode writes `"[userdata]"`.
  - `pcall` reports kind `lua`.
  - `json.array` errors, including on a `var` table, and `json.array` returning a decoded list unchanged.
  - A memory-limit error during `json.decode`, under a small Lua memory limit, passes through without a `json.decode:` prefix.
  - `crates/runvil-internal/lua/src/tests/sys_and_var.rs`: `var.items = json.array()` and an empty list from `json.decode` read back as `[]`, and `var.items = {}` still reads back as `{}`.
  - `crates/runvil-internal/lua/src/tests/argv.rs`: an empty list in frozen `argv` encodes as `[]`.
  - `crates/runvil-internal/lua/src/prelude-tests.rs`: a prelude can call `json.encode`.
  - `json.null` gets past every guard that refuses userdata, because those guards match only full userdata (`Value::UserData`) and `json.null` is light userdata. Each case is pinned, because the docs state each: `var.x = json.null` reads back as `null` and reads in Lua as `json.null` (`sys.rs`); an H1 `argv = { a = json.null }` passes the freeze, and `argv.a` reads nil in later sections (`vm/state.rs`, `argv.rs`); `tasks.spawn` with `item = { a = json.null }` (`protocol/parse/tasks.rs`); a fanout member that is `json.null` (`collection.rs`), with an Engine execute test in `crates/runvil-internal/engine/src/execute/tests/suite/exec_flow/fanout_collections.rs` pinning what the arm's `item` reads, since a JSON null seed may install as `json.null` or as no `item` at all; and a prose bare global holding `json.null` (`prose.rs`), whose outcome, likely the `NullValue` substitution failure, the test pins before the docs describe it.
  - The chapter 13 example: wrapping a `web/fetch` header block and a JSON body with `untrusted`, then running the docs' `string.match` pattern and `json.decode`, recovers the body's data. Also, a body string containing `<` decodes to a string holding `&lt;`, the caveat the docs state.
- Integration and end-to-end:
  - `every_global_section_setup_leaves_in_g_is_a_reserved_name` (`crates/runvil-internal/engine/src/lua/tests/globals.rs`) compares `RESERVED_NAMES` with a fully set-up section VM's globals in both directions, so it fails until `json` is both installed and listed.
  - The `RESERVED_NAMES` checks in `crates/runvil-internal/lua/src/globals-tests.rs` and `crates/runvil-internal/parser/src/contract/tests.rs` still pass.
  - A `tools.call` argument table containing `json.array()` parses to an empty JSON array, tested beside the existing tool-call parse tests in `crates/runvil-internal/lua/src/protocol/tests/parse_tool_call.rs`.
- Regression, security, and performance:
  - The same invalid table built in two key orders raises the same message, and `{ b = 1, a = 2 }` encodes with sorted keys, which also guards against a future `preserve_order` change.
  - No existing test asserts that an empty `var` or `argv` list reads back as `{}`. A search of the Engine, Lua, parser, and Harness crates and their fixtures found none, so no existing assertion changes. If a Harness run turns one up, it is updated to expect `[]`, because that is the intended behavior change.
  - The byte cap and both depth limits are tested at their boundaries: 8,388,608 bytes passes and 8,388,609 fails, and 127 levels pass and 128 fail.
- Exit criteria (the CI gates in `.github/workflows/ci.yml` that this change can affect):
  - `cargo fmt --all --check` passes.
  - `cargo test -p runvil-lua -p runvil-engine -p runvil-parser -p runvil -p harness-runner -p harness` passes. The Harness suites are included because the `var` change reaches every caller that reads `var` back. The crates' build scripts enforce the 500-line file limit during those builds, test files included.
  - `cargo clippy -p runvil-lua -p runvil-engine --all-targets --all-features -- -D warnings` passes. `--all-features` matches CI and builds runvil-lua's `test-support` feature.
  - With `RUSTDOCFLAGS=-D warnings`, `cargo doc -p runvil-lua -p runvil-engine --no-deps --all-features` and `cargo doc --no-deps --all-features -p runvil-engine --document-private-items` pass.
  - `cargo +nightly-2026-09-05 xtask api --check` passes. A plain `cargo xtask api` only prints its findings, and it refuses to run on any toolchain except the nightly pinned in `crates/build-xtask/src/api/toolchain.rs`.
  - `cargo +1.95 build -p runvil-lua -p runvil-engine` passes, the `rust-version` floor in the root `Cargo.toml`.
  - With `RUNVIL_DOCS` set to the `runvil-docs` root, `cargo xtask site --books-only` passes. It checks that every link names a file that exists, but not `#` anchors, so every new anchor is also checked by reading it against its heading.

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - `json` is always available as an Engine global instead of being loaded with `require`. `require` loads only sections of the running prompt, never files or shipped code (`crates/runvil-internal/lua/src/require.rs`, module docs), and JSON helpers are pure functions like the other always-on globals. User: "Always-on makes sense."
  - JSON null is a sentinel, `json.null`, so `{ a = null }` keeps its key. It is mlua's existing null value, which every `from_value` path already reads as null. User chose "A sentinel json.null that keeps the key".
  - An unmarked empty table encodes as an object, and a sequence encodes as a list. User chose "Rule: empty means object, sequence means array, plus json.array for empty arrays".
  - Whole floats encode as integers, and NaN and infinity raise. User chose "Whole floats as integers, NaN and infinity raise an error".
  - Decoded strings stay plain, and authors wrap them with `untrusted` when needed. User chose "No, plain strings; the author wraps with untrusted when needed".
  - Decoded `[]` and `{}` are fresh writable tables. User chose "Fresh mutable tables, so decoded data is always writable".
  - `json.array()` returns a new empty list, `json.array(t)` marks an existing table, and there is no `json.object`. A plain `{}` already means an empty object, and a function lets authors build a list that may end up empty. User, after asking "what do we lose if we get rid of json.array and json.object?": "fine. do that".
  - `var` and frozen `argv` keep the list marker, so empty lists read back as `[]`. User chose "Yes, fix it here: var (and frozen argv) keep the array marker".
  - The list marker is mlua's array metatable, and decoding uses mlua's `to_value`, so marked lists work in `tools.call` arguments, `item`, message records, and `var`. Accepted during plan review. User: "fine. do that".
  - The encoder reads through sealed views and raises a clear error for views it cannot list, because otherwise `json.encode(var.items)` would silently produce `{}`. Accepted during plan review. User: "fine. do that".
  - The encoder visits keys in sorted order, so error text does not change between runs. Accepted during plan review. User: "fine. do that".
  - Plugin preludes can use `json`. Accepted during plan review. User: "fine. do that".
  - JSON Lines is supported in both directions through `json.encode_lines` and `json.decode_lines`. User: "what about JSONL" and "but what if I have an array and I want to output that as JSONL".
  - Debug placeholders exist for dumping values, and they are opt-in through `{ debug = true }`, so a value that isn't JSON data fails where it is encoded instead of reaching a file, model, or tool as `"[function]"`. User: "this would actually be helpful because you could use it to debug" and "opt-in seems fine to me".
  - In debug mode every userdata other than `json.null` renders as the fixed text `"[userdata]"`. No userdata type has to define how it prints, and the encoder never calls back into Lua. User: "my purpose for rendering userdata as '[userdata]' was to make it easier, I'm not sure I like the idea of a global registration for userdatas".
  - Encoding is compact, the decode input cap is 8,388,608 bytes, mixed tables raise, and errors are `lua`-kind error values. These were proposed as defaults, and the user raised no objection.
  - Both depth limits are 127, because serde_json rejects the 128th nested array or object and the encoder should never produce text the decoder rejects. User chose "127 on both encode and decode, boundary tests 127 pass / 128 fail" during plan review.
  - `{type}` in error messages is mlua's type name, so integers read `integer` and `json.null` reads `lightuserdata`, as in every other Engine argument error. User chose "mlua's type_name like every other Engine error" during plan review.
  - A non-UTF-8 string key sorts bytewise among the strings, and an infinite number key sorts by value among the numbers, so only reference-type keys have an unstable order. Applied during plan review.
  - A table's key rules are checked before its values, and the missing list item is the first of 1 to n absent, with n the count of integer keys. Applied during plan review.
  - The docs update covers every chapter under `runvil-docs/src/language/` that the change makes wrong or incomplete, not only `05`, `12`, and `17`. Each statement is fixed where a reader meets it, and pointers to `json` are added only where a reader is about to need it. User: "I want runvil-docs/src/language updated too".
  - Chapter 13 shows a worked example that cuts a `web/fetch` JSON body out of the untrusted envelope and decodes it, with the escaping and re-wrap caveats. User chose "Show a short example that cuts the JSON out of the envelope and decodes it, with the escaping and re-wrap caveats".
  - The `runvil-docs` chapter updates are this plan's last work item, but they run by hand in the `runvil-docs` repository after the Vibe Coder run closes, outside its review and commit cycle, because the tool reviews and commits in one repository. User chose "Keep the docs as this plan's last step, committed by hand in runvil-docs outside the review and commit cycle".
  - The code work is two commits, on the Vibe Coder's Bounded path. Step 1 adds decoding, `json.null`, `json.array`, the install, and the `var` list marker, all observable without an encoder. Step 2 adds the encoder and the frozen `argv` list marker, which only the encoder's read-through can observe. One commit would put the encoder's many rules and the decoder in a single review. Splitting debug mode out would cost a third cycle and rework the same walker. User: "keep the number of steps as small as practical. balance risk vs steps use your best judgement".
  - Cycle detection follows the current path and compares tables by identity, so a shared table is allowed, and it runs before the depth check, so a loop of at most 127 tables gets the cycle message. User asked "how will cycle detection work", then "apply the fixes". A loop longer than the depth limit reaches the limit before it repeats, which plan review found and recorded.
  - `json.array` returns an already-marked table unchanged, so a decoded list can be passed to it. User: "apply the fixes".
  - Key errors follow a fixed precedence, and keys whose order is unstable are ranked by type name, so the same table always reports the same error. User: "apply the fixes".
  - `json.encode_lines` reports an object, and a marked list reports a non-integer key, with messages of their own, because the general list messages would describe those tables wrongly. User: "apply the fixes".
  - Memory-limit errors pass through unchanged instead of gaining a `json.*` prefix. User: "apply the fixes".
- Rejected alternatives:
  - A JSON library loaded with `require`: `require` cannot load shipped code, so every prompt would carry its own copy. Revisit if `require` gains shipped libraries.
  - A pure-Lua JSON library in a Plugin prelude: slower, re-run in every section VM, and opt-in through frontmatter. Revisit if the Engine must not grow new globals.
  - JSON through a Plugin tool: every call would be an effect, and tool results arrive as text. Revisit only if parsing must leave the VM.
  - JSON null as plain `nil`: it silently drops keys and list items. No revisit condition.
  - `json.array` and `json.object` as shared read-only values: as proxy tables, every other Lua-to-JSON path would see `{}`, and as userdata, `type()` would change and `var` and `item` would refuse them at the top level. In both forms authors could not append to them. Revisit if authors need an identity test for an empty list.
  - `json.object`: `{}` already encodes as an object. Revisit if the empty-table rule changes.
  - A private list marker: no other Lua-to-JSON path would honor it. No revisit condition.
  - mlua's `from_value` as the encoder: it silently drops keys from mixed tables and reports no paths. Revisit if mlua gains a strict mode with paths.
  - Debug placeholders as the default: a placeholder string that reaches a file, model, or tool is harder to trace than an error at the encoding line, and `json.decode` cannot tell it from real text. Revisit if authors routinely reach for `{ debug = true }` outside debugging.
  - Rendering a userdata with its own `__tostring` text in debug mode: it would show more detail, but each userdata type would effectively have to define how it prints, and the encoder would call back into Lua mid-encode. Revisit if debug dumps need to identify Engine objects such as tool or model handles.
  - Wrapping decoded strings in the untrusted envelope: rejected by the user's choice. Revisit if decoded model or web text is found reaching models unwrapped.
  - Keeping Lua's float form, such as `2.0`: rejected by the user's choice. No revisit condition.
  - Keeping a depth limit of 128 by enabling serde_json's `unbounded_depth` feature and tracking depth in the decoder: a workspace-wide feature change and new code for a one-level difference. Revisit if the decoder starts building Lua values straight from the parser, which would track depth anyway.
  - Lua's `type()` names, such as `number` and `userdata`, in error messages: they read better to authors, but `json` would be the only Engine global whose errors differ from the rest. Revisit if the Engine moves every argument error to Lua's names.
- Assumptions, risks, and notes:
  - mlua converts an integer above the i64 range to a float rather than raising (mlua 0.12.0, `src/conversion.rs`, `lua_convert_int`). This was verified during plan review, and a test pins it.
  - The serde_json tree on the Rust heap can briefly use many times the input size when the data holds many tiny items, roughly 100 MiB for an 8 MiB decode input. Encoding has the same kind of cost, bounded by the Lua heap that holds the input. These are estimates, not measurements.
  - `json` becomes a reserved name, so a Plugin prelude can no longer define a global named `json`. A search of the `runvil` and `runvil-docs` repositories found no Lua code that uses a `json` global today.
  - Message-record views are the only serializable userdata in `runvil-lua` today (`crates/runvil-internal/lua/src/messages-view.rs`). Every other Engine object exposed to Lua is created as plain userdata: message lists (`crates/runvil-internal/lua/src/messages.rs`), tool objects (`crates/runvil-internal/lua/src/tools/objects.rs`), Plugin objects (`crates/runvil-internal/lua/src/plugins.rs`), and chat results (`crates/runvil-internal/lua/src/protocol/render.rs`).
  - `json.encode` is stricter than the other Lua-to-JSON paths, which encode a mixed table as a list and drop its named keys.
  - An untrusted tool's JSON reaches Lua already escaped by the envelope: every `<` becomes `&lt;`, chat-template markup such as `[INST]` gets a space, and copies of the nonce are broken (`runvil-docs` repository, `src/language/12-tools.md`, "What the envelope escapes"). None of these can occur in JSON's own syntax outside a string, so the cut-out body still parses, but its string values can differ from what the server sent. Chapter 13 says so.
  - The docs' cut-and-decode example relies on the envelope's shape, which the docs call a contract you can rely on (`src/language/09-the-store.md`, "The envelope shape"), and on the `web/fetch` header block ending in a blank line (`src/language/13-web-fetch-and-search.md`, around line 720).
  - A Plugin prelude's read-only view of `var` uses a function `__index` (`crates/runvil-internal/lua/src/prelude.rs`, `read_only_var`), so `json.encode(var)` inside a prelude function raises the cannot-read error.
  - The `runvil-docs` repository has no book configuration of its own. `cargo xtask site` in the `runvil` repository stages and builds it (`crates/build-xtask/src/site.rs`), and its link check covers files but not `#` anchors, so anchors are checked by reading them.
  - Line numbers cited in the `runvil` and `runvil-docs` repositories were current when this plan was written. If one has moved, find the quoted text or named item instead.

### Deferred and Out of Scope

- Deferred: building Lua values straight from the parser to avoid the Rust-heap tree. Revisit if decode memory becomes a problem.
- Deferred: a pretty-printing option. Revisit when an author asks for readable dumps.
- Deferred: making `tools.call` arguments, `item`, and message records as strict as `json.encode`. Revisit if silently dropped keys cause a reported bug.
- Deferred: reading function-backed views such as `sys`. Revisit if authors need to encode `sys` whole.
- Deferred: an Engine way to get an untrusted tool's text, or its decoded JSON, without cutting it out of the envelope by hand and without the envelope's escaping. Revisit if authors routinely decode web JSON, or if the escaped `&lt;` in decoded strings causes a reported bug.
- Deferred: encoding userdata that mlua can already serialize, such as message-record views, as its JSON form. Until then `json.encode(msgs[i])` raises the userdata error. Revisit together with the next item. User: "yeah lets leave the current plan alone and we can worry about this other thing later".
- Deferred: making every Rust object exposed to Lua serializable by default, enforced by banning `mlua::Lua::create_userdata` in `crates/runvil-internal/clippy.toml` with a written reason required for each exception, so a model can be shown objects such as installed Plugins in a turn. Each object's JSON form would be a deliberate, documented shape, and Plugin and tool objects would refer to each other by name to avoid endless recursion. Revisit after the `json` global ships.
- Out of scope: any change to the Rust facade crate, including re-exporting serde_json types.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p gateway`, the workspace's only default member. Crate build scripts bundle the UIs with esbuild, so `crates/workshop/node_modules` and `crates/gateway/config-ui/ui/node_modules` must exist; both are present on this machine, and CI creates them with `npm ci --prefix crates/workshop` and `npm ci --prefix crates/gateway/config-ui/ui`. The Engine crates also build alone: `cargo build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`, which CI repeats at the `rust-version` floor as `cargo +1.95 build ...`.
- Focused test command pattern: `cargo nextest run --locked -p <package> --all-features <test-name-filter>`, where the filter matches the test's full module path, such as `tests::json` or `json` in `runvil-lua`. Use `-E 'test(<name>)'` for a nextest filterset. Integration test binaries are named `it` in most crates and `suite` in `runvil` and `harness`, so `-E 'binary(suite) & test(<name>)'` narrows to one. Drop `--all-features` for `workshop`, `workshop-server`, and `workshop-server-api`. cargo-nextest 0.9.128 is installed.
- Component test command pattern: `cargo nextest run --locked -p <package> --all-features`, without `--all-features` for `workshop`, `workshop-server`, and `workshop-server-api`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`. On the pinned nightly, CI also runs `cargo nextest run --locked -p build-xtask --run-ignored only`.
- Linter command: `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `CARGO_BUILD_WARNINGS=deny cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`. In Windows PowerShell 5.1, run `$env:CARGO_BUILD_WARNINGS='deny'` as its own statement first. Companion gates, each a yes-or-no check:
  - `cargo test -p build-xtask` for the product and container boundaries, the Engine dependency and clippy-config guards, the doctest ban, the `test-support` leak guard, and the facade shape.
  - `node --test crates/workshop/ui/test/docs-claims.mjs`, the wording scan that keeps Engine crate text from naming the Host.
  - `cargo +nightly-2026-09-05 xtask api --check` for the facade surface against `crates/runvil/public-api.txt`. Run it with `CARGO_BUILD_WARNINGS` unset (`Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue`), because the nightly cargo turns its own warnings into errors while that variable is set.
  - `cargo check -p gateway --no-default-features` for the headless gateway shape.
  - Never run a standalone `cargo check --workspace` beside clippy.
- Formatter check command: `cargo fmt --all --check`, with rustfmt `style_edition = "2024"`. The pre-commit hook runs it.
- Docs command: `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` with `RUSTDOCFLAGS="-D warnings"` (in PowerShell, `$env:RUSTDOCFLAGS='-D warnings'` first). Under the same flags, also run `cargo doc -p runvil --no-deps` and `cargo doc -p harness --no-deps` with default features, and `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`. The language guide in the sibling `runvil-docs` repository builds with `cargo xtask site --books-only` from this repository, with `RUNVIL_DOCS` set to the `runvil-docs` root. It needs `mdbook`, which is installed, and its link check verifies link targets as files but not `#` anchors.
- Test placement and naming conventions:
  - Unit tests take three forms: a sibling `<module>-tests.rs` wired with `#[cfg(test)] #[path = "<module>-tests.rs"] mod tests;` (a large one splits into `<module>-tests-<label>.rs` siblings, as `prelude-tests-environment.rs` does); an inline `#[cfg(test)] mod tests { }`; or a `src/**/tests/` directory.
  - In `runvil-lua`, `src/tests.rs` declares the crate's behavior tests as `mod <topic>;` children in `src/tests/` (15 files, including `sys_and_var.rs` and `argv.rs`), and a topic with children declares them as a directory, as `src/tests/require.rs` declares `errors`, `prose`, and `reports` in `src/tests/require/`. `src/test_vm.rs` holds the `TestVm` builder: `TestVm::build` and `run_chunk` call `install_engine_globals`, and `TestVm::limits` sets a small Lua memory limit. Protocol parse tests live in `src/protocol/tests/`, such as `parse_tool_call.rs`.
  - Engine Lua tests live in `crates/runvil-internal/engine/src/lua/tests/` (`coroutine.rs`, `errors.rs`, `globals.rs`, `models_loop_contract.rs`, `quota.rs`, `shims.rs`), and executor tests in `crates/runvil-internal/engine/src/execute/tests/`.
  - Integration tests compile as one binary per crate at `tests/it/main.rs`, or `tests/suite/main.rs` in `runvil` and `harness`. Engine prompt fixtures live in `crates/runvil/tests/prompts/`.
  - Test functions are snake_case sentences that state the behavior, such as `one_parent_cancel_ends_every_run_holding_one_of_its_children`. Tests use `expect` freely, which the root `clippy.toml` allows in tests.
- Directory map:
  - `crates/` holds all code. Its top level is the public and shared layer: `runvil` (the Engine's public facade), `runvil-plugin`, `harness` and `harness-gateway-client`, the linked Plugins `plugin-web`, `plugin-mcp`, and `plugin-user-input`, `gateway-api-types` and `gateway-api-discovery`, `shared-error-source`, `shared-loopback`, and `shared-ui`, and the `build-*` tooling (`build-xtask` behind `cargo xtask`, `build-workshop`, `build-ui`, `build-user-guide`, `build-llama-cuda`, and `build-ceiling`).
  - Four manifestless containers hold private crates: `crates/runvil-internal/` (Engine: `types`, `parser`, `lua`, `engine`, `vfs`, `model-client`), `crates/harness-internal/` (`runner`, package `harness-runner`), `crates/gateway/`, and `crates/workshop/`. The root `Cargo.toml` is a virtual workspace manifest with resolver 3, edition 2024, and `rust-version = "1.95"`.
  - Outside `crates/`: `guide/` (user guide source), `prompts/` (sample prompts), `vibe/` (dated implementation plans), `tools/` (Node scripts), `.github/workflows/` (CI; `ci.yml` holds the gates), `.githooks/` (pre-commit runs fmt; pre-push runs the wording scan, the headless gateway check, clippy, and `cargo deny`), `.cargo/config.toml` (the `xtask` and `workshop` aliases), `.config/nextest.toml` (timeouts), and root `clippy.toml`, `deny.toml`, `rustfmt.toml`, and `rust-toolchain.toml` (stable).
  - The language guide lives in the separate `runvil-docs` git repository at `../runvil-docs`, with chapters under `src/language/`. It has no build or tests of its own, and its commit subjects are imperative sentence case.
- Component boundaries: the `runvil` facade depends on all six `runvil-internal` crates, and `runvil-engine` depends on the other five. `runvil-parser` depends on `runvil-lua` and `runvil-types`. `runvil-lua` depends on `runvil-model-client`, `runvil-types`, and `runvil-vfs`, plus `mlua` and `serde_json`. `runvil-model-client` depends on `runvil-types`, and `runvil-types` and `runvil-vfs` are leaves. `harness-runner` depends on `runvil` and `runvil-plugin`, and `harness` on `harness-runner`, `runvil`, and `runvil-plugin`. The gateway family never depends on the Engine or Harness. `cargo test -p build-xtask` enforces these boundaries.
- Conventions summary:
  - Every dependency is declared once in `[workspace.dependencies]`. Workspace lints deny clippy `all` and `pedantic` (casts included), `unwrap_used` and `expect_used` outside tests, and bare `allow` attributes, so code uses `#[expect(..., reason = "...")]`. `missing_docs` warns, and the gates deny warnings.
  - `crates/runvil-internal/clippy.toml` bans sleeps, wall clocks, timed waits, and `RandomState::new` in Engine code, and Engine crates never depend on `tokio`, `reqwest`, `getrandom`, or `fastrand`.
  - Doc comments hold no compiled code blocks; tag a fence `text` or `lua`.
  - Source directories are flat: a subdirectory needs at least three files. Otherwise modules sit beside the parent as `parent-label.rs`, wired with `#[path = "parent-label.rs"] mod label;`, and a sibling group that reaches three files becomes a `parent/` subdirectory in standard module layout. Every source file, tests included, stays under the 500-line ceiling `build_ceiling::check` enforces from each crate's `build.rs`.
  - Engine, Harness, Host, and Plugin are capitalized defined terms. Engine crate text says "the caller" and never names the Host.
  - Comments state only non-obvious constraints. Error messages are written for model consumption and name required versus actual. A Rust-side author error is raised as `mlua::Error::external(crate::Error::Lua(message))`, which `pcall` sees as kind `lua`. Argument errors name a value's type with mlua's `Value::type_name`.
  - Behavior changes ship with tests in the same change. JSON reaching a recorder or replay round-trips exactly with sorted keys. Text files are LF, and commit subjects are imperative sentence case.

</project-survey>
<execution-plan>

## Execution Instructions

<step-1>

### Step 1: json global with decoding, json.null, and json.array [completed]

- Component: `none`
- Tests: Verify runs these from the repository root in PowerShell, in this order.
  - `cargo nextest run --locked -p runvil-lua --all-features`
  - `cargo nextest run --locked -p runvil-engine --all-features`
  - `cargo nextest run --locked -p runvil-parser -p runvil -p harness-runner -p harness --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p runvil-lua -p runvil-engine --all-targets --all-features`
  - `cargo fmt --all --check`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

Paths starting `L/` are under `crates/runvil-internal/lua/src/`, and paths starting `E/` are under `crates/runvil-internal/engine/src/`.

- Code:
  - New `L/json.rs`: module docs and `pub(crate) fn install(lua: &Lua) -> Result<()>`, which builds a plain `json` table holding `null`, `array`, `decode`, and `decode_lines` and raw-sets it into `_G`. It declares `#[path = "json-decode.rs"] mod decode;`. Step 2 adds `encode` and `encode_lines` to the same table.
  - New `L/json-decode.rs`: `json.decode` and `json.decode_lines`.
  - `L/lib.rs`: declare `mod json;`, and name the `json` global in the crate docs beside `log` and `store`.
  - `L/vm/install.rs`: `install_engine_globals` calls `crate::json::install` after `install_store_table`, and its doc comment says it installs `log`, `store`, and `json`.
  - `L/globals.rs`: `RESERVED_NAMES` gains `("json", Reserved::EngineGlobal)` between `item` and `jump`, growing from 61 to 62 entries.
  - `L/prelude.rs`: `VISIBLE_GLOBALS` gains `"json"` after `"untrusted"`, growing from 19 to 20 entries, and the module docs and the `environment` doc comment name `json` among what a prelude sees.
  - `L/sys.rs`: `guarded_json_value` sets `lua.array_metatable()` on the data table it builds for a `Json::Array`. `materialize_guarded_value` sets it on the plain copy when the source data table has it, so `var_to_json`, and the prose substitution that reads `var` through it, give `[]` for an empty list.
- Behavior:
  - `json` is a plain table, like `string`. Null and list detection rely on the values themselves, so reassigning a field of `json` cannot break them.
  - `json.null` is `lua.null()`, a light userdata with a null pointer. `type(json.null)` is `userdata`, it is truthy, and code tests for it with `v == json.null`. Every decoded JSON null is this value.
  - A marked list is a table whose metatable is `lua.array_metatable()`. That metatable holds only `__metatable = false`, so `getmetatable` on a marked list returns `false`, and `setmetatable` on it raises `cannot change a protected metatable`. Every path that converts Lua to JSON with mlua's `from_value` (tool-call arguments, `var` writes, task `item`, message records, and fanout members) already encodes a marked list as a JSON array, so `json.array()` inside a `tools.call` argument table sends `[]`.
  - `json.array()` and `json.array(nil)` return a new empty marked list. `json.array(t)` sets the marker on table `t` and returns `t`, and a table that already has the marker, such as a decoded list, comes back unchanged. A non-table raises `json.array expects a table or no argument, got {type}`. A table with any other metatable raises `json.array: the table already has a metatable`, which covers `var` tables, frozen `argv`, sealed Plugin globals, fanout results, and `_G`.
  - `json.decode(text)` takes a string (`json.decode expects a string, got {type}`) of at most 8,388,608 bytes (`json.decode: input exceeds the 8388608-byte limit`). It parses one document from the string's bytes with `serde_json::from_slice::<serde_json::Value>`, then converts the result with `lua.to_value`, whose default options mark every list and turn every null into `json.null`. A parser failure raises `json.decode: ` followed by serde_json's own message, as in `json.decode: expected value at line 1 column 1` for `x` and `json.decode: trailing characters at line 1 column 3` for `1 2`.
  - serde_json rejects the 128th nested array or object, so 127 levels decode and 128 raise `json.decode: recursion limit exceeded at line 1 column 128`.
  - Integers in the i64 range decode to Lua integers, and every other number decodes to a float, including `9223372036854775808`, which mlua converts to a float rather than raising. Objects become plain writable tables, lists become writable marked lists, and strings stay plain Lua strings, not wrapped in the untrusted envelope.
  - `json.decode_lines(text)` takes the same argument and raises the same three kinds of error with the `json.decode_lines` prefix. It reads every value with `serde_json::Deserializer::from_slice(...).into_iter::<serde_json::Value>()`, so any whitespace separates values, blank lines and `\r\n` included. Several values may share a line, a value may span lines, and an error's line and column count across the whole text. It returns a marked list of the values in order, and empty or blank text returns an empty marked list.
  - Each function checks its argument before parsing. Every error is raised as `mlua::Error::external(crate::Error::Lua(message))`, so `pcall` reports kind `lua`. `{type}` is `Value::type_name`, so `json.decode(5)` reads `got integer`, `json.decode(1.5)` reads `got number`, and `json.array(json.null)` reads `got lightuserdata`. A Lua memory-limit error (`mlua::Error::MemoryError`) raised while building the result passes through unchanged, with no `json.decode:` prefix.
- Tests:
  - `L/tests.rs` declares `mod json;`, and `L/tests/json.rs` declares the children `decode`, `lines`, `marked`, and `envelope` in `L/tests/json/`. Step 2 adds more.
  - `decode.rs`:
    - scalars, nested data, and non-ASCII text decode to the expected Lua values, with `math.type` telling integers (`2`, `-9223372036854775808`) from floats (`1.5`, `1.0`, `9223372036854775808`);
    - `json.decode('[1,null,3]')` has 3 items and the second is `json.null`, and `{"a":null}` keeps `a` as `json.null`;
    - an 8,388,608-byte document decodes, and an 8,388,609-byte one raises the cap message;
    - 127 nested arrays decode, and 128 raise the recursion message;
    - the parser messages for `x` and `1 2`, the type-name messages for `5`, `1.5`, and no argument, and `pcall` reporting kind `lua`;
    - under `TestVm::limits` with a small Lua memory limit, decoding a document of many small arrays raises the memory error with no `json.decode:` prefix.
  - `lines.rs`: blank lines, `\r\n`, two values on one line, one value across lines, empty and blank text returning an empty marked list, an error on line 3 whose message contains `at line 3`, and the argument and cap errors with the `json.decode_lines` prefix.
  - `marked.rs`: `type(json.null)` and its truthiness, `getmetatable(json.array()) == false`, `setmetatable` on a marked list raising `cannot change a protected metatable`, `json.array(t)` returning `t`, `json.array` returning a decoded list unchanged, a decoded list emptied with `table.remove` keeping its marker, writes to decoded objects and lists, and the errors for `json.array(5)`, `json.array(json.null)`, `json.array(setmetatable({}, {}))`, and `json.array(var)`.
  - `envelope.rs` pins the example that chapter 13 of the language guide will quote, patterns included:
    - `untrusted('url: https://example.com/data.json\ntruncated: false\nextraction: plain\n\n{"key":"value","nested":{"ok":true}}')`, cut with `r:match('\n\n(.*)\n</untrusted_input_%x+>$')`, decodes so that `.nested.ok` is `true`;
    - `untrusted('{"query":"rust","results":[]}')`, cut with `r:match('<untrusted_input_%x+>\n(.*)\n</untrusted_input_%x+>$')`, decodes with `results` an empty marked list;
    - a body holding `{"html":"<b>hi</b>"}` decodes so that `html` is `&lt;b>hi&lt;/b>`, because the envelope escaped every `<`.
  - `L/tests/sys_and_var.rs`:
    - `var.items = json.array()` and `var.items = json.decode('[]')` read back through `var_to_json` as `{"items":[]}`;
    - `var.t = { list = json.array() }` reads back as `{"t":{"list":[]}}`, and `var.items = {}` still reads back as `{"items":{}}`;
    - a `var` seeded from Rust with `json!({"items": []})` reads back with `"items": []`;
    - `var.x = json.null` reads back as `{"x":null}`, and `var.x == json.null` holds in Lua.
  - `L/protocol/tests/parse_tool_call.rs`: a `tools.call` argument table `{ items = json.array() }` parses to the arguments `{"items":[]}`.
  - `L/protocol/tests/parse_tasks.rs`: `tasks.spawn` with `item = { a = json.null }` parses to the item `{"a":null}`.
  - The `L/collection.rs` tests: `collection_members` accepts a `lua.null()` member and converts it to JSON null.
  - `E/execute/tests/suite/exec_flow/fanout_collections.rs`: a fanout over `{ json.null }` runs one arm, and the test asserts what the arm's `item` reads, either `json.null` or nil when a null seed installs no `item`. Either outcome is acceptable. The assertion is the record that the docs work after Step 2 reads.
  - `E/execute/tests/suite/exec_flow/globals.rs`: a prose placeholder naming a bare global that holds `json.null`, with the test asserting the outcome, expected to be the `NullValue` substitution failure. Either outcome is acceptable, and the assertion is the record in the same way.
  - `E/execute/tests/suite/args_surface.rs`, beside `an_h1_repair_is_visible_to_every_downstream_section`: an H1 block that assigns `argv = { a = json.null }` passes the freeze, and a later section reads `argv.a` as nil.
  - `L/prelude-tests.rs`: a prelude function reads `type(json)` as `table` and returns `json.decode('[1]')[1]`.
  - These existing tests pass unchanged: `every_global_section_setup_leaves_in_g_is_a_reserved_name` and `every_reserved_global_is_present_after_section_setup` in `E/lua/tests/globals.rs`, the `RESERVED_NAMES` tests in `L/globals-tests.rs`, and `every_reserved_name_the_grammar_allows_parses_as_a_model_role_label` in `crates/runvil-internal/parser/src/contract/tests.rs`. No existing test asserts that an empty `var` list reads back as `{}`. If a Harness test turns one up, it changes to expect `[]`, the intended behavior.
- Done when: every section VM has a `json` global holding `null`, `array`, `decode`, and `decode_lines`, `json` is reserved and visible to preludes, an empty marked list in `var` reads back as `[]`, and the Tests commands pass.

</step-1>
<step-2>

### Step 2: json.encode and json.encode_lines

- Component: `none`
- Tests: Verify runs these from the repository root in PowerShell, in this order, beside the FULL-scope commands in the Project Survey.
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`

Paths starting `L/` are under `crates/runvil-internal/lua/src/`, and paths starting `E/` are under `crates/runvil-internal/engine/src/`.

- Code:
  - New `L/json-encode.rs`: the encoder behind `json.encode` and `json.encode_lines`, with option parsing and the strict and debug modes. It builds a `serde_json::Value` and writes it with `serde_json::to_string`. `L/json.rs` declares it with `#[path = "json-encode.rs"] mod encode;` and adds `encode` and `encode_lines` to the `json` table, which then holds its six members. If the encoder needs a third child file, the module becomes `json.rs` with `json/encode.rs`, `json/decode.rs`, and `json/keys.rs`, as the Technical Design says.
  - `L/argv.rs`: `frozen_json_value` sets `lua.array_metatable()` on the data table it builds for a `Json::Array`, so the encoder, reading through frozen `argv`, sees an empty list as `[]`.
- Behavior of `json.encode(value [, options])`:
  - Options are nil or a table whose only key is `debug`, a boolean, and they are checked before the value. The errors are `json.encode: options must be a table, got {type}`, `json.encode: unknown option {key}` with `{key}` rendered by `key_text` (`L/proxy.rs`), and `json.encode: option debug must be a boolean, got {type}`. So `json.encode(function() end, 5)` raises the options error.
  - Output is compact JSON with no spaces and sorted object keys. A newline inside a string is written `\n`, so every encoded value is one line.
  - Top-level `nil` and `json.null` encode as `null`, and booleans as themselves. Strings must be valid UTF-8: `json.encode: {path} is not valid UTF-8`.
  - Integers encode as integers. A float with no fractional part, from -2^63 up to but not including 2^63, encodes as an integer, so `2.0` is `2`, `-0.0` is `0`, and `-2^63` is `-9223372036854775808`, while `2^63` stays a float. Other finite floats use the shortest text that reads back exactly. NaN and infinities raise `json.encode: {path} is not a finite number`.
  - Functions, threads, and every userdata other than `json.null`, light or full, raise `json.encode: {path} must be JSON data, got {type}`. That includes the message-record views that `msgs[i]` returns, even though mlua can serialize them.
  - A sealed view is a table with no entries of its own whose metatable, read with `Table::metatable`, has both `__index` and `__metatable`. When `__index` is a table, that table is encoded in the view's place by these same rules, which covers `var` tables, `var` itself, frozen `argv`, sealed Plugin globals, and fanout results. When `__index` is a function, the view raises `json.encode: {path} cannot be read ({label}); copy the fields you need into a plain table`. The parenthetical appears only when `__metatable` is a string, as in `json.encode: value cannot be read (sys is sealed); copy the fields you need into a plain table`. When `__index` is any other value, the table is not a sealed view and encodes as its own entries.
  - A marked list must have only the keys 1 to n, where n may be 0, and encodes as a JSON array. A non-integer key raises `json.encode: {path} is marked as a list but has a {type} key`, and integer keys that are not exactly 1 to n raise `json.encode: {path} is missing list item [{n}]`.
  - An unmarked table encodes as `{}` when empty, as an array when its keys are exactly 1 to n, and as an object when every key is a string. Any other table raises, reporting in this order: a key that is neither an integer nor a string (`json.encode: {path} has a {type} key; object keys must be strings`), then a mix of integer and string keys (`json.encode: {path} mixes list items and named keys`), then a missing item (`json.encode: {path} is missing list item [{n}]`). Any metatable other than the list marker or a sealed view's is ignored, and the table's own entries are encoded.
  - For the missing item, n is the table's count of integer keys, and the reported item is the first of 1 to n that is absent: `{ [1] = 'a', [3] = 'c' }` reports `[2]`, `{ [0] = 'a' }` reports `[1]`, and `{ [-1] = 'a', [1] = 'b' }` reports `[2]`.
  - A table's key rules are checked before any of its values are encoded, so `{ [1] = function() end, a = 1 }` reports the mix, not the function, and error paths only step through string and integer keys.
  - Paths start at `value` and add each key with `key_path` (`L/proxy.rs`), giving `.name` for a string key and `[k]` for any other, as in `value.items[3]`. For an object key that is not valid UTF-8, the not-valid-UTF-8 error names that key's own path, which `key_path` renders with replacement characters.
  - Keys are visited in the order `collection::sort_key` and `SortKey::compare` define: booleans, then numbers by value, then strings bytewise. A non-UTF-8 string key sorts as `SortKey::Text` of its bytes, and an infinite number key as `SortKey::Float`, so `-math.huge` comes first among the numbers and `math.huge` last. Table, function, userdata, and thread keys come last, ranked by `Value::type_name`, so a table with both a table key and a function key reports the function key.
  - Cycle detection keeps the tables on the current path, counting a sealed view by its data table, and compares each new table to them by pointer, never printing an address. A table met again on its own path raises `json.encode: {path} contains itself`, while the same table in two places is allowed. A key that is a table is never stepped into.
  - The top-level table is level 1, and reading through a sealed view to its data table adds no level. A table more than 127 levels deep raises `json.encode: {path} is nested more than 127 levels deep`. The cycle check runs before the depth check at each table, so a loop of at most 127 tables gets the cycle message, even when its repeat falls at level 128.
  - Every error is raised as `mlua::Error::external(crate::Error::Lua(message))`, so `pcall` reports kind `lua`, and `{type}` is `Value::type_name`. A Lua memory-limit error raised while building the result string passes through unchanged.
- Debug mode, `{ debug = true }`, follows the same walk and writes a string in place of each failure:
  - A function becomes `"[function]"`, a thread `"[thread]"`, and every userdata other than `json.null` `"[userdata]"`, whether or not it defines `__tostring`, because the encoder never calls into Lua.
  - A table that contains itself becomes `"[cycle]"`, and a table more than 127 levels deep becomes `"[too deep]"`.
  - NaN becomes `"NaN"`, infinity `"Infinity"`, and negative infinity `"-Infinity"`. Invalid UTF-8 in strings and keys is replaced with replacement characters.
  - A table that breaks the key rules, marked or not, becomes an object. A number key becomes the text it would encode as, such as `2`, `2.5`, or `1e300`, with infinite keys as `Infinity` and `-Infinity`, and boolean keys become `true` and `false`. When two keys give the same text, the later one in visiting order wins. Table, function, userdata, and thread keys are left out.
  - A sealed view with an `__index` function becomes `"[cannot read: <label>]"`, or `"[cannot read]"` when `__metatable` is not a string.
  - Option errors still raise, and the output never contains a memory address.
- Behavior of `json.encode_lines(list [, options])`:
  - It returns each item encoded by the `json.encode` rules, each followed by `\n`, so an empty list returns `""`. The list may be a marked list, an empty table, a table with keys 1 to n, or a sealed view of one, such as `var.rows`.
  - A non-table raises `json.encode_lines expects a list, got {type}`. An unmarked table with no integer keys but some other key raises `json.encode_lines expects a list, got a table with a {type} key`, naming the first key in visiting order, so an object reports `got a table with a string key`. A marked list, or an unmarked table with integer keys, that is still not a list raises the list messages above with the `json.encode_lines:` prefix, as in `json.encode_lines: value mixes list items and named keys`. A list argument that is an unreadable sealed view raises `json.encode_lines: value cannot be read (sys is sealed); copy the fields you need into a plain table`. Option errors carry the `json.encode_lines:` prefix too.
  - An item error names the item under the list's path, as in `json.encode_lines: value[2].name must be JSON data, got function`. The list stays on the cycle path while its items encode, so a list containing itself raises `json.encode_lines: value[1] contains itself`.
  - In debug mode, the argument errors above still raise, and only failures inside items become placeholders.
- Tests:
  - `L/tests/json.rs` adds the children `encode`, `errors`, `debug`, and `views`, and `lines.rs` gains the `json.encode_lines` cases.
  - `encode.rs`:
    - `json.encode({})` is `{}`, `json.encode(json.array())` is `[]`, `json.encode(json.decode('[]'))` is `[]`, and `json.encode({ b = 1, a = 2 })` is `{"a":2,"b":1}`;
    - round trips of scalars, nested data, and non-ASCII text through `json.decode` and `json.encode`;
    - the numbers `2.0`, `-0.0`, `1.5`, `2^63` (its text decodes back to a float equal to `2^63`), and `-2^63`;
    - `json.encode()`, `json.encode(nil)`, and `json.encode(json.null)` give `null`, and `json.null` inside objects and lists encodes as `null`;
    - a decoded list emptied with `table.remove` encodes as `[]`, and a string holding `\n` encodes on one line;
    - 127 nested tables encode, and 128 raise the depth message.
  - `errors.rs`:
    - every strict message above with its path;
    - the key-rule precedence, the three missing-item examples, and the key-before-value example;
    - a table with a table key and a function key reporting the function key;
    - a non-UTF-8 string key and an infinite number key each reporting the same error however the table was built;
    - the same invalid table built in two key orders, in two fresh VMs, raising the same message;
    - `t.self = t` raising `json.encode: value.self contains itself`, a loop of 127 tables whose repeat falls at level 128 getting the cycle message, and a shared table in two places encoding;
    - the options messages, `json.encode(function() end, 5)` reporting the options error, and `pcall` reporting kind `lua`.
  - `debug.rs`:
    - each placeholder above, including a test-only userdata that defines `__tostring` still rendering as `"[userdata]"`;
    - number keys rendering as `2`, `2.5`, `1e300`, and `Infinity`, and a key `1` colliding with a key `"1"`, where the string key wins because it is visited later;
    - `json.encode({ f = function() end }, { debug = true })` giving `{"f":"[function]"}`;
    - non-UTF-8 and infinite keys ordering the same way every time, and option errors still raising.
  - `views.rs`:
    - `json.encode(var.items)`, `json.encode(var)`, and frozen `argv` encode the stored data, and an empty list in either encodes as `[]`;
    - a `var` value 127 levels deep encodes, because each view adds no level;
    - `json.encode(sys)` raises the cannot-read message with `(sys is sealed)`, and debug mode writes `"[cannot read: sys is sealed]"`;
    - `json.encode(msgs[1])` on a message-record view raises `json.encode: value must be JSON data, got userdata`, and debug mode writes `"[userdata]"`;
    - an empty table whose metatable has `__metatable` and an `__index` of `5` encodes as `{}`.
  - `lines.rs`:
    - `json.encode_lines({ { a = 1 }, { a = 2 } })` is `{"a":1}\n{"a":2}\n`, an empty list gives `""`, and `var.rows` reads through;
    - `json.decode_lines(json.encode_lines(rows))` equals `rows` for a list of objects;
    - the errors for an object (`got a table with a string key`), a mixed table, `sys`, an option error, an argument error in debug mode, an item error naming `value[2]`, and a list containing itself, and debug placeholders inside items.
  - `L/tests/argv.rs`: frozen `argv` holding an empty list encodes it as `[]`.
  - `L/prelude-tests.rs`:
    - a prelude function can call `json.encode`;
    - `json.encode(var)` inside a prelude function raises `json.encode: value cannot be read (var is read-only); copy the fields you need into a plain table`, because a prelude's `var` view has a function `__index`;
    - section code encoding a sealed Plugin global gets the prelude's hidden table.
  - `E/execute/tests/suite/fanout.rs`, or a new sibling if that file would pass 500 lines: `json.encode` of a fanout result gives its `exhausted`, `item`, `ok`, and `text` fields as an object.
- Done when: `json` holds `encode`, `encode_lines`, `decode`, `decode_lines`, `null`, and `array`, the tests above pass, and the FULL-scope commands and the Tests commands pass.

</step-2>

### After Step 2, by hand: the runvil-docs language guide

This work runs outside the Vibe Coder's review and commit cycle, as the Decision Record says, so no `step-N` range holds it and the Run Sequence never dispatches it.

- When and where: after the Vibe Coder run's `Close plan:` commit lands in `runvil`, in the `runvil-docs` repository at `../runvil-docs`, starting from a clean worktree there.
- Facts come from the Functional Specification, plus three things the steps' tests pin:
  - what a fanout arm's `item` reads for a `json.null` member, from `crates/runvil-internal/engine/src/execute/tests/suite/exec_flow/fanout_collections.rs`;
  - what a prose placeholder does for a bare global holding `json.null`, from `crates/runvil-internal/engine/src/execute/tests/suite/exec_flow/globals.rs`;
  - the two `string.match` patterns chapter 13 quotes, from `crates/runvil-internal/lua/src/tests/json/envelope.rs`.
- Writing rules: line numbers below are from before any edit, so after the first change in a file, find the quoted text instead. Every new statement about behavior matches the Functional Specification or a test. New examples follow each chapter's existing form: a prompt in a `markdown` fence, then "the run result is" and a `text` fence.
- Chapters:
  - `src/language/04-how-a-prompt-runs.md`: at line 1123, "To return data held in a table, build a string from it first, for example with `table.concat`", add `json.encode` for a result that should be JSON text.
  - Update `src/language/05-lua-environment.md`:
    - Add `json` to the Engine globals list (lines 165-172).
    - Change the count of 20 Engine globals to 21 (lines 174 and 178).
    - Change setup step 2 to "`log`, `store`, and `json`" (line 187).
    - Change "61 names: the 20 Engine globals" to "62 names: the 21 Engine globals" (line 199).
    - Add a link to the new section in the paragraph at line 176.
    - Add a section, "Encoding and decoding JSON with `json`", before "Run facts in `sys` and `ui`" (line 1197). It covers every behavior under Functional Specification, a `store.append` example with `json.encode_lines`, the 256-character `log` limit for dumps, that `json.null` is truthy so code tests `v == json.null`, that marked lists return `false` from `getmetatable` and refuse `setmetatable`, the 127-level depth limit on both sides, and that an empty list in `var` now reads back, and renders in prose, as `[]`.
    - At line 1069, where `var` values read back as JSON, add that an empty list reads back as `[]` when it is a marked list (from `json.array`, `json.decode`, or a tool result) and as `{}` when it is a plain empty table, and that `json.null` stored in `var` reads back as `null`.
    - At lines 1124-1131, where a `var` table looks empty to `#`, `pairs`, and `next`, add that `json.encode` reads through it, so `json.encode(var.items)` gives the stored data.
    - At lines 1174-1177, the `{path} must be JSON data, got {type}` guard, add that `json.null` is the one userdata the guard accepts, and that it stores JSON null.
    - At line 1509, "A JSON null field reads as Lua `nil` ... and never as a NULL sentinel userdata", add that `json.decode` and structured tool results do give that sentinel, `json.null`.
    - At line 2761, the protected-metatable rule "even when the value is `false`", add that lists from `json.decode`, `json.array`, and tool results are a case of it: `getmetatable` returns `false`.
  - `src/language/06-arguments.md`:
    - At line 572, "The `args` global always holds the exact argument string, never parsed as JSON", add that `json.decode(args)` parses it yourself, under `pcall` when it may be malformed, and returns `json.null`, not nil, for a JSON `null`.
    - At lines 1176, 1261, and 1263, where the freeze refuses "a function, userdata, or thread", add that `json.null` is accepted and freezes as JSON null, which later sections read as nil, as line 841 already says.
  - `src/language/07-substitution.md`:
    - In the render table at lines 968-976, add a row for an empty list, as in `json.array()` rendering `[]`, and a row for an empty table, as in `{}` rendering `{}`. Say that `{{ var.items }}` for an empty list stored in `var` renders `[]`.
    - At lines 849 and 900, where a bare global holding "a function, userdata, or coroutine thread" is refused, add what a placeholder does when the global holds `json.null`, as the prose test under Testing Plan pins it.
  - `src/language/08-jump-and-call.md`: at line 1136, where the call input becomes the chain's `args`, add that `json.encode` builds a call input from a table, as in `call('## Sub', json.encode({ query = 'chain' }))`, and that the chain reads it back through `argv` or with `json.decode(args)`.
  - `src/language/09-the-store.md`:
    - At line 322, "`store.append` adds no separator of its own", add that `json.encode_lines` ends every record with a newline, so `store.append('rows.jsonl', json.encode_lines(rows))` appends whole records, and `json.decode_lines(store.read('rows.jsonl'))` reads them back.
    - At line 355, "The store holds text", add that a table goes through `json.encode` before a write and `json.decode` after a read.
  - `src/language/11-conversations.md`:
    - At line 366, where a tool call's `arguments` arrive "already parsed into a Lua table", add that JSON lists arrive as marked lists and JSON null as `json.null`.
    - At line 368, "A Lua `arguments` table is stored as a JSON object", add that an empty list inside it needs `json.array()` to be sent as `[]`.
    - At lines 619 and 635, which list the JSON-representable values as strings, numbers, booleans, and nested tables, add `json.null`.
    - At line 893, where a message-record view converts like its record, add that `json.encode` refuses a view with the userdata error, so encode `msgs[i].content` or a plain table instead.
    - At line 1406, "the JSON null that a tool result holds", name it `json.null`.
  - Update `src/language/12-tools.md`:
    - At line 1151, where an empty table calls a tool with no arguments, add that an empty list nested inside the arguments needs `json.array()` to reach the tool as `[]`, as in `{ include_domains = json.array() }`.
    - At line 1313, rewrite "The script never decodes JSON itself and has no JSON codec to reach for", which becomes false: a structured tool's output arrives already parsed, so no decode is needed, and JSON text from a plain tool can be decoded with `json.decode`.
    - At line 1317, "A JSON `null` becomes a null light userdata, not `nil`", add that this value is `json.null`, tested with `v == json.null`.
    - At line 1319, "JSON text from a plain tool still arrives as a string", link to `json.decode`, and for an untrusted tool, to the chapter 13 example.
    - Add `json` to the globals a prelude sees (the bullet at line 2543, which becomes "the `tools`, `store`, `untrusted`, and `json` globals"), to the helper sentence at line 2552 ("A helper can call `tools.call`, use the `store` table, call `untrusted`, and use the standard `string` library"), and to the Engine globals a prelude global may not take (line 2617, between `item` and `jump`).
  - `src/language/13-web-fetch-and-search.md`:
    - After line 727, "`web/fetch` returns the JSON body verbatim", add a subsection with a worked example. It fetches `https://example.com/data.json` as `fetch-data` does, cuts the body out with `string.match` (after the header block's blank line and before the closing `</untrusted_input_...>` tag), decodes it with `json.decode`, and returns one field, followed by its run result. It states the caveats: the envelope has already escaped every `<` in the body as `&lt;`, broken copies of the nonce, and spaced chat-template markup, so string values can differ from what the server sent; and decoded strings are plain, so wrap them with `untrusted` before they reach a model. It links to "The envelope shape" in `09-the-store.md` and "What the envelope escapes" in `12-tools.md`.
    - In "Reading the search results" (line 301), add one sentence that the same cut-and-decode works on search results, which have no header block, linking to the example.
    - At line 368, "Omitting either list is the same as passing an empty list", add that from Lua an empty list is written `json.array()`, because a bare `{}` is sent as an empty object.
  - `src/language/14-fanout.md`: at lines 521 and 1760, where a function, userdata, or thread is never a member, add that `json.null` is accepted as a member, and what the arm's `item` then reads, as the fanout test under Testing Plan pins it.
  - `src/language/15-tasks.md`: at line 303, "`item` takes any JSON data value", add that an empty list seed needs `json.array()` to arrive as `[]`, and at line 339, where userdata is refused, add that `json.null` is accepted.
  - Update `src/language/17-quick-reference.md`: add rows for `json`, `json.array`, `json.decode`, `json.decode_lines`, `json.encode`, `json.encode_lines`, and `json.null` in alphabetical order, which puts them between the `item` and `jump` rows. Add the decode byte cap and the 127-level depth limit to the limits table, both `fixed`.
  - Chapters `01`, `02`, `03`, `10`, and `16` need no change. The places in `02` and `03` that name example reserved names or globals still read correctly without `json`, and `16` covers serialization failures as `lua` errors already.
- Checks, run in PowerShell, each passing before the commit:
  - From the `runvil-docs` root, `rg -n "has no JSON codec|61 names|20 Engine globals|beyond these 20" src/language` prints nothing.
  - From the `runvil` root, `$env:RUNVIL_DOCS=(Resolve-Path ../runvil-docs).Path; cargo xtask site --books-only` passes.
  - Every new or changed heading's anchor matches every link to it, checked by reading, because the site build does not check `#` anchors.
  - One read of the whole diff against the Functional Specification.
- Commit: one commit in `runvil-docs` with an imperative sentence-case subject, such as `Document the json global`. Do not push.

</execution-plan>
