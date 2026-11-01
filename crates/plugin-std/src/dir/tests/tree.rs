//! The shape of the tree ListDir answers with: the layout, the order, what
//! is skipped, how a link is listed, and what each failure says.

use runvil_plugin::{Entry, FileType, OutputTrust, Stat, VfsError};
use serde_json::json;

use super::{failure_of, list, new_call, seeded, text_of};
use crate::dir::check_directory;
use crate::dir::render::render;
use crate::dir::walk::{Failure, walk};
use crate::refusal::refusal;

/// The listing ListDir answers for `target`.
fn tree(files: &[&str], target: &str) -> String {
    text_of(list(&seeded(files), &json!({ "target_directory": target })))
}

/// A directory entry of kind `file_type`.
fn entry(name: &str, file_type: FileType) -> Entry {
    Entry::new(name, Stat::new(file_type, 0))
}

#[test]
fn the_posix_tree_lists_every_level_in_byte_order() {
    // The parent report shows `package.json` before `README.md`, because
    // Cursor sorts with `localeCompare`. Byte order governs here, so
    // `README.md` comes first.
    let text = tree(
        &[
            "/home/dev/proj/README.md",
            "/home/dev/proj/package.json",
            "/home/dev/proj/docs/guide.md",
            "/home/dev/proj/src/app.tsx",
            "/home/dev/proj/src/index.ts",
            "/home/dev/proj/src/utils/math.ts",
        ],
        "/home/dev/proj",
    );
    let expected = "\
/home/dev/proj/
  - README.md
  - docs/
    - guide.md
  - package.json
  - src/
    - app.tsx
    - index.ts
    - utils/
      - math.ts";
    assert_eq!(text, expected);
}

#[test]
fn the_output_has_no_trailing_newline() {
    let text = tree(&["/p/a.md"], "/p");
    assert_eq!(text, "/p/\n  - a.md");
    assert!(!text.ends_with('\n'));
}

#[test]
fn an_empty_directory_says_it_has_no_children() {
    let call = new_call();
    call.context()
        .access()
        .mkdir("/home/dev/proj/empty", true)
        .expect("the directory is made");
    assert_eq!(
        text_of(list(
            &call,
            &json!({ "target_directory": "/home/dev/proj/empty" })
        )),
        "/home/dev/proj/empty/\n... no children found ..."
    );
}

#[test]
fn a_root_holding_only_dot_git_says_it_has_no_children() {
    let call = seeded(&["/repo/.git/HEAD", "/repo/.git/objects/ab"]);
    assert_eq!(
        text_of(list(&call, &json!({ "target_directory": "/repo" }))),
        "/repo/\n... no children found ..."
    );
}

#[test]
fn a_root_whose_children_are_all_ignored_says_it_has_no_children() {
    let call = seeded(&["/p/a.log", "/p/b.log"]);
    assert_eq!(
        text_of(list(
            &call,
            &json!({ "target_directory": "/p", "ignore_globs": ["*.log"] })
        )),
        "/p/\n... no children found ..."
    );
}

#[test]
fn a_directory_that_is_empty_below_the_root_is_still_listed() {
    let call = seeded(&["/p/a.md"]);
    call.context()
        .access()
        .mkdir("/p/empty", false)
        .expect("the directory is made");
    assert_eq!(
        text_of(list(&call, &json!({ "target_directory": "/p" }))),
        "/p/\n  - a.md\n  - empty/"
    );
}

#[test]
fn a_file_as_the_target_is_not_a_directory() {
    let call = seeded(&["/home/dev/proj/README.md"]);
    assert_eq!(
        failure_of(list(
            &call,
            &json!({ "target_directory": "/home/dev/proj/README.md" })
        )),
        "Path is not a directory: /home/dev/proj/README.md"
    );
}

#[test]
fn a_missing_path_does_not_exist() {
    let call = new_call();
    assert_eq!(
        failure_of(list(&call, &json!({ "target_directory": "/nope" }))),
        "Path does not exist: /nope"
    );
}

#[test]
fn a_failure_echoes_the_path_as_the_model_wrote_it() {
    let call = seeded(&["/notes/a.md"]);
    assert_eq!(
        failure_of(list(&call, &json!({ "target_directory": "nope" }))),
        "Path does not exist: nope"
    );
    assert_eq!(
        failure_of(list(&call, &json!({ "target_directory": "notes/a.md" }))),
        "Path is not a directory: notes/a.md"
    );
}

#[test]
fn a_blank_target_is_refused_before_any_io() {
    let call = new_call();
    for blank in ["", "  ", "\t\n"] {
        assert_eq!(
            failure_of(list(&call, &json!({ "target_directory": blank }))),
            format!(
                "std_ListDir: `target_directory` is empty or only whitespace, \
                 and the call passed \"{blank}\""
            )
        );
    }
}

#[test]
fn the_file_type_decision_covers_every_kind_of_node() {
    assert_eq!(check_directory("/p", FileType::Directory), Ok(()));
    assert_eq!(check_directory("/p", FileType::Symlink), Ok(()));
    for other in [
        FileType::File,
        FileType::Fifo,
        FileType::Socket,
        FileType::CharDevice,
        FileType::BlockDevice,
    ] {
        assert_eq!(
            check_directory("/p", other),
            Err("Path is not a directory: /p".to_owned()),
            "{other:?}"
        );
    }
}

#[test]
fn a_relative_target_gets_a_leading_slash_on_the_root_line() {
    let call = seeded(&["/home/dev/a.md"]);
    assert_eq!(
        text_of(list(&call, &json!({ "target_directory": "home/dev" }))),
        "/home/dev/\n  - a.md"
    );
}

#[test]
fn a_trailing_slash_on_the_target_is_not_doubled() {
    let call = seeded(&["/home/dev/a.md"]);
    assert_eq!(
        text_of(list(&call, &json!({ "target_directory": "/home/dev/" }))),
        "/home/dev/\n  - a.md"
    );
}

#[test]
fn the_filesystem_root_is_a_single_slash() {
    let call = seeded(&["/a.md", "/b/c.md"]);
    assert_eq!(
        text_of(list(&call, &json!({ "target_directory": "/" }))),
        "/\n  - a.md\n  - b/\n    - c.md"
    );
}

#[test]
fn dot_git_is_skipped_while_other_dotfiles_are_listed() {
    let call = seeded(&[
        "/p/.git/HEAD",
        "/p/.env",
        "/p/.gitignore",
        "/p/src/.hidden",
        "/p/src/.git/config",
        "/p/src/a.rs",
    ]);
    let expected = "\
/p/
  - .env
  - .gitignore
  - src/
    - .hidden
    - a.rs";
    assert_eq!(
        text_of(list(&call, &json!({ "target_directory": "/p" }))),
        expected
    );
}

#[test]
fn a_dot_git_that_is_a_file_is_skipped_too() {
    let call = seeded(&["/p/.git", "/p/a.md", "/p/sub/.git"]);
    assert_eq!(
        text_of(list(&call, &json!({ "target_directory": "/p" }))),
        "/p/\n  - a.md\n  - sub/"
    );
}

#[test]
fn a_name_that_only_starts_with_dot_git_is_listed() {
    let call = seeded(&["/p/.github/ci.yml", "/p/.gitmodules"]);
    assert_eq!(
        text_of(list(&call, &json!({ "target_directory": "/p" }))),
        "/p/\n  - .github/\n    - ci.yml\n  - .gitmodules"
    );
}

#[test]
fn the_sort_order_is_the_order_access_glob_gives() {
    let call = seeded(&["/B", "/a-b/x", "/a.txt", "/a/x", "/b"]);
    assert_eq!(
        text_of(list(&call, &json!({ "target_directory": "/" }))),
        "/\n  - B\n  - a-b/\n    - x\n  - a.txt\n  - a/\n    - x\n  - b"
    );
    let globbed = call
        .context()
        .access()
        .glob("/**")
        .expect("the pattern is valid");
    assert_eq!(globbed, ["/B", "/a-b/x", "/a.txt", "/a/x", "/b"]);
}

#[test]
fn a_directory_sorts_by_its_name_with_a_slash_added() {
    // `a` the directory sorts as `a/`, which is after `a-b` and `a.txt`,
    // because `/` is 0x2f and `-` and `.` are lower.
    let call = seeded(&["/a/x", "/a-b", "/a.txt", "/a0"]);
    let text = text_of(list(&call, &json!({ "target_directory": "/" })));
    let names: Vec<&str> = text
        .lines()
        .skip(1)
        .filter(|line| line.starts_with("  - "))
        .collect();
    assert_eq!(names, ["  - a-b", "  - a.txt", "  - a/", "  - a0"]);
}

#[test]
fn a_symlink_is_listed_by_name_without_a_slash_and_is_never_entered() {
    let lister = |path: &str| -> Result<Vec<Entry>, VfsError> {
        match path {
            "/p" => Ok(vec![
                entry("link", FileType::Symlink),
                entry("sub", FileType::Directory),
                entry("file", FileType::File),
            ]),
            "/p/sub" => Ok(vec![entry("inner", FileType::File)]),
            other => panic!("the walk entered {other}"),
        }
    };
    let tree = walk("/p", &[], 15_000, lister).expect("the walk succeeds");
    assert_eq!(
        render(&tree),
        "/p/\n  - file\n  - link\n  - sub/\n    - inner"
    );
}

#[test]
fn every_other_kind_of_node_is_listed_as_a_plain_name() {
    let lister = |_: &str| -> Result<Vec<Entry>, VfsError> {
        Ok(vec![
            entry("block", FileType::BlockDevice),
            entry("char", FileType::CharDevice),
            entry("fifo", FileType::Fifo),
            entry("sock", FileType::Socket),
        ])
    };
    let tree = walk("/dev", &[], 15_000, lister).expect("the walk succeeds");
    assert_eq!(
        render(&tree),
        "/dev/\n  - block\n  - char\n  - fifo\n  - sock"
    );
}

#[test]
fn the_pack_sorts_each_listing_itself_because_list_promises_no_order() {
    let lister = |path: &str| -> Result<Vec<Entry>, VfsError> {
        match path {
            "/p" => Ok(vec![
                entry("b", FileType::File),
                entry("a", FileType::Directory),
                entry("B", FileType::File),
                entry("a-b", FileType::File),
            ]),
            "/p/a" => Ok(vec![entry("z", FileType::File), entry("Z", FileType::File)]),
            other => panic!("the walk entered {other}"),
        }
    };
    let tree = walk("/p", &[], 15_000, lister).expect("the walk succeeds");
    assert_eq!(
        render(&tree),
        "/p/\n  - B\n  - a-b\n  - a/\n    - Z\n    - z\n  - b"
    );
}

#[test]
fn a_failure_listing_the_root_is_returned_for_the_tool_to_word() {
    let lister = |path: &str| -> Result<Vec<Entry>, VfsError> {
        Err(VfsError::NotADirectory {
            path: path.to_owned(),
        })
    };
    let failure = walk("/link/", &[], 15_000, lister).expect_err("the root cannot be listed");
    assert_eq!(
        failure,
        Failure {
            path: "/link/".to_owned(),
            error: VfsError::NotADirectory {
                path: "/link".to_owned()
            },
        },
        "the root's path is the target exactly as written"
    );
}

#[test]
fn a_subdirectory_that_is_gone_or_no_longer_a_directory_is_shown_without_children() {
    let lister = |path: &str| -> Result<Vec<Entry>, VfsError> {
        match path {
            "/p" => Ok(vec![
                entry("gone", FileType::Directory),
                entry("open", FileType::Directory),
                entry("swapped", FileType::Directory),
            ]),
            "/p/gone" => Err(VfsError::NotFound {
                path: path.to_owned(),
            }),
            "/p/open" => Ok(vec![entry("a", FileType::File)]),
            "/p/swapped" => Err(VfsError::NotADirectory {
                path: path.to_owned(),
            }),
            other => panic!("the walk entered {other}"),
        }
    };
    let tree = walk("/p", &[], 15_000, lister).expect("an empty look at these is true");
    assert_eq!(
        render(&tree),
        "/p/\n  - gone/\n  - open/\n    - a\n  - swapped/"
    );
}

#[test]
fn a_subdirectory_that_cannot_be_listed_for_any_other_reason_fails_the_walk() {
    for label in ["conflict", "denied", "io"] {
        let lister = |path: &str| -> Result<Vec<Entry>, VfsError> {
            match path {
                "/p" => Ok(vec![
                    entry("locked", FileType::Directory),
                    entry("open", FileType::Directory),
                ]),
                "/p/locked" => Err(listing_error(label, path)),
                "/p/open" => Ok(vec![entry("a", FileType::File)]),
                other => panic!("the walk entered {other}"),
            }
        };
        let failure = walk("/p", &[], 15_000, lister).expect_err(label);
        assert_eq!(
            failure.path, "/p/locked",
            "{label}: the directory that failed"
        );
        assert_eq!(failure.error, listing_error(label, "/p/locked"), "{label}");
    }
}

/// The error a listing of `path` fails with, for the kind named by `label`.
fn listing_error(label: &str, path: &str) -> VfsError {
    match label {
        "conflict" => VfsError::Conflict {
            path: path.to_owned(),
            detail: "another call holds a write claim".to_owned(),
        },
        "denied" => VfsError::PermissionDenied {
            path: path.to_owned(),
            reason: "Reading /p/locked is denied by policy.".to_owned(),
        },
        "io" => VfsError::Io {
            path: path.to_owned(),
            kind: std::io::ErrorKind::PermissionDenied,
            raw_os_error: None,
            message: "access denied".to_owned(),
        },
        other => panic!("no listing error named {other}"),
    }
}

#[test]
fn a_conflict_below_the_root_is_worded_for_the_directory_that_conflicted() {
    let lister = |path: &str| -> Result<Vec<Entry>, VfsError> {
        match path {
            "notes" => Ok(vec![entry("locked", FileType::Directory)]),
            "notes/locked" => Err(VfsError::Conflict {
                path: "/notes/locked".to_owned(),
                detail: "another call holds a write claim".to_owned(),
            }),
            other => panic!("the walk entered {other}"),
        }
    };
    let failure = walk("notes", &[], 15_000, lister).expect_err("the conflict fails the walk");
    assert_eq!(
        refusal("ListDir", &failure.path, &failure.error),
        "notes/locked was changed, or is being changed, by another call in this round, so it \
         cannot be read in this round. Call ListDir again in your next message."
    );
}

#[test]
fn every_successful_listing_is_untrusted_output() {
    let call = seeded(&["/p/a.md", "/p/.git/HEAD"]);
    call.context()
        .access()
        .mkdir("/empty", false)
        .expect("the directory is made");
    for args in [
        json!({ "target_directory": "/p" }),
        json!({ "target_directory": "/empty" }),
        json!({ "target_directory": "/" }),
        json!({ "target_directory": "/p", "ignore_globs": ["*.md"] }),
    ] {
        let output = list(&call, &args).expect("the listing succeeds");
        assert_eq!(output.trust(), OutputTrust::Untrusted, "{args}");
    }
}
