//! The `std` file tools over a real directory, mounted with
//! `RealBackend::rooted`: the acceptance flow a coding agent follows. List
//! the directory, find a word with Grep, read a window of the file, and read
//! the next window from the offset the first window's trailer names.
//!
//! The tools come from the std Plugin's dynamic library, loaded from a
//! Plugin directory as the server loads it, so each call crosses the C
//! contract and each read goes through the loader's file functions to the
//! real backend.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use runvil::effect::{ToolCallOrigin, ToolCaller};
use runvil::harness::{PluginReport, insert_plugin_dir};
use runvil::plugin::{Pack, PackRegistry, ToolContext};
use runvil::vfs::{Access, Origin, RealBackend, VfsRef};
use runvil::{OutputTrust, ToolId, ToolOutput};
use serde_json::{Value, json};

use crate::common::plugin_dir;

/// The file the flow runs over.
const FILE: &str = "big.txt";

/// How many lines the file has.
const LINES: usize = 5000;

/// The line that holds the word Grep finds.
const NEEDLE_LINE: usize = 3000;

/// The word only that line holds.
const NEEDLE: &str = "needleword";

/// The first line the second window starts at, which is the line after the
/// last one a 100,000-byte window of 48-byte numbered lines holds.
const SECOND_OFFSET: usize = 2084;

/// Counts the directories this process has made, so two tests never share
/// one.
static DIRECTORIES: AtomicUsize = AtomicUsize::new(0);

/// A directory named from the test, the process id, and a counter, holding
/// the file the flow reads.
// clippy.toml's allow-expect-in-tests covers #[test] functions only, not
// their helpers; failing the test by panicking with the invariant named
// is exactly what this is for.
#[expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]
fn seeded_directory() -> tempfile::TempDir {
    let name = format!(
        "std-real-directory-{}-{}-",
        std::process::id(),
        DIRECTORIES.fetch_add(1, Ordering::Relaxed)
    );
    let directory = tempfile::Builder::new()
        .prefix(&name)
        .tempdir()
        .expect("a temporary directory is made");
    let mut text = String::new();
    for number in 1..=LINES {
        if number == NEEDLE_LINE {
            text.push_str(NEEDLE);
            text.push_str(&"x".repeat(40 - NEEDLE.len()));
        } else {
            text.push_str(&"x".repeat(40));
        }
        text.push('\n');
    }
    std::fs::write(directory.path().join(FILE), text).expect("the file is written");
    directory
}

/// One run's instance of the Pack `runvil/std`, made by the std Plugin's
/// library, loaded from the Plugin directory the tests share. The report
/// keeps the Plugin loaded, so it comes back with the Pack.
#[expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]
fn std_pack() -> (Arc<dyn Pack>, PluginReport) {
    let mut packs = PackRegistry::new();
    let report = insert_plugin_dir(&mut packs, &plugin_dir()).expect("the directory is read");
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let (_info, id, factory) = packs
        .into_entries()
        .find(|(info, _, _)| info.fqn == "runvil/std")
        .expect("the std Plugin inserts its Pack");
    let pack = factory
        .expect("the Pack is usable")
        .new_pack(&id)
        .expect("the Pack instance is made");
    (pack, report)
}

/// Drives a future to its output, polling again after each `Pending`.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

/// Calls the tool `segment` of `pack` with `args` over `access`.
#[expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]
fn call(pack: &dyn Pack, access: &Access, segment: &str, args: Value) -> ToolOutput {
    let tool = ToolId::parse(&format!("std/{segment}")).expect("a two-segment tool id parses");
    let origin = ToolCallOrigin::new("Test", ToolCaller::Script);
    let context = ToolContext::new(&tool, access, &origin);
    block_on(pack.call(context, args)).expect("the call succeeds")
}

#[test]
fn list_find_read_a_window_and_read_the_next_one_by_the_trailer_offset() {
    let directory = seeded_directory();
    let backend = RealBackend::rooted(directory.path()).expect("the directory mounts");
    let vfs = VfsRef::builder().mount("/", backend).build();
    let access = vfs
        .acquire(Origin::new("std real directory"))
        .expect("the run acquires the filesystem");
    let (pack, _report) = std_pack();

    let listing = call(&*pack, &access, "dir", json!({ "target_directory": "/" }));
    assert_eq!(listing.trust(), OutputTrust::Untrusted);
    assert_eq!(listing.text(), format!("/\n  - {FILE}"));

    let found = call(&*pack, &access, "grep", json!({ "pattern": NEEDLE }));
    assert_eq!(found.trust(), OutputTrust::Untrusted);
    let expected_line = format!("  {NEEDLE_LINE}:{NEEDLE}{}", "x".repeat(40 - NEEDLE.len()));
    assert_eq!(found.text(), format!("/{FILE}\n{expected_line}"));

    let path = format!("/{FILE}");
    let first = call(&*pack, &access, "read", json!({ "path": path }));
    assert_eq!(first.trust(), OutputTrust::Untrusted);
    assert!(
        first
            .text()
            .starts_with(&format!("     1|{}\n", "x".repeat(40))),
        "the first window starts at line 1"
    );
    let trailer = first
        .text()
        .lines()
        .last()
        .expect("a first window has a last line");
    assert!(
        trailer.contains(&format!("Use offset={SECOND_OFFSET} to continue.")),
        "{trailer}"
    );

    let second = call(
        &*pack,
        &access,
        "read",
        json!({ "path": path, "offset": SECOND_OFFSET }),
    );
    assert!(
        second.text().starts_with(&format!("  {SECOND_OFFSET}|")),
        "the second window starts at the offset the trailer named: {}",
        &second.text()[..20]
    );
    assert!(
        second.text().contains(&format!("  {NEEDLE_LINE}|{NEEDLE}")),
        "the second window holds the line Grep found"
    );
}
