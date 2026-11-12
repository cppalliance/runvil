//! What a `require` names: a path from the top is the same library from a
//! nested fanout worker, from a section under another parent, and from a
//! library's own code, while it loads and when its functions run later.

use super::*;

#[test]
fn a_nested_fanout_worker_requires_a_top_level_library() {
    let source = PromptSource::new()
        .section(
            "Research",
            "
            local results = tasks.fanout('Worker', { 'alpha', 'beta' })
            return table.concat(results, ',')",
        )
        .section("### Worker", "return require('/helpers').label(item)")
        .prose(
            "helpers",
            "
            for {{ item }}

            ```lua
            local seen = prose
            return { label = function(value) return value .. ' ' .. seen end }
            ```",
        );
    let trace = require_case(source.build()).run();
    assert_eq!(
        trace.text(),
        "alpha for alpha,beta for beta",
        "each arm renders the library prose with its own item"
    );
    let loads = library_loads(&trace.records());
    assert_eq!(
        loads
            .iter()
            .filter(|(section, detail)| section == "Worker"
                && detail == "Lua library load succeeded: /helpers")
            .count(),
        2,
        "each arm's VM loads the library itself: {loads:?}"
    );
}

#[test]
fn a_nested_worker_reads_and_requires_a_top_level_section_by_one_path() {
    let source = PromptSource::new()
        .section(
            "Research",
            "return tostring(tasks.fanout('Worker', { 'alpha' })[1])",
        )
        .section(
            "### Worker",
            "
            local read = prompt.section('/house_style')
            local raw = read:find('{{ item }}', 1, true) ~= nil
            return tostring(raw) .. '|' .. require '/house_style'",
        )
        .prose(
            "house_style",
            "
            Plain words for {{ item }}.

            ```lua
            return prose
            ```",
        );
    let trace = require_case(source.build()).run();
    assert_eq!(
        trace.text(),
        "true|Plain words for alpha.",
        "the read returns the raw text and the library renders its prose"
    );
}

#[test]
fn a_library_requires_its_own_helper_by_one_path_while_it_loads_and_later() {
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local lib = require '/lib'
            return lib.helped .. '|' .. lib.later() .. '|' .. call('Nested')",
        )
        .section(
            "### Nested",
            "return require('/lib').later() .. ',' .. require '/lib/helper'",
        )
        .section(
            "lib",
            "
            local helped = require '/lib/helper'
            return {
              helped = helped,
              later = function() return require '/lib/helper' end,
            }",
        )
        .section("### helper", "return 'helped'");
    let trace = require_case(source.build()).run();
    assert_eq!(
        trace.text(),
        "helped|helped|helped,helped",
        "the path names the library's helper wherever the require runs"
    );
}

#[test]
fn a_section_requires_sections_nested_under_other_parents() {
    let source = PromptSource::new()
        .section("Main", "return call('Deep')")
        .section(
            "### Deep",
            "
            return table.concat({ require '/Other/Niece', require '/Other/Niece/Grand',
              require '/Last' }, ',')",
        )
        .prose("Other", "")
        .section("### Niece", "return 'niece'")
        .section("#### Grand", "return 'grand'")
        .section("Last", "return 'last'");
    let trace = require_case(source.build()).run();
    assert_eq!(trace.text(), "niece,grand,last");
}
