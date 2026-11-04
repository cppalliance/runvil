//! Mapping a local directory into a run's filesystem: a directory mapped
//! under `/_runvil` reads through the mapping, is reached by listing down
//! from the root, refuses writes, leaves the in-memory root writable, and
//! is visible to a prompt that runs over the filesystem. Every test uses a
//! local directory it makes, so none depends on the language guide's files
//! or on a path on any one machine.

use std::error::Error;
use std::path::Path;
use std::sync::Arc;

use harness::record::RunOutcome;
use runvil::vfs::{Access, FileType, Origin, VfsError, VfsRef};
use workshop_registry::Registry;

use super::mount_at;
use crate::agents::host_context;
use crate::agents::tests::run_prompt_over;

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
    let (host, _ask) = host_context(&Registry::new(), None);

    let outcome = run_prompt_over(
        &Arc::new(host),
        PROMPT.to_owned(),
        filesystem_with(directory.path()),
    )
    .await;

    let Some(RunOutcome::Completed { final_text }) = outcome else {
        return Err(format!("the prompt did not complete: {outcome:?}").into());
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
