//! The `prompt` reads over a reader: `prompt.section` raises for a path
//! that names no section, `prompt.section_or` returns nil for it, and both
//! raise for a path the reader refuses and for a target that is not a
//! string.

use super::*;

/// The error the reader gives for a path that names no section.
const MISSING: &str = "section `Missing` not found: `/Main` has no child `Missing`";

/// The error the reader gives for the path it refuses.
const REFUSED: &str = "section path \"bad/\" has an empty step";

/// A VM whose reader knows one section, `Rules`, refuses the path `bad/`,
/// and finds no section for any other path.
fn reading_vm() -> BuiltVm {
    let vm = TestVm::new().build();
    vm.install_control_globals(|path: String, _| match path.as_str() {
        "Rules" => Ok(SectionRead::Found(SectionText {
            markdown: "Be brief.".to_owned(),
            items: Vec::new(),
        })),
        "bad/" => Err(crate::make_lua_runtime_error(REFUSED)),
        _ => Ok(SectionRead::Missing(crate::make_lua_runtime_error(MISSING))),
    })
    .expect("the control globals install");
    vm
}

/// The message `prompt.<read>(<argument>)` raises.
fn raised(vm: &BuiltVm, read: &str, argument: &str) -> String {
    vm.eval(&format!(
        "local ok, err = pcall(prompt.{read}, {argument})\n\
         assert(not ok, 'the read must raise')\n\
         return tostring(err)"
    ))
}

#[test]
fn section_returns_the_markdown_and_raises_for_a_path_that_names_no_section() {
    let vm = reading_vm();
    assert_eq!(
        vm.eval::<String>("return prompt.section('Rules')"),
        "Be brief."
    );
    let message = raised(&vm, "section", "'Missing'");
    assert!(
        message.contains(MISSING),
        "the read raises the reader's not-found error, got {message}"
    );
}

#[test]
fn section_or_returns_the_markdown_and_nil_for_a_path_that_names_no_section() {
    let vm = reading_vm();
    assert_eq!(
        vm.eval::<Option<String>>("return prompt.section_or('Rules')"),
        Some("Be brief.".to_owned())
    );
    assert_eq!(
        vm.eval::<Option<String>>("return prompt.section_or('Missing')"),
        None
    );
}

#[test]
fn both_reads_raise_for_a_refused_path_and_for_a_target_that_is_not_a_string() {
    let vm = reading_vm();
    for read in ["section", "section_or"] {
        let message = raised(&vm, read, "'bad/'");
        assert!(message.contains(REFUSED), "prompt.{read} got {message}");
        for (argument, kind) in [("42", "integer"), ("nil", "nil"), ("{}", "table")] {
            let message = raised(&vm, read, argument);
            assert!(
                message.contains(&format!("section target must be a string, got {kind}")),
                "prompt.{read}({argument}) got {message}"
            );
        }
    }
}
