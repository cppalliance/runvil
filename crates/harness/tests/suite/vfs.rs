//! Guarding and watching the files a Host hands a run: a policy refuses an
//! undeclared path before the operation watcher sees it, and while the run
//! waits on its inbox, the Host's edit of a file the run read conflicts
//! and leaves the run's view intact.

use std::sync::{Arc, Mutex, PoisonError};

use harness::RunSetup;
use harness::record::{MemoryRecorder, RecordKind, RunId};
use harness::vfs::{Origin, VfsError, VfsRef};
use runvil::vfs::{MemoryBackend, Op, Policy, Verdict, VfsPath};

use crate::support::{Offline, launch, no_packs};

/// Reads the notes, waits for the operator's message, and writes the
/// approved notes.
const REVIEW: &str = concat!(
    "---\nname: review\ndescription: Reads the notes, then waits for the operator\nrunvil: 0\n",
    "uses:\n  - user\n",
    "input: { path: notes.md, description: The operator's notes }\n",
    "output: { path: summary.md, description: The approved notes }\n",
    "---\n\n# Review\n\n## Approve\n\n```lua\n",
    "local notes = store.read('notes.md')\n",
    "local answer = tasks.receive()\n",
    "store.write('summary.md', notes .. ' ' .. answer)\n",
    "```\n",
);

/// Each watched operation's origin label and file.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

/// Refuses every path except the two files the review prompt declares.
struct DeskFiles;

impl Policy for DeskFiles {
    fn check(&self, op: Op, path: &VfsPath) -> Verdict {
        let desk_file = ["/notes.md", "/summary.md"].contains(&path.as_str());
        match op {
            Op::Read
            | Op::Write
            | Op::Append
            | Op::Delete
            | Op::Rename
            | Op::Mkdir
            | Op::Copy
            | Op::Exists
            | Op::Glob
            | Op::List
            | Op::Stat
            | Op::Symlink
            | Op::ReadLink
            | Op::Chmod
                if desk_file =>
            {
                Verdict::Allow
            }
            // `Op` is `#[non_exhaustive]`: an operation added later is
            // refused until the desk names it.
            _ => Verdict::Deny(format!("{op:?} on {path} is not a desk file operation")),
        }
    }
}

/// A memory store behind [`DeskFiles`], beside the origins its watcher
/// saw.
fn desk_vfs() -> (VfsRef, Seen) {
    let seen = Seen::default();
    let sink = Arc::clone(&seen);
    let vfs = VfsRef::builder()
        .store("/", MemoryBackend::new())
        .policy(DeskFiles)
        .on_op(move |event| {
            sink.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((event.origin().label.clone(), event.origin().file.clone()));
        })
        .build();
    (vfs, seen)
}

/// Yields until the run the recorder began first waits on its inbox,
/// failing if it never does within a few thousand polls.
async fn until_the_run_waits_on_its_inbox(recorder: &MemoryRecorder) {
    for _ in 0..10_000 {
        let waits = recorder.records(RunId::from_raw(1)).iter().any(|record| {
            record.kind == RecordKind::Event && record.payload["kind"] == "inbox_waiting"
        });
        if waits {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the run never waited on its inbox");
}

#[test]
fn a_policy_refuses_an_undeclared_path_before_the_watcher_sees_it() {
    let (vfs, seen) = desk_vfs();
    vfs.acquire_store(Origin::new("desk seed"))
        .expect("the handle declares a store")
        .write("notes.md", b"Ship on Friday.")
        .expect("the policy allows a declared file");
    let stray = vfs
        .acquire_store(Origin::new("desk stray"))
        .expect("the handle declares a store")
        .write("secret.md", b"x");
    assert!(matches!(stray, Err(VfsError::PermissionDenied { .. })));
    assert_eq!(
        seen.lock().unwrap()[..],
        [("desk seed".to_owned(), file!().to_owned())]
    );
}

#[tokio::test]
async fn a_hosts_edit_of_a_file_the_run_read_conflicts_while_the_run_waits_on_its_inbox() {
    let (vfs, seen) = desk_vfs();
    vfs.acquire_store(Origin::new("desk seed"))
        .expect("the handle declares a store")
        .write("notes.md", b"Ship on Friday.")
        .expect("the policy allows a declared file");

    let recorder = Arc::new(MemoryRecorder::new());
    let setup = RunSetup {
        has_user: true,
        vfs: vfs.clone(),
        ..RunSetup::new(recorder.clone(), Arc::new(Offline))
    };
    let harness = launch(no_packs(), REVIEW, setup);
    let control = harness.control();
    // While the run waits on its inbox, the Host edits the notes the run
    // already read, then sends the approval.
    let (report, edited) = tokio::join!(harness.run(), async {
        until_the_run_waits_on_its_inbox(&recorder).await;
        let edit = vfs
            .acquire_store(Origin::new("desk edit"))
            .and_then(|view| view.write("notes.md", b"Ship on Monday."));
        assert!(control.send("Approved.".to_owned()));
        edit
    });
    let report = report.expect("the run is reported");

    assert!(matches!(edited, Err(VfsError::Conflict { .. })));
    let notes = vfs
        .acquire_store(Origin::new("desk check"))
        .expect("the handle declares a store")
        .read_string("notes.md")
        .expect("the notes stay readable");
    assert_eq!(notes, "Ship on Friday.");
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .any(|(_, file)| file == "Review")
    );
    assert_eq!(
        report.output.expect("the run completed"),
        "Ship on Friday. Approved."
    );
}
