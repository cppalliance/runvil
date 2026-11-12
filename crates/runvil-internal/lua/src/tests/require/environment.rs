//! A library's blocks load with the library's environment.
//!
//! Two things hold. A library's code sees the VM's globals exactly as a
//! section's own block does, through the `_G` guard: plain globals both
//! ways, `argv`, `messages`, `prose`, and a metatable the author set on
//! `_G`. And the suspending calls and the prompt reads that a library's
//! code makes say which library made them, while the library loads and
//! from a function called after the load, a tail call included.

use super::*;

/// A shim VM built from `vm` with `require` installed over `entries`,
/// rendering library prose verbatim.
#[track_caller]
fn env_vm(vm: TestVm<'_>, entries: Vec<Entry>) -> BuiltVm {
    let vm = vm.shims().build();
    install(&vm, entries);
    vm
}

/// Installs `require` over `entries` on a built VM.
#[track_caller]
fn install(vm: &BuiltVm, entries: Vec<Entry>) {
    vm.install_require(
        resolver(entries, &Calls::default()),
        |template, _, _| Ok(template.to_owned()),
        &null_emitter(),
        TEST_SECTION,
    )
    .expect("require must install");
}

/// The one library most cases load: `/kit`, under the identity `kit`.
fn kit(sources: &[&str]) -> Vec<Entry> {
    vec![("/kit", library("kit", "/kit", sources))]
}

/// Runs `source` as a block to its first yield and returns the request.
#[track_caller]
fn first_request(vm: &SectionVm, source: &str) -> Request {
    let step = vm
        .start_block_coro(&program(source))
        .expect("the block starts");
    let CoroStep::Yielded(_, values) = step else {
        panic!("the block must suspend on a request, got {step:?}");
    };
    match vm.request_from_yield(&values) {
        YieldParse::Request(request) => request,
        other => panic!("the yield must be a request, got {other:?}"),
    }
}

/// The target of a `call` or `spawn` request and the identity of the
/// library whose code made it.
#[track_caller]
fn origin(request: &Request) -> (String, Option<String>) {
    match request {
        Request::Call { target, from, .. } | Request::Spawn { target, from, .. } => (
            target.clone(),
            from.as_ref().map(|key| key.identity().to_owned()),
        ),
        other => panic!("expected a call or a spawn, got {other:?}"),
    }
}

/// `(target, Some(identity))`.
fn from(target: &str, identity: &str) -> (String, Option<String>) {
    (target.to_owned(), Some(identity.to_owned()))
}

// ---------------------------------------------------------------- globals

#[test]
fn library_code_reads_the_callers_globals_as_they_are_now() {
    let vm = env_vm(
        TestVm::new(),
        kit(&["at_load = seen\nfunction now() return seen end"]),
    );
    let value = returned(
        &vm,
        "seen = 'first'\nrequire '/kit'\nseen = 'second'\nreturn at_load .. '|' .. now()",
    );
    assert_eq!(value.as_deref(), Some("first|second"));
}

#[test]
fn a_library_global_lands_in_the_callers_globals_for_every_later_block() {
    let vm = env_vm(
        TestVm::new(),
        kit(&[
            "function greet(name) return 'hello ' .. name end\ncount = 1\n\
             function bump() count = count + 1 end",
        ]),
    );
    assert_eq!(
        returned(
            &vm,
            "require '/kit'\nbump()\ncount = count + 10\nbump()\nreturn greet('a') .. count"
        )
        .as_deref(),
        Some("hello a13")
    );
    assert_eq!(
        returned(&vm, "bump()\nreturn greet('b') .. count").as_deref(),
        Some("hello b14"),
        "a later block of the same VM sees the library's globals and its functions' writes"
    );
}

#[test]
fn library_code_reaches_every_engine_global_it_does_not_bind() {
    let vm = env_vm(
        TestVm::new(),
        kit(&["function kinds()\n\
               local names = { 'args', 'json', 'log', 'models', 'store', 'sys', 'tools', 'var',\n\
                 'untrusted', 'jump', 'require', 'pcall', 'string', 'table', 'math' }\n\
               local missing = {}\n\
               for _, name in ipairs(names) do\n\
                 if _ENV[name] == nil then missing[#missing + 1] = name end\n\
                 if _ENV[name] ~= _G[name] then missing[#missing + 1] = name .. ' differs' end\n\
               end\n\
               return table.concat(missing, ',')\n\
             end"]),
    );
    let value = returned(&vm, "require '/kit'\nreturn kinds()");
    assert_eq!(
        value.as_deref(),
        Some(""),
        "every other global is the VM's own, read through the environment"
    );
}

#[test]
fn library_code_reads_the_frozen_argv_and_cannot_assign_it() {
    let argv = json!({ "topic": "x" });
    let vm = env_vm(
        TestVm::new().argv(Argv::Frozen(Some(&argv))),
        kit(&[
            "topic_at_load = argv.topic\nfunction assign() argv = { topic = 'y' } end\n\
             function topic() return argv.topic end",
        ]),
    );
    let value = returned(
        &vm,
        "require '/kit'\nlocal ok, err = pcall(assign)\n\
         return topic_at_load .. '|' .. tostring(ok) .. '|' .. tostring(err) .. '|' .. topic()",
    )
    .expect("the block returns its text");
    assert!(
        value.starts_with("x|false|") && value.ends_with("|x"),
        "the read serves the frozen value and the write is refused, got {value}"
    );
    assert!(
        value.contains(crate::argv::ASSIGNMENT_REFUSAL),
        "the refusal is the guard's own, got {value}"
    );
}

#[test]
fn a_library_loaded_by_the_h1_pass_repairs_argv_in_the_vms_globals() {
    let vm = env_vm(
        TestVm::new().argv(Argv::Writable(None)),
        kit(&["argv = { repaired = true }"]),
    );
    let value = returned(&vm, "require '/kit'\nreturn tostring(argv.repaired)");
    assert_eq!(value.as_deref(), Some("true"));
    assert_eq!(
        vm.argv_json().expect("the repaired argv reads back"),
        Some(json!({ "repaired": true })),
        "the write went to the VM's globals, where the executor reads the repair"
    );
}

#[test]
fn library_code_reads_the_sections_messages_and_cannot_assign_them() {
    let vm = env_vm(
        TestVm::new(),
        kit(&["function same() return messages == _G.messages end\n\
               function assign() messages = {} end\n\
               function add() messages:user('from the library') return #messages end"]),
    );
    let value = returned(
        &vm,
        "require '/kit'\nlocal ok, err = pcall(assign)\n\
         return tostring(same()) .. '|' .. tostring(ok) .. '|' .. add() .. '|' .. #messages \
           .. '|' .. tostring(err)",
    )
    .expect("the block returns its text");
    assert!(
        value.starts_with("true|false|1|1|"),
        "the library reads and appends to the section's one list, got {value}"
    );
    assert!(
        value.contains(crate::messages::ASSIGNMENT_REFUSAL),
        "the refusal is the guard's own, got {value}"
    );
}

#[test]
fn library_code_cannot_assign_prose() {
    let vm = env_vm(
        TestVm::new(),
        kit(&["function assign() prose = 'mine' end"]),
    );
    let value = returned(
        &vm,
        "require '/kit'\nlocal ok, err = pcall(assign)\nreturn tostring(ok) .. '|' .. tostring(err)",
    )
    .expect("the block returns its text");
    assert!(
        value.starts_with("false|") && value.contains(crate::prose::ASSIGNMENT_REFUSAL),
        "the refusal is the guard's own, got {value}"
    );
}

#[test]
fn the_authors_strict_globals_handler_catches_an_undefined_global_in_library_code() {
    let vm = env_vm(
        TestVm::new(),
        kit(&["function touch()\n  return undefined_here\nend"]),
    );
    let value = returned(
        &vm,
        "require '/kit'\n\
         setmetatable(_G, { __index = function(_, key) error('undefined global ' .. key, 2) end })\n\
         local ok, err = pcall(touch)\n\
         return tostring(ok) .. '|' .. tostring(err)",
    );
    assert_eq!(
        value.as_deref(),
        Some("false|[string \"line 100\"]:2: undefined global undefined_here"),
        "the handler runs for library code and its level-2 position is the library's line"
    );
}

#[test]
fn the_authors_newindex_handler_receives_a_new_global_from_library_code() {
    let vm = env_vm(TestVm::new(), kit(&["function define() fresh = 1 end"]));
    let value = returned(
        &vm,
        "require '/kit'\n\
         local seen\n\
         setmetatable(_G, { __newindex = function(t, key, value)\n\
           seen = key .. '=' .. tostring(value) .. ' on _G: ' .. tostring(t == _G)\n\
         end })\n\
         define()\n\
         return seen .. '|' .. tostring(fresh)",
    );
    assert_eq!(
        value.as_deref(),
        Some("fresh=1 on _G: true|nil"),
        "the handler sees the VM's globals table, and decides the write as it does for a section"
    );
}

#[test]
fn a_library_can_set_the_metatable_of_the_vms_globals() {
    let vm = env_vm(
        TestVm::new(),
        kit(&["setmetatable(_G, { __index = function(_, key) return 'default:' .. key end })"]),
    );
    let value = returned(
        &vm,
        "require '/kit'\nreturn tostring(anything_unset) .. '|' .. type(getmetatable(_G))",
    );
    assert_eq!(value.as_deref(), Some("default:anything_unset|table"));
}

#[test]
fn a_librarys_environment_is_not_the_globals_table_and_its_metatable_is_closed() {
    let vm = env_vm(
        TestVm::new(),
        kit(&["env_is_g = (_ENV == _G)\n\
               meta = getmetatable(_ENV)\n\
               set_ok, set_err = pcall(setmetatable, _ENV, {})"]),
    );
    let value = returned(
        &vm,
        "require '/kit'\n\
         return tostring(_ENV == _G) .. '|' .. tostring(env_is_g) .. '|' .. tostring(meta) \
           .. '|' .. tostring(set_ok) .. '|' .. tostring(set_err)",
    )
    .expect("the block returns its text");
    assert!(
        value.starts_with("true|false|a library's environment is guarded|false|"),
        "a section's own environment is _G, a library's is not, got {value}"
    );
    assert!(
        value.contains("cannot change a protected metatable"),
        "library code cannot replace the environment's metatable, got {value}"
    );
}

#[test]
fn the_three_bound_names_are_the_librarys_own() {
    let vm = env_vm(
        TestVm::new(),
        kit(&[
            "lib_call, lib_tasks, lib_prompt = call, tasks, prompt\nfunction drop() call = nil end",
        ]),
    );
    let value = returned(
        &vm,
        "require '/kit'\nlocal mine = call\ndrop()\n\
         return table.concat({\n\
           tostring(lib_call ~= mine), tostring(call == mine),\n\
           tostring(lib_tasks ~= tasks), tostring(lib_tasks.spawn ~= tasks.spawn),\n\
           tostring(lib_tasks.fanout ~= tasks.fanout), tostring(lib_tasks.join == tasks.join),\n\
           tostring(lib_prompt ~= prompt), tostring(lib_prompt.section ~= prompt.section),\n\
         }, ',')",
    );
    assert_eq!(
        value.as_deref(),
        Some("true,true,true,true,true,true,true,true"),
        "a library's call, tasks.spawn, tasks.fanout, and prompt are bound copies; dropping one \
         inside the library leaves the caller's"
    );
}

#[test]
fn a_callers_rebound_call_is_not_what_library_code_calls() {
    // A library's `call` is its own, so a section's replacement of the
    // global is not what the library's code calls.
    let vm = env_vm(
        TestVm::new(),
        kit(&["function which() return call == wrapper end"]),
    );
    let value = returned(
        &vm,
        "require '/kit'\nwrapper = function() end\ncall = wrapper\nreturn tostring(which())",
    );
    assert_eq!(value.as_deref(), Some("false"));
}

// ------------------------------------------------------- what a yield says

#[test]
fn a_sections_own_call_names_no_library() {
    let vm = env_vm(TestVm::new(), kit(&["return true"]));
    let request = first_request(&vm, "require '/kit'\nreturn call('x')");
    assert_eq!(origin(&request), ("x".to_owned(), None));
}

#[test]
fn a_function_the_caller_passes_to_a_library_keeps_the_callers_place() {
    // A function keeps the place it was written in: a callback written in
    // the section names the section's neighbors, even when a library's
    // code is what calls it.
    let vm = env_vm(
        TestVm::new(),
        kit(&["function each(f) local value = f() return value end"]),
    );
    let request = first_request(
        &vm,
        "require '/kit'\nreturn each(function() return call('mine') end)",
    );
    assert_eq!(origin(&request), ("mine".to_owned(), None));
}

#[test]
fn a_call_made_while_a_library_loads_names_the_library() {
    let vm = env_vm(TestVm::new(), kit(&["answer = call('x')"]));
    let request = first_request(&vm, "require '/kit'");
    assert_eq!(origin(&request), from("x", "kit"));
}

#[test]
fn a_library_function_called_after_the_load_names_the_library_in_a_tail_call_too() {
    for body in [
        "function later() local value = call('x') return value end",
        "function later() return call('x') end",
        "local go = call\nfunction later() return go('x') end",
        "function later() local ok, value = pcall(call, 'x') return value end",
    ] {
        let vm = env_vm(TestVm::new(), kit(&[body]));
        let request = first_request(&vm, "require '/kit'\nreturn later()");
        assert_eq!(origin(&request), from("x", "kit"), "{body}");
    }
}

#[test]
fn a_bound_call_handed_to_the_caller_still_names_the_library() {
    let vm = env_vm(TestVm::new(), kit(&["return { call = call }"]));
    let request = first_request(&vm, "local kit = require '/kit'\nreturn kit.call('x')");
    assert_eq!(origin(&request), from("x", "kit"));
}

#[test]
fn a_name_the_caller_passes_in_is_resolved_where_it_is_used() {
    let vm = env_vm(
        TestVm::new(),
        kit(&["function run(name) return call(name) end"]),
    );
    let request = first_request(&vm, "require '/kit'\nreturn run('mine')");
    assert_eq!(origin(&request), from("mine", "kit"));
}

#[test]
fn a_library_fanout_and_spawn_name_the_library_in_every_spawn() {
    let vm = env_vm(
        TestVm::new(),
        kit(&["function run(items) return tasks.fanout('Worker', items) end"]),
    );
    let request = first_request(&vm, "require '/kit'\nreturn run({ 'a', 'b' })");
    assert_eq!(origin(&request), from("Worker", "kit"));
    assert!(
        matches!(&request, Request::Spawn { item: Some(item), index: Some(1), .. } if item == "a"),
        "the first arm's spawn carries its item and index as before, got {request:?}"
    );

    let vm = env_vm(
        TestVm::new(),
        kit(&["function start() return tasks.spawn('Worker', { item = 'x' }) end"]),
    );
    let request = first_request(&vm, "require '/kit'\nreturn start()");
    assert_eq!(origin(&request), from("Worker", "kit"));

    let vm = env_vm(TestVm::new(), kit(&["return true"]));
    let request = first_request(
        &vm,
        "require '/kit'\nreturn tasks.fanout('Worker', { 'a' })",
    );
    assert_eq!(
        origin(&request),
        ("Worker".to_owned(), None),
        "the section's own fanout names no library"
    );
}

#[test]
fn each_library_names_itself_when_one_library_requires_another() {
    let entries = || {
        vec![
            (
                "/a",
                library(
                    "a",
                    "/a",
                    &[
                        "local b = require '/b'\nvia_b = b.go\nfunction via_a() return call('x') end",
                    ],
                ),
            ),
            (
                "/b",
                library(
                    "b",
                    "/b",
                    &["return { go = function() return call('x') end }"],
                ),
            ),
        ]
    };
    let vm = env_vm(TestVm::new(), entries());
    assert_eq!(
        origin(&first_request(&vm, "require '/a'\nreturn via_a()")),
        from("x", "a")
    );
    let vm = env_vm(TestVm::new(), entries());
    assert_eq!(
        origin(&first_request(&vm, "require '/a'\nreturn via_b()")),
        from("x", "b"),
        "a function keeps the environment of the library it was written in"
    );
}

#[test]
fn a_library_call_still_suspends_and_resumes_the_callers_block() {
    let vm = env_vm(
        TestVm::new(),
        kit(&["function ask() return 'kit got ' .. call('worker', 'job') end"]),
    );
    let block = program("require '/kit'\nreturn ask()");
    let CoroStep::Yielded(thread, values) = vm.start_block_coro(&block).expect("the block starts")
    else {
        panic!("the library function's call must suspend the caller's block");
    };
    assert!(matches!(
        vm.request_from_yield(&values),
        YieldParse::Request(Request::Call { ref input, .. }) if input.as_deref() == Some("job")
    ));
    let step = vm
        .resume_block_coro(&block, &thread, (true, "hi"))
        .expect("resuming finishes the block");
    assert!(
        matches!(step, CoroStep::Done(LuaBlockResult::Returned(Some(ref text))) if text == "kit got hi"),
        "the answer resumes inside the library function, got {step:?}"
    );
}

// ------------------------------------------------------------ prompt reads

/// Every read so far: the path as written, and the identity of the
/// library whose code made the read.
type Reads = Arc<Mutex<Vec<(String, Option<String>)>>>;

#[test]
fn a_prompt_read_says_which_library_made_it() {
    let reads = Reads::default();
    let log = Arc::clone(&reads);
    let vm = TestVm::new()
        .reading(move |path, from| {
            let from = from.map(|key| key.identity().to_owned());
            let reader = from.as_deref().unwrap_or("the section");
            let found = if path == "Missing" {
                SectionRead::Missing(crate::make_lua_runtime_error(format!(
                    "no `{path}` for {reader}"
                )))
            } else {
                let markdown = format!("{path} for {reader}");
                SectionRead::Found(SectionText::new(
                    Arc::from(markdown.as_str()),
                    0..markdown.len(),
                    NonZeroU32::MIN,
                ))
            };
            log.lock()
                .expect("the reads lock must not be poisoned")
                .push((path, from));
            found
        })
        .build();
    install(
        &vm,
        kit(&["at_load = prompt.section('T')\n\
               function later() return prompt.section_or('T') .. '+' .. prompt.section('L', 'prose').text end\n\
               function users() return _G.prompt.section('T') end\n\
               function miss() return select(2, pcall(prompt.section, 'Missing')) end\n\
               function users_miss() return select(2, pcall(_G.prompt.section, 'Missing')) end"]),
    );
    let value = returned(
        &vm,
        "local own = prompt.section('T')\nrequire '/kit'\n\
         return own .. '|' .. at_load .. '|' .. later() .. '|' .. users()",
    );
    assert_eq!(
        value.as_deref(),
        Some("T for the section|T for kit|T for kit+L for kit|T for the section")
    );
    assert_eq!(
        *reads.lock().expect("the reads lock must not be poisoned"),
        [
            ("T".to_owned(), None),
            ("T".to_owned(), Some("kit".to_owned())),
            ("T".to_owned(), Some("kit".to_owned())),
            ("L".to_owned(), Some("kit".to_owned())),
            ("T".to_owned(), None),
        ],
        "the section's own read and a read through _G.prompt name no library"
    );
    let misses = returned(
        &vm,
        "return tostring(miss()) .. '|' .. tostring(users_miss())",
    )
    .expect("the block returns its text");
    assert!(
        misses.contains("no `Missing` for kit") && misses.contains("no `Missing` for the section"),
        "a miss in a library's code is the library's, and one through _G is the section's, got \
         {misses}"
    );
}
