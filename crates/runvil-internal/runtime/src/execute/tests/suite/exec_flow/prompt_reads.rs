//! `prompt.section` and `prompt.list`: what each returns for a relative
//! path and for a path from the top, the only calls that raise, and the
//! reads from H1 and a library.

use super::*;

/// The raw Markdown `Rules` holds: an indented first line, inline markup,
/// a `lua` fence, a thematic break, and a template, all of which
/// `prompt.section` returns verbatim, neither run nor rendered. The break
/// resets the section's prose and list capture but not its Markdown.
const RULES: &str = "  Indented first line.\n\n\
- **Bold** and `code` stay raw.\n\n\
```lua\n\
var.never = 'run'\n\
```\n\n\
---\n\n\
Closing line with {{ args }}.";

/// The sections every test joins after its own `Main`: a child list `Kid`
/// with a grandchild `Grand`, a numbered `Nums`, the raw `Rules`, an empty
/// `Empty`, and `Other` with a niece `Niece`. It stays literal because
/// `Rules` must come back byte for byte.
const DOCUMENT: [&str; 3] = [
    "\n### Kid\n\n\
- kid-one\n\
- kid-two\n\n\
#### Grand\n\n\
- grand-item\n\n\
## Nums\n\n\
1. one\n\
2. two\n\
3. three\n\n\
## Rules\n\n",
    RULES,
    "\n\n\
## Empty\n\n\
## Other\n\n\
Other prose.\n\n\
### Niece\n\n\
- niece-item\n",
];

/// A relative path names a neighbor: `name` is a direct child and
/// `../name` is a sibling. `prompt.section` returns the section's own raw
/// Markdown and `prompt.list` its pre-parsed items in order. A niece, a
/// grandchild, the caller itself, and a name no section has are no
/// neighbor's name, so each reads as `nil` from both.
#[test]
fn a_relative_path_reads_a_child_or_a_sibling() {
    let main = prompt(
        "",
        &[(
            "Main",
            "
            assert(prompt.section('Kid') == '- kid-one\\n- kid-two', 'a child reads its own Markdown, without its children')
            local kids = prompt.list('Kid')
            assert(#kids == 2 and kids[1] == 'kid-one' and kids[2] == 'kid-two', 'a child list reads its items')
            local nums = prompt.list('../Nums')
            assert(#nums == 3 and nums[1] == 'one' and nums[2] == 'two' and nums[3] == 'three', 'a sibling list reads its items in order')
            assert(prompt.section('../Nums') == '1. one\\n2. two\\n3. three', 'a list section reads its raw Markdown too')
            assert(prompt.section('../Empty') == '', 'an empty section reads as the empty string')
            for _, path in ipairs({'Niece', 'Grand', '../Kid', 'Nums', '../Main', '../Missing', '## Nums'}) do
              assert(prompt.section(path) == nil, path .. ' must read nil from prompt.section')
              assert(prompt.list(path) == nil, path .. ' must read nil from prompt.list')
            end
            return prompt.section('../Rules')",
        )],
    );
    let trace = Scenario::new(main + &DOCUMENT.concat()).run();
    assert_eq!(
        trace.text(),
        RULES,
        "a section's Markdown comes back verbatim"
    );
}

/// A path from the top names any section, and the same one from
/// everywhere: the caller itself, its parent, a grandchild, and a section
/// nested under another parent. A path that names no section reads as
/// `nil`.
#[test]
fn a_path_from_the_top_reads_any_section_from_any_place() {
    let reads = "
        assert(prompt.section('/Main') ~= nil, 'a top-level section reads')
        assert(prompt.section('/Main/Kid') == '- kid-one\\n- kid-two', 'a nested section reads its own Markdown')
        assert(prompt.list('/Main/Kid/Grand')[1] == 'grand-item', 'a grandchild reads')
        assert(prompt.list(' / Other / Niece ')[1] == 'niece-item', 'a section under another parent reads')
        assert(prompt.section('/Other') == 'Other prose.', 'a section reads without its children')
        assert(prompt.section('/Empty') == '', 'an empty section reads as the empty string')
        assert(prompt.section('/Tree/Leaf'):find('leaf', 1, true), 'a nested section with Lua reads')
        assert(prompt.section('/Tree'):find('Leaf', 1, true), 'its parent reads')
        for _, path in ipairs({'/Kid', '/Main/Niece', '/Missing/Kid', '/Other/Niece/Deeper', '/Reads/Main'}) do
          assert(prompt.section(path) == nil, path .. ' must read nil from prompt.section')
          assert(prompt.list(path) == nil, path .. ' must read nil from prompt.list')
        end";
    let head = PromptSource::new()
        .h1(reads)
        .section(
            "Main",
            &format!("{reads}\nreturn call('../Tree') .. '|' .. prompt.section('/Rules')"),
        )
        .build();
    // `Leaf` reads itself and its parent by the same paths as everyone.
    let tree = format!(
        "\n## Tree\n\n```lua\nreturn call('Leaf')\n```\n\n\
         ### Leaf\n\n```lua\n{reads}\nreturn 'leaf'\n```\n"
    );
    let trace = Scenario::new([head, DOCUMENT.concat(), tree].concat()).run();
    assert_eq!(
        trace.text(),
        format!("leaf|{RULES}"),
        "the H1, a top-level section, and a nested one read the same sections"
    );
}

/// A read raises only when the call itself is wrong: a target that is not a
/// string, a malformed path, or `prompt.list` naming a section with no
/// pre-parsed items, whose message shows the path without the whitespace
/// around it. A path that names no section is not a mistake, so it reads
/// as `nil`. A `call` with the same path raises, and its error says where
/// it looked and lists no section.
#[test]
fn prompt_reads_raise_only_for_mistakes_in_the_call() {
    let main = prompt(
        "",
        &[(
            "Main",
            "
            local function expect_raise(read, target, needle)
              local ok, err = pcall(read, target)
              assert(not ok, 'expected a raise for ' .. tostring(target))
              assert(tostring(err):find(needle, 1, true), tostring(target) .. ' must raise with ' .. needle .. ', got: ' .. tostring(err))
            end
            for _, read in ipairs({prompt.section, prompt.list}) do
              expect_raise(read, 42, 'section target must be a string, got integer')
              expect_raise(read, '', 'section path \"\" has an empty step')
              expect_raise(read, 'Kid/', 'section path \"Kid/\" has an empty step')
              expect_raise(read, 'Kid/Grand', 'section path \"Kid/Grand\" has more than one name; a relative path is a child\\'s name, or \"../\" and a sibling\\'s name')
              expect_raise(read, '..', 'section path \"..\" needs a name after \"..\"')
              expect_raise(read, 'Kid[1]', 'section path \"Kid[1]\" has an index; a path in a prompt takes none')
              expect_raise(read, '/Main/../Nums', 'section path \"/Main/../Nums\" uses \"..\"; a path from the top takes none')
              for _, path in ipairs({'../Missing', 'Niece', '/Missing'}) do
                local ok, value = pcall(read, path)
                assert(ok and value == nil, path .. ' must read as nil, not raise: ' .. tostring(value))
              end
            end
            expect_raise(prompt.list, '  ../Rules  ', 'section `../Rules` has no pre-parsed items')
            expect_raise(prompt.list, '/Empty', 'section `/Empty` has no pre-parsed items')
            assert(prompt.section('../Rules') ~= nil, 'prompt.section reads a section prompt.list refuses')
            local ok, err = pcall(call, '../Missing')
            assert(not ok, 'a call to a missing section still raises')
            local message = tostring(err)
            assert(message:find('section `../Missing` not found: `/Main` has no sibling `Missing`', 1, true), 'the call error says where it looked: ' .. message)
            for _, other in ipairs({'Other', 'Kid', 'Nums', 'Niece', 'Grand'}) do
              assert(not message:find(other, 1, true), 'the error lists ' .. other .. ': ' .. message)
            end
            return 'ok'",
        )],
    );
    let trace = Scenario::new(main + &DOCUMENT.concat()).run();
    assert_eq!(
        trace.text(),
        "ok",
        "every raise must be caught and every miss must read nil"
    );
}

/// Both reads work in H1, whose children are the top-level sections and
/// which has no siblings, and in a library, which reads from the calling
/// section: at load time, where each requiring section's load resolves
/// from that section, and from a library function, which resolves from
/// whichever section calls it.
#[test]
fn prompt_reads_work_in_h1_and_a_library() {
    let head = PromptSource::new()
        .h1(
            "
            require '/kid_reader'
            assert(prompt.section('Main') ~= nil, 'a top-level section is a child of the H1')
            assert(prompt.section('Kid') == nil, 'a nested section is no child of the H1')
            assert(prompt.section('../Main') == nil, 'the H1 has no siblings')
            assert(kids_at_load == nil, 'the load-time read in the H1 resolves from the H1')
            assert(kid_items() == nil, 'a library function reads from the H1')
            var.nums = table.concat(prompt.list('Nums'), ',')",
        )
        .section(
            "Main",
            "
            require '/kid_reader'
            assert(kids_at_load ~= nil, 'the load-time read in Main resolves from Main')
            return var.nums .. '|' .. table.concat(kids_at_load, ',') .. '|' .. table.concat(kid_items(), ',')",
        );
    let library = "\n## kid_reader\n\n\
```lua\n\
kids_at_load = prompt.list('Kid')\n\
function kid_items()\n\
  return prompt.list('Kid')\n\
end\n\
```\n";
    let source = [head.build().as_str(), &DOCUMENT.concat(), library].concat();
    let trace = Scenario::new(source).run();
    let out = trace.text();
    assert_eq!(
        out, "one,two,three|kid-one,kid-two|kid-one,kid-two",
        "H1 reads the top-level list, and both the load-time read and the library function read the caller's child"
    );
}
