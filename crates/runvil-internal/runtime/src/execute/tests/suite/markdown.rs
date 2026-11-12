//! The `markdown` global in a running prompt: text from the store and
//! from a `prompt` read parsed into blocks, a parse result carried to a
//! later section in `var`, and parsed sections as a fanout collection.

use runvil_prompt::test_support::PromptSource;

use crate::test_support::scenario::Scenario;

/// A stored document with a title, two sections, and a fence in the
/// second.
const NOTES: &str = "# Notes\\n\\n## Abstract\\n\\nShort and plain.\\n\\n## Data\\n\\n\
```json\\n{ \"ok\": true }\\n```\\n";

#[test]
fn a_stored_file_is_read_then_parsed_and_one_section_is_written_back_as_text() {
    let source = PromptSource::new().section(
        "Main",
        &format!(
            "
            store.write('notes.md', '{NOTES}')
            local paper = markdown.parse(store.read('notes.md'))
            local abstract = markdown.render(markdown.section(paper, '//Abstract').blocks)
            local data = json.decode(markdown.section(paper, '/Notes/Data', 'fence json').text)
            local titles = {{}}
            for _, section in ipairs(markdown.select(paper, 'section')) do
              titles[#titles + 1] = section.title
            end
            return abstract .. '|' .. tostring(data.ok) .. '|' .. table.concat(titles, ',')
              .. '|' .. tostring(markdown.section_or(paper, '//Findings'))"
        ),
    );
    assert_eq!(
        Scenario::new(source.build()).run().text(),
        "Short and plain.|true|Notes,Abstract,Data|nil"
    );
}

#[test]
fn the_text_a_prompt_read_returns_parses_and_its_fence_parses_again() {
    let source = PromptSource::new()
        .section(
            "Write",
            "
            local blocks = markdown.parse(prompt.section('Template'))
            local outline = markdown.parse(blocks[2].text)
            return table.concat({ #blocks, blocks[1].kind, blocks[2].lang, blocks[2].line,
              markdown.section(outline, '/Title/Summary').level }, '|')",
        )
        .prose(
            "### Template",
            "
            Fill in this outline.

            ```markdown
            # Title

            ## Summary
            ```",
        );
    assert_eq!(
        Scenario::new(source.build()).run().text(),
        "2|prose|markdown|3|2",
        "a parse of a section's text counts lines from the section's first line"
    );
}

#[test]
fn a_parse_result_kept_in_var_works_with_every_function_in_a_later_section() {
    let source = PromptSource::new()
        .section("Parse", &format!("var.doc = markdown.parse('{NOTES}')"))
        .section(
            "Use",
            "
            local doc = var.doc
            return table.concat({
              markdown.section(doc, '/Notes/Abstract', 'prose').text,
              markdown.section(doc, '//Data', 'fence').lang,
              tostring(markdown.section_or(doc, '/Notes/Missing')),
              #markdown.select(doc, 'section'),
              markdown.render(markdown.section(doc, '//Abstract').blocks),
            }, '|')",
        );
    assert_eq!(
        Scenario::new(source.build()).run().text(),
        "Short and plain.|json|nil|3|Short and plain."
    );
}

#[test]
fn parsed_sections_are_a_fanout_collection_and_each_arm_gets_one_as_its_item() {
    let source = PromptSource::new()
        .section(
            "Main",
            &format!(
                "
                local doc = markdown.parse('{NOTES}')
                local sections = markdown.section(doc, '/Notes').blocks
                local replies = tasks.fanout('Worker', sections)
                return tostring(replies[1]) .. '|' .. tostring(replies[2])"
            ),
        )
        .section(
            "### Worker",
            "return item.title .. ':' .. item.kind .. ':' .. #item.blocks .. ':' .. sys.index",
        );
    assert_eq!(
        Scenario::new(source.build()).run().text(),
        "Abstract:section:1:1|Data:section:1:2"
    );
}
