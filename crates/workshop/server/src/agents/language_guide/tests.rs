//! Mapping a local directory into a run's filesystem: a directory mapped
//! under `/_runvil` reads through the mapping, is reached by listing down
//! from the root, refuses writes, leaves the in-memory root writable, and
//! is visible to a prompt that runs over the filesystem. Two directories
//! mapped one after the other are both visible, as the guide and the C
//! drive are with both switches on. Every test uses a local directory it
//! makes, so none depends on the language guide's files or on a path on
//! any one machine, but for the one that maps a drive's root, which runs
//! on Windows alone.

use std::error::Error;
use std::path::Path;

use runvil::vfs::{Access, FileType, Origin, VfsError, VfsRef};
use workshop_registry::Registry;

use super::mount_at;
#[cfg(windows)]
use super::{C_DRIVE_MOUNT, C_DRIVE_SOURCE};
use crate::agents::host_packs;
use crate::agents::tests::{BASHKIT_LIBRARY, plugin_dir, plugin_dir_of, run_prompt_over};

/// What a test returns: any error from setup or from the VFS fails it.
type Outcome = Result<(), Box<dyn Error>>;

/// Where the local directory is mapped.
const MOUNT: &str = "/_runvil/test";

/// The one page the local directory holds.
const PAGE: &str = "hello.md";

/// What that page says.
const TEXT: &str = "# Hello from a local page\n";

/// A directory holding the one page.
fn local_directory() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let directory = tempfile::tempdir().map_err(|e| format!("create a temp dir: {e}"))?;
    let page = directory.path().join(PAGE);
    std::fs::write(&page, TEXT).map_err(|e| format!("write {}: {e}", page.display()))?;
    Ok(directory)
}

/// The filesystem with `source` mapped to [`MOUNT`].
fn filesystem_with(source: &Path) -> VfsRef {
    mount_at(VfsRef::default(), MOUNT, source)
}

/// A run's access over the filesystem with `source` mapped to [`MOUNT`].
fn access_with(source: &Path) -> Result<Access, VfsError> {
    filesystem_with(source).acquire(Origin::new("local mapping test"))
}

#[test]
fn a_page_of_the_directory_reads_through_the_mapping() -> Outcome {
    let directory = local_directory()?;
    let access = access_with(directory.path())?;

    assert_eq!(access.read_string(&format!("{MOUNT}/{PAGE}"))?, TEXT);
    let names: Vec<String> = access
        .list(MOUNT)?
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, [PAGE]);
    Ok(())
}

#[test]
fn listing_down_from_the_root_reaches_the_directory() -> Outcome {
    let directory = local_directory()?;
    let access = access_with(directory.path())?;
    let names = |path: &str| -> Result<Vec<String>, VfsError> {
        Ok(access
            .list(path)?
            .into_iter()
            .map(|entry| entry.name)
            .collect())
    };

    // The in-memory root holds nothing, so the mapping is the only way down.
    assert_eq!(names("/")?, ["_runvil"]);
    assert_eq!(names("/_runvil")?, ["test"]);
    assert_eq!(names(MOUNT)?, [PAGE]);
    assert_eq!(access.stat("/_runvil")?.file_type, FileType::Directory);
    Ok(())
}

#[test]
fn the_mapping_refuses_a_write_and_leaves_the_page_alone() -> Outcome {
    let directory = local_directory()?;
    let access = access_with(directory.path())?;

    let created = access.write(&format!("{MOUNT}/new.md"), b"new");
    assert!(
        matches!(created, Err(VfsError::ReadOnly { .. })),
        "{created:?}"
    );
    let replaced = access.write(&format!("{MOUNT}/{PAGE}"), b"changed");
    assert!(
        matches!(replaced, Err(VfsError::ReadOnly { .. })),
        "{replaced:?}"
    );
    let page = directory.path().join(PAGE);
    let text =
        std::fs::read_to_string(&page).map_err(|e| format!("read {}: {e}", page.display()))?;
    assert_eq!(text, TEXT);
    assert!(!directory.path().join("new.md").exists());
    Ok(())
}

#[test]
fn the_in_memory_root_stays_writable_beside_the_mapping() -> Outcome {
    let directory = local_directory()?;
    let access = access_with(directory.path())?;

    access.write("/scratch.txt", b"kept")?;
    assert_eq!(access.read_string("/scratch.txt")?, "kept");
    Ok(())
}

#[test]
fn a_directory_that_cannot_be_mapped_leaves_the_run_its_root() -> Outcome {
    let directory = local_directory()?;
    let missing = directory.path().join("no-such-directory");
    let access = access_with(&missing)?;

    assert!(!access.exists(MOUNT)?, "nothing is mapped");
    access.write("/scratch.txt", b"kept")?;
    assert_eq!(access.read_string("/scratch.txt")?, "kept");
    Ok(())
}

/// Where a second local directory is mapped.
const OTHER_MOUNT: &str = "/c";

/// The one page the second local directory holds.
const OTHER_PAGE: &str = "other.md";

/// What that page says.
const OTHER_TEXT: &str = "# Hello from another page\n";

/// A second directory, holding its own page.
fn other_directory() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let directory = tempfile::tempdir().map_err(|e| format!("create a temp dir: {e}"))?;
    let page = directory.path().join(OTHER_PAGE);
    std::fs::write(&page, OTHER_TEXT).map_err(|e| format!("write {}: {e}", page.display()))?;
    Ok(directory)
}

#[test]
fn two_directories_mapped_one_after_the_other_are_both_visible() -> Outcome {
    let first = local_directory()?;
    let second = other_directory()?;
    let access = mount_at(filesystem_with(first.path()), OTHER_MOUNT, second.path())
        .acquire(Origin::new("two mappings test"))?;

    assert_eq!(access.read_string(&format!("{MOUNT}/{PAGE}"))?, TEXT);
    assert_eq!(
        access.read_string(&format!("{OTHER_MOUNT}/{OTHER_PAGE}"))?,
        OTHER_TEXT
    );
    let names: Vec<String> = access
        .list("/")?
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["_runvil", "c"], "the root lists both ways down");

    let written = access.write(&format!("{OTHER_MOUNT}/new.md"), b"new");
    assert!(
        matches!(written, Err(VfsError::ReadOnly { .. })),
        "{written:?}"
    );
    access.write("/scratch.txt", b"kept")?;
    assert_eq!(access.read_string("/scratch.txt")?, "kept");
    Ok(())
}

/// The root of a drive maps as any directory does, which is what the C
/// drive switch relies on. A machine with no C drive has nothing to map.
#[cfg(windows)]
#[test]
fn a_drives_root_maps_read_only_like_any_directory() -> Outcome {
    let root = Path::new(C_DRIVE_SOURCE);
    if !root.is_dir() {
        return Ok(());
    }
    let access =
        mount_at(VfsRef::default(), C_DRIVE_MOUNT, root).acquire(Origin::new("drive root test"))?;

    assert_eq!(access.stat(C_DRIVE_MOUNT)?.file_type, FileType::Directory);
    assert!(
        !access.list(C_DRIVE_MOUNT)?.is_empty(),
        "the drive's root lists its entries"
    );
    let written = access.write(&format!("{C_DRIVE_MOUNT}/runvil-test.tmp"), b"x");
    assert!(
        matches!(written, Err(VfsError::ReadOnly { .. })),
        "{written:?}"
    );
    Ok(())
}

/// A prompt that lists `/_runvil` and reads the mapped page with the `std`
/// file tools, and returns both answers.
const PROMPT: &str = r"---
name: sees-the-mapping
description: lists and reads a mapped directory
runvil: 0
uses:
  - std
---

# Sees the mapping

## Only

```lua
local listing = tools.call('std/dir', { target_directory = '/_runvil' })
local page = tools.call('std/read', { path = '/_runvil/test/hello.md' })
return listing .. '\n' .. page
```
";

#[tokio::test]
async fn a_prompt_sees_the_directory_mapped_under_the_runvil_mount() -> Outcome {
    let directory = local_directory()?;
    // The std Plugin is a library, which the Host loads from a directory.
    let (plugins, _library) = plugin_dir("sees-the-mapping");
    let (host, _) = host_packs(&Registry::new(), None, Some(plugins.path()));

    let result = run_prompt_over(host, PROMPT.to_owned(), filesystem_with(directory.path())).await;

    let Ok(Ok(final_text)) = result else {
        return Err(format!("the prompt did not complete: {result:?}").into());
    };
    assert!(
        final_text.contains("  - test/"),
        "the prompt lists the mapping under /_runvil: {final_text}"
    );
    assert!(
        final_text.contains("     1|# Hello from a local page"),
        "the prompt reads the mapped page: {final_text}"
    );
    Ok(())
}

/// A prompt that lists the root and reads a page of each mapped
/// directory with the shell, and returns what the shell answers.
const SHELL_PROMPT: &str = r"---
name: shells-over-the-mappings
description: reads two mapped directories with the shell
runvil: 0
uses: [bash]
---

# Shells over the mappings

## Only

```lua
return tools.call('bash/shell', {
  command = 'ls /; cat /c/other.md /_runvil/test/hello.md',
})
```
";

#[tokio::test]
async fn the_shell_reads_two_directories_mapped_into_its_run() -> Outcome {
    let first = local_directory()?;
    let second = other_directory()?;
    let vfs = mount_at(filesystem_with(first.path()), OTHER_MOUNT, second.path());
    let (plugins, _library) = plugin_dir_of("shells-over-the-mappings", BASHKIT_LIBRARY);
    let (host, _) = host_packs(&Registry::new(), None, Some(plugins.path()));

    let result = run_prompt_over(host, SHELL_PROMPT.to_owned(), vfs).await;

    let Ok(Ok(final_text)) = result else {
        return Err(format!("the prompt did not complete: {result:?}").into());
    };
    assert!(
        final_text.contains(
            "Exit code: 0\n\nCommand output:\n\n```\n_runvil\nc\n# Hello from another page\n\
             # Hello from a local page\n```"
        ),
        "the shell lists both mappings at the root and reads a page of each: {final_text}"
    );
    Ok(())
}
