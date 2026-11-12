//! A list in a section: the parser gives it no meaning of its own.

use super::*;

#[test]
fn a_section_that_holds_only_a_list_is_an_ordinary_section() {
    let body = "- alpha\n- beta\n3. gamma";
    let prompt = parse_ok(&PromptSource::new().prose("S", body).build());
    let section = &prompt.sections[0];
    assert_eq!(section.blocks().len(), 1, "the list is one prose block");
    assert_eq!(section.prose(), body);
    assert_eq!(section.markdown(), body);
}

#[test]
fn a_bullet_with_no_content_parses() {
    for body in ["- alpha\n1.\n- beta", "-", "- alpha\n-\n- gamma"] {
        let prompt = parse_ok(&PromptSource::new().prose("S", body).build());
        assert_eq!(prompt.sections[0].markdown(), body);
    }
}

#[test]
fn a_list_beside_a_lua_fence_stays_in_the_prose_before_it() {
    let worker = "- one\n- two\n\n```lua\nreturn item\n```";
    let src = PromptSource::new().prose("Parent", "p");
    let prompt = parse_ok(&src.prose("### Worker", worker).build());
    let worker = &prompt.sections[0].children[0];
    assert_eq!(worker.name, "Worker");
    assert_eq!(worker.blocks().len(), 2);
    assert_eq!(worker.prose(), "- one\n- two");
}
