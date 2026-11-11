//! Tests for the file functions of a tool call's step: what each hands
//! back from the call's access, how a `VfsError` reads to a Plugin, that a
//! failed file function leaves the call running, and what each is refused
//! for as a misuse.

use std::io;
use std::sync::atomic::Ordering;

use runvil_plugin::{FileType, PathReason, Stat, VfsError};
use runvil_plugin_sys::{
    RV_FILE_ERROR_CONFLICT, RV_FILE_ERROR_DENIED, RV_FILE_ERROR_IO, RV_FILE_TYPE_OTHER,
    RV_FILE_TYPE_SYMLINK,
};
use serde_json::json;

use super::{Lent, describe, raw_stat};
use crate::fakes::*;

/// The files every call below reads.
const FILES: &[(&str, &str)] = &[("/notes/a.md", "hello"), ("/notes/b.md", "")];

pack_fake!(
    reader,
    {},
    new: |call| {
        set_pack_run(call, instance(PACK_TAG));
        add_tool(call, "stat");
        add_tool(call, "list");
        add_tool(call, "read");
        add_tool(call, "tail")
    },
    step: |call| {
        let args = step_args(call);
        // The arguments are one JSON string, the path.
        let path: String = serde_json::from_str(&text(args.arguments)).unwrap_or_default();
        let said = match text(args.tool).as_str() {
            "stat" => format!("{:?}", stat_of(call, &path)),
            "list" => format!("{:?}", list_of(call, &path)),
            "read" => format!("{:?}", read_of(call, &path, (0, u64::MAX))),
            _ => format!("{:?}", read_of(call, &path, (3, 100))),
        };
        finish(call, RV_RESULT_TRUSTED, &said)
    }
);

/// What the `reader` Pack's tool `tool` says of `path` over [`FILES`].
fn said(tool: &str, path: &str) -> String {
    let pack = made(reader::entry);
    let call = seeded_call(tool, FILES);
    block_on(pack.call(call.context(), json!(path)))
        .expect("the call succeeds")
        .text()
        .to_owned()
}

/// The text a fake's helper formats for a failure of `kind` that says
/// `text`.
fn failed(kind: u32, text: &str) -> String {
    format!("Err(({kind}, {text:?}))")
}

#[test]
fn file_stat_says_what_a_path_names() {
    assert_eq!(
        said("stat", "/notes/a.md"),
        format!("Ok(({RV_FILE_TYPE_FILE}, 5))")
    );
    assert!(
        said("stat", "/notes").starts_with(&format!("Ok(({RV_FILE_TYPE_DIRECTORY}, ")),
        "{}",
        said("stat", "/notes")
    );
}

#[test]
fn file_list_hands_back_each_entry_with_its_name_and_metadata() {
    assert_eq!(
        said("list", "/notes"),
        format!("Ok([(\"a.md\", {RV_FILE_TYPE_FILE}, 5), (\"b.md\", {RV_FILE_TYPE_FILE}, 0)])")
    );
}

#[test]
fn file_read_hands_back_the_whole_file_or_the_bytes_inside_a_range() {
    assert_eq!(said("read", "/notes/a.md"), format!("Ok({:?})", b"hello"));
    assert_eq!(said("read", "/notes/b.md"), "Ok([])");
    assert_eq!(said("tail", "/notes/a.md"), format!("Ok({:?})", b"lo"));
}

#[test]
fn a_path_without_a_leading_slash_is_taken_from_the_root() {
    assert_eq!(said("read", "notes/a.md"), format!("Ok({:?})", b"hello"));
}

#[test]
fn a_failed_file_function_fills_the_error_and_leaves_the_call_running() {
    assert_eq!(
        said("stat", "/absent"),
        failed(RV_FILE_ERROR_NOT_FOUND, "not found: /absent")
    );
    assert_eq!(
        said("list", "/notes/a.md"),
        failed(
            RV_FILE_ERROR_NOT_A_DIRECTORY,
            "not a directory: /notes/a.md"
        )
    );
    assert_eq!(
        said("read", "/notes"),
        failed(RV_FILE_ERROR_IS_A_DIRECTORY, "is a directory: /notes")
    );
    assert_eq!(
        said("stat", ""),
        failed(RV_FILE_ERROR_OTHER, "path is empty")
    );
}

#[test]
fn a_path_of_exactly_4096_bytes_is_within_the_bound() {
    let longest = format!("/{}", "a".repeat(4095));
    assert_eq!(
        said("stat", &longest),
        failed(RV_FILE_ERROR_NOT_FOUND, &format!("not found: {longest}"))
    );
}

pack_fake!(
    twice,
    {},
    new: |call| {
        set_pack_run(call, instance(PACK_TAG));
        add_tool(call, "twice")
    },
    step: |call| {
        let read = read_of(call, "/notes/a.md", (0, u64::MAX));
        if tool_call(call).is_null() {
            set_tool_call(call, instance(0x300));
            return if read.is_ok() { RV_PENDING } else { ODD_STATUS };
        }
        finish(call, RV_RESULT_TRUSTED, &format!("{read:?}"))
    }
);

#[test]
fn each_step_of_a_call_reads_through_its_own_ctx() {
    let pack = made(twice::entry);
    let call = seeded_call("twice", FILES);
    let output = block_on(pack.call(call.context(), json!({}))).expect("the call succeeds");
    assert_eq!(output.text(), format!("Ok({:?})", b"hello"));
}

#[test]
fn each_kind_of_node_keeps_its_number_and_any_other_kind_is_other() {
    for (file_type, raw) in [
        (FileType::File, RV_FILE_TYPE_FILE),
        (FileType::Directory, RV_FILE_TYPE_DIRECTORY),
        (FileType::Symlink, RV_FILE_TYPE_SYMLINK),
        (FileType::Fifo, RV_FILE_TYPE_OTHER),
        (FileType::Socket, RV_FILE_TYPE_OTHER),
        (FileType::CharDevice, RV_FILE_TYPE_OTHER),
        (FileType::BlockDevice, RV_FILE_TYPE_OTHER),
    ] {
        let stat = raw_stat(&Stat::new(file_type, 7));
        assert_eq!(
            (stat.file_type, stat.reserved, stat.len),
            (raw, 0, 7),
            "{file_type:?}"
        );
    }
}

#[test]
fn each_error_reads_as_its_kind_and_a_text_without_a_label() {
    let path = || "/x".to_owned();
    for (error, kind, text) in [
        (
            VfsError::NotFound { path: path() },
            RV_FILE_ERROR_NOT_FOUND,
            "not found: /x",
        ),
        (
            VfsError::NotADirectory { path: path() },
            RV_FILE_ERROR_NOT_A_DIRECTORY,
            "not a directory: /x",
        ),
        (
            VfsError::IsADirectory { path: path() },
            RV_FILE_ERROR_IS_A_DIRECTORY,
            "is a directory: /x",
        ),
        (
            VfsError::PermissionDenied {
                path: path(),
                reason: "the mode allows only markdown".to_owned(),
            },
            RV_FILE_ERROR_DENIED,
            "the mode allows only markdown",
        ),
        (
            VfsError::ApprovalRequired {
                path: path(),
                reason: "reading /x needs approval".to_owned(),
            },
            RV_FILE_ERROR_DENIED,
            "reading /x needs approval",
        ),
        (
            VfsError::Conflict {
                path: path(),
                detail: "a write by another call".to_owned(),
            },
            RV_FILE_ERROR_CONFLICT,
            "conflicting claim: a write by another call",
        ),
        (
            VfsError::Io {
                path: path(),
                kind: io::ErrorKind::PermissionDenied,
                raw_os_error: Some(13),
                message: "access denied".to_owned(),
            },
            RV_FILE_ERROR_IO,
            "permission denied: access denied",
        ),
        (
            VfsError::InvalidPath {
                path: String::new(),
                reason: PathReason::Empty,
            },
            RV_FILE_ERROR_OTHER,
            "path is empty",
        ),
        (
            VfsError::ScopeEnded { path: path() },
            RV_FILE_ERROR_OTHER,
            "the run or tool call that owned this access has ended, so /x can no longer be \
             touched through it",
        ),
        (
            VfsError::Backend {
                message: "the backend is gone".to_owned(),
            },
            RV_FILE_ERROR_OTHER,
            "the backend is gone",
        ),
        (
            VfsError::NoStore,
            RV_FILE_ERROR_OTHER,
            "the handle declares no store",
        ),
    ] {
        assert_eq!(describe(&error), (kind, text.to_owned()), "{error:?}");
    }
}

#[test]
fn each_file_function_keeps_only_what_it_hands_back() {
    let mut lent = Lent::default();
    let bytes = lent.bytes(b"hello".to_vec());
    assert_eq!((bytes.len, lent.bytes.len()), (5, 5));

    let failure = lent.error(&VfsError::NotFound {
        path: "/x".to_owned(),
    });
    assert_eq!(
        (failure.kind, failure.reserved, text(failure.text)),
        (RV_FILE_ERROR_NOT_FOUND, 0, "not found: /x".to_owned())
    );
    assert!(lent.bytes.is_empty(), "the bytes of the read are gone");

    let listed = lent.entries(vec![runvil_plugin::Entry::new(
        "a.md",
        Stat::new(FileType::File, 5),
    )]);
    assert_eq!((listed.len, lent.entries.len()), (1, 1));
    assert!(lent.text.is_empty(), "the text of the failure is gone");
}

/// The message of a call that failed because its Plugin misused it.
macro_rules! misuse_reason {
    ($detail:expr) => {
        concat!("the tool's Plugin misused the tool call: ", $detail)
    };
}

/// Defines a test whose fake step runs `$step`, which calls a file
/// function wrongly and returns the step's status. The call must fail as
/// a `ToolOther` error that names the misuse `$detail`.
macro_rules! file_misuse_test {
    ($test:ident, $detail:expr, |$call:ident| $step:expr) => {
        #[test]
        fn $test() {
            pack_fake!(
                plug,
                {},
                new: |call| {
                    set_pack_run(call, instance(PACK_TAG));
                    add_tool(call, "tool")
                },
                step: |$call| $step
            );
            let pack = made(plug::entry);
            let call = seeded_call("tool", FILES);
            let failure = block_on(pack.call(call.context(), json!({})))
                .expect_err("the call fails");
            assert_eq!(failure.code, runvil_plugin::ErrorCode::ToolOther);
            assert_eq!(failure.message, misuse_reason!($detail));
        }
    };
}

file_misuse_test!(
    a_path_over_4096_bytes_is_misuse,
    "the path is longer than 4096 bytes",
    |call| {
        let (mut stat, mut error) = (unset_stat(), unset_error());
        // The length is refused before any byte is read, so the text
        // behind it need not be that long.
        let long = RvStr {
            ptr: "x".as_ptr().cast(),
            len: 4097,
        };
        stat_as(
            call,
            step_args(call).ctx,
            long,
            &raw mut stat,
            &raw mut error,
        )
    }
);

file_misuse_test!(
    a_path_with_a_null_pointer_and_a_length_is_misuse,
    "the path has a NULL pointer and a nonzero length",
    |call| {
        let (mut stat, mut error) = (unset_stat(), unset_error());
        let null = RvStr {
            ptr: ptr::null(),
            len: 4,
        };
        stat_as(
            call,
            step_args(call).ctx,
            null,
            &raw mut stat,
            &raw mut error,
        )
    }
);

file_misuse_test!(
    a_path_that_is_not_utf8_is_misuse,
    "the path is not UTF-8",
    |call| {
        let (mut stat, mut error) = (unset_stat(), unset_error());
        let path = rv_str_bytes(&[0xff, 0xfe]);
        stat_as(
            call,
            step_args(call).ctx,
            path,
            &raw mut stat,
            &raw mut error,
        )
    }
);

file_misuse_test!(
    a_file_function_through_another_ctx_is_misuse,
    "a tool call function was handed a ctx other than the open RV_TOOL_CALL step's",
    |call| {
        let (mut stat, mut error) = (unset_stat(), unset_error());
        stat_as(
            call,
            ptr::null_mut(),
            rv_str("/notes/a.md"),
            &raw mut stat,
            &raw mut error,
        )
    }
);

file_misuse_test!(
    a_null_stat_is_misuse,
    "file_stat was handed a NULL stat pointer",
    |call| {
        let mut error = unset_error();
        stat_as(
            call,
            step_args(call).ctx,
            rv_str("/notes/a.md"),
            ptr::null_mut(),
            &raw mut error,
        )
    }
);

file_misuse_test!(
    a_misaligned_stat_is_misuse,
    "file_stat was handed a misaligned stat pointer",
    |call| {
        let mut error = unset_error();
        let odd = ptr::without_provenance_mut(1);
        stat_as(
            call,
            step_args(call).ctx,
            rv_str("/notes/a.md"),
            odd,
            &raw mut error,
        )
    }
);

file_misuse_test!(
    a_null_error_of_a_stat_is_misuse,
    "file_stat was handed a NULL error pointer",
    |call| {
        let mut stat = unset_stat();
        stat_as(
            call,
            step_args(call).ctx,
            rv_str("/notes/a.md"),
            &raw mut stat,
            ptr::null_mut(),
        )
    }
);

file_misuse_test!(
    a_misaligned_error_of_a_stat_is_misuse,
    "file_stat was handed a misaligned error pointer",
    |call| {
        let mut stat = unset_stat();
        let odd = ptr::without_provenance_mut(1);
        stat_as(
            call,
            step_args(call).ctx,
            rv_str("/notes/a.md"),
            &raw mut stat,
            odd,
        )
    }
);

file_misuse_test!(
    a_null_entries_is_misuse,
    "file_list was handed a NULL entries pointer",
    |call| {
        let mut error = unset_error();
        list_as(
            call,
            step_args(call).ctx,
            rv_str("/notes"),
            ptr::null_mut(),
            &raw mut error,
        )
    }
);

file_misuse_test!(
    a_misaligned_entries_is_misuse,
    "file_list was handed a misaligned entries pointer",
    |call| {
        let mut error = unset_error();
        let odd = ptr::without_provenance_mut(1);
        list_as(
            call,
            step_args(call).ctx,
            rv_str("/notes"),
            odd,
            &raw mut error,
        )
    }
);

file_misuse_test!(
    a_null_error_of_a_list_is_misuse,
    "file_list was handed a NULL error pointer",
    |call| {
        let mut entries = unset_entries();
        list_as(
            call,
            step_args(call).ctx,
            rv_str("/notes"),
            &raw mut entries,
            ptr::null_mut(),
        )
    }
);

file_misuse_test!(
    a_misaligned_error_of_a_list_is_misuse,
    "file_list was handed a misaligned error pointer",
    |call| {
        let mut entries = unset_entries();
        let odd = ptr::without_provenance_mut(1);
        list_as(
            call,
            step_args(call).ctx,
            rv_str("/notes"),
            &raw mut entries,
            odd,
        )
    }
);

file_misuse_test!(
    a_null_bytes_is_misuse,
    "file_read was handed a NULL bytes pointer",
    |call| {
        let mut error = unset_error();
        read_as(
            call,
            step_args(call).ctx,
            rv_str("/notes/a.md"),
            (0, 1),
            ptr::null_mut(),
            &raw mut error,
        )
    }
);

file_misuse_test!(
    a_misaligned_bytes_is_misuse,
    "file_read was handed a misaligned bytes pointer",
    |call| {
        let mut error = unset_error();
        let odd = ptr::without_provenance_mut(1);
        read_as(
            call,
            step_args(call).ctx,
            rv_str("/notes/a.md"),
            (0, 1),
            odd,
            &raw mut error,
        )
    }
);

file_misuse_test!(
    a_null_error_of_a_read_is_misuse,
    "file_read was handed a NULL error pointer",
    |call| {
        let mut bytes = unset_bytes();
        read_as(
            call,
            step_args(call).ctx,
            rv_str("/notes/a.md"),
            (0, 1),
            &raw mut bytes,
            ptr::null_mut(),
        )
    }
);

file_misuse_test!(
    a_misaligned_error_of_a_read_is_misuse,
    "file_read was handed a misaligned error pointer",
    |call| {
        let mut bytes = unset_bytes();
        let odd = ptr::without_provenance_mut(1);
        read_as(
            call,
            step_args(call).ctx,
            rv_str("/notes/a.md"),
            (0, 1),
            &raw mut bytes,
            odd,
        )
    }
);

#[test]
fn a_misuse_writes_neither_the_result_nor_the_error() {
    pack_fake!(
        plug,
        {
            /// The file type and the error kind after a refused call.
            pub(super) static LEFT: AtomicUsize = AtomicUsize::new(0);
        },
        new: |call| {
            set_pack_run(call, instance(PACK_TAG));
            add_tool(call, "tool")
        },
        step: |call| {
            let (mut stat, mut error) = (unset_stat(), unset_error());
            let status =
                stat_as(call, ptr::null_mut(), rv_str("/notes/a.md"), &raw mut stat, &raw mut error);
            LEFT.store((stat.file_type + error.kind) as usize, Ordering::SeqCst);
            status
        }
    );
    let pack = made(plug::entry);
    let call = seeded_call("tool", FILES);
    block_on(pack.call(call.context(), json!({}))).expect_err("the call fails");
    assert_eq!(plug::LEFT.load(Ordering::SeqCst), 18, "both still hold 9");
}

#[test]
fn a_file_function_called_while_a_pack_instance_is_made_is_misuse() {
    pack_fake!(
        plug,
        {},
        new: |call| {
            let (mut stat, mut error) = (unset_stat(), unset_error());
            // SAFETY: the function refuses a call that is not a step
            // before it reads or writes through any pointer.
            unsafe {
                crate::file::file_stat(
                    pack_args(call).ctx,
                    rv_str("/notes/a.md"),
                    &raw mut stat,
                    &raw mut error,
                )
            }
        },
        step: |_call| RV_OK
    );
    let failure = new_pack(plug::entry)
        .err()
        .expect("the instance is not made");
    assert_eq!(
        failure.message,
        "the Plugin fake misused the Pack functions: a tool call function was called outside \
         RV_TOOL_CALL"
    );
}

#[test]
fn a_late_file_function_through_a_stashed_ctx_is_misuse_and_writes_nothing() {
    pack_fake!(
        plug,
        {
            pub(super) static STEP_CTX: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
        },
        new: |call| {
            set_pack_run(call, instance(PACK_TAG));
            add_tool(call, "tool")
        },
        step: |call| {
            STEP_CTX.store(step_args(call).ctx, Ordering::SeqCst);
            finish(call, RV_RESULT_TRUSTED, "done")
        }
    );
    let pack = made(plug::entry);
    let call = seeded_call("tool", FILES);
    block_on(pack.call(call.context(), json!({}))).expect("the call succeeds");
    let stashed = plug::STEP_CTX.load(Ordering::SeqCst);
    let (mut stat, mut error) = (unset_stat(), unset_error());
    // SAFETY: the loader's function compares a `ctx` with the open call's
    // before it reads through it, and no call is open on this thread.
    let status = unsafe {
        crate::file::file_stat(
            stashed,
            rv_str("/notes/a.md"),
            &raw mut stat,
            &raw mut error,
        )
    };
    assert_eq!(status, RV_ERR_MISUSE);
    assert_eq!((stat.file_type, error.kind), (9, 9));
}
