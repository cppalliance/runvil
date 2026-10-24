//! Tests for the offer verbs' array length bound: an array longer than
//! the held key limit is refused before any entry is read.

use crate::test_vm::{lua_over, offer_set, offers, raised};

#[test]
fn a_len_of_maxinteger_is_refused_and_the_vm_still_offers() {
    let vm = lua_over(offer_set());
    let error = raised(
        &vm,
        "tools.offer(setmetatable({}, { __len = function() return math.maxinteger end }))",
    );
    assert!(
        error.contains(
            "tools.offer takes an array of at most 100000 entries, got 9223372036854775807"
        ),
        "the refusal names the bound and the reported length: {error}"
    );
    vm.exec("tools.offer('web')");
    assert_eq!(
        offers(&vm),
        ["static:web"],
        "the refused offer recorded nothing and the verb still works"
    );
}

#[test]
fn an_array_past_the_reference_stack_cap_is_refused() {
    let vm = lua_over(offer_set());
    let error = raised(
        &vm,
        "local t = {} for i = 1, 1200000 do t[i] = 'web' end tools.offer(t)",
    );
    assert!(
        error.contains("got 1200000"),
        "an array too long to hold one reference per entry is refused: {error}"
    );
    assert!(offers(&vm).is_empty(), "a refusal records nothing");
}

#[test]
fn an_array_of_exactly_the_limit_is_accepted() {
    let vm = lua_over(offer_set());
    vm.exec("local t = {} for i = 1, 100000 do t[i] = 'web' end tools.offer(t)");
    assert_eq!(
        offers(&vm),
        ["static:web"],
        "an array at the bound is read whole"
    );
}

#[test]
fn offer_dynamic_refuses_an_array_one_past_the_limit() {
    let vm = lua_over(offer_set());
    let error = raised(
        &vm,
        "local t = {} for i = 1, 100001 do t[i] = 'web' end tools.offer_dynamic(t)",
    );
    assert!(
        error.contains("tools.offer_dynamic takes an array of at most 100000 entries, got 100001"),
        "the bound applies to both verbs and names the verb: {error}"
    );
    assert!(offers(&vm).is_empty(), "a refusal records nothing");
}

#[test]
fn a_len_too_large_to_allocate_is_refused() {
    let vm = lua_over(offer_set());
    let error = raised(
        &vm,
        "tools.offer(setmetatable({}, { __len = function() return 1 << 40 end }))",
    );
    assert!(
        error.contains("tools.offer takes an array of at most 100000 entries, got 1099511627776"),
        "a length that would abort on allocation is refused first: {error}"
    );
}
