//! Tests for `Access::read_range`: it takes the same four steps as
//! `read`, it passes its range through to the backend untouched, and it
//! slices on the memory backend.

use super::*;
use crate::MemoryBackend;
use crate::stat::{Entry, Stat};
use crate::traits::{ExecId, Vfs, VfsAccess};

/// A policy that denies reads and allows everything else, with a reason
/// that names the path.
struct DenyReads;

impl Policy for DenyReads {
    fn check(&self, op: Op, path: &VfsPath) -> Verdict {
        if op == Op::Read {
            Verdict::Deny(format!("reads of {path} are sealed"))
        } else {
            Verdict::Allow
        }
    }
}

/// The `(offset, len)` pairs a [`RangeSpy`] received, in arrival order.
type Ranges = Arc<Mutex<Vec<(u64, u64)>>>;

/// A backend over memory whose sessions record the range each
/// `read_range` asks for, then answer it from storage.
struct RangeSpy {
    memory: MemoryBackend,
    ranges: Ranges,
}

impl Vfs for RangeSpy {
    fn acquire(&mut self, cx: &AcquireContext) -> Result<Box<dyn VfsAccess>, VfsError> {
        Ok(Box::new(RangeSpyAccess {
            storage: self.memory.acquire(cx)?,
            ranges: Arc::clone(&self.ranges),
        }))
    }

    fn release(&mut self, id: ExecId) -> Result<(), VfsError> {
        self.memory.release(id)
    }
}

/// One session with a [`RangeSpy`]: the required methods forward to
/// storage, and `read_range` records its range first.
struct RangeSpyAccess {
    storage: Box<dyn VfsAccess>,
    ranges: Ranges,
}

impl VfsAccess for RangeSpyAccess {
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>, VfsError> {
        self.storage.read(path)
    }

    fn read_range(&self, path: &VfsPath, offset: u64, len: u64) -> Result<Vec<u8>, VfsError> {
        self.ranges
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((offset, len));
        self.storage.read_range(path, offset, len)
    }

    fn write(&mut self, path: &VfsPath, contents: &[u8]) -> Result<(), VfsError> {
        self.storage.write(path, contents)
    }

    fn append(&mut self, path: &VfsPath, contents: &[u8]) -> Result<(), VfsError> {
        self.storage.append(path, contents)
    }

    fn remove(&mut self, path: &VfsPath, recursive: bool) -> Result<(), VfsError> {
        self.storage.remove(path, recursive)
    }

    fn exists(&self, path: &VfsPath) -> Result<bool, VfsError> {
        self.storage.exists(path)
    }

    fn glob(&self, pattern: &str) -> Result<Vec<String>, VfsError> {
        self.storage.glob(pattern)
    }

    fn list(&self, path: &VfsPath) -> Result<Vec<Entry>, VfsError> {
        self.storage.list(path)
    }

    fn stat(&self, path: &VfsPath) -> Result<Stat, VfsError> {
        self.storage.stat(path)
    }

    fn mkdir(&mut self, path: &VfsPath, recursive: bool) -> Result<(), VfsError> {
        self.storage.mkdir(path, recursive)
    }

    fn rename(&mut self, from: &VfsPath, to: &VfsPath) -> Result<(), VfsError> {
        self.storage.rename(from, to)
    }

    fn copy(&mut self, from: &VfsPath, to: &VfsPath) -> Result<(), VfsError> {
        self.storage.copy(from, to)
    }
}

#[test]
fn a_policy_denied_read_range_never_reaches_the_backend() -> Result<(), VfsError> {
    let fs = TestFs::seeded(&[("/f.txt", "hello world")]);
    let vfs = VfsRef::builder()
        .mount("/", fs.clone())
        .policy(DenyReads)
        .build();
    let access = access(&vfs)?;
    assert_eq!(
        access.read_range("/f.txt", 0, 10),
        Err(VfsError::PermissionDenied {
            path: "/f.txt".to_owned(),
            reason: "reads of /f.txt are sealed".to_owned(),
        })
    );
    let reads: Vec<Call> = fs
        .calls()
        .into_iter()
        .filter(|call| matches!(call, Call::Op(Op::Read, _)))
        .collect();
    assert!(reads.is_empty(), "a denied read_range reached the backend");
    Ok(())
}

#[test]
fn a_read_range_takes_a_read_claim() -> Result<(), VfsError> {
    let vfs = handle(TestFs::seeded(&[("/f.txt", "data")]));
    let reader = access(&vfs)?;
    let writer = access(&vfs)?;
    reader.read_range("/f.txt", 0, 4)?;
    let message = conflict_message(writer.write("/f.txt", b"new"));
    assert!(message.contains("/f.txt"), "names the path: {message}");
    assert!(
        message.contains("read"),
        "names the standing claim kind: {message}"
    );
    Ok(())
}

#[test]
fn read_ranges_by_two_identities_never_conflict() -> Result<(), VfsError> {
    let vfs = handle(TestFs::seeded(&[("/f.txt", "data")]));
    let first = access(&vfs)?;
    let second = access(&vfs)?;
    first.read_range("/f.txt", 0, 2)?;
    assert_eq!(second.read_range("/f.txt", 2, 2)?, b"ta");
    Ok(())
}

#[test]
fn a_read_range_fires_one_read_event_and_a_denied_one_fires_none() -> Result<(), VfsError> {
    let events: Arc<Mutex<Vec<(Op, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&events);
    let vfs = VfsRef::builder()
        .mount("/", TestFs::seeded(&[("/a/f.txt", "data")]))
        .on_op(move |event: OpEvent<'_>| {
            recorded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((event.op(), event.path().to_string()));
        })
        .build();
    let caller = access(&vfs)?;
    // The path is spelled with an alias, so the event carries the
    // canonical spelling.
    caller.read_range("/a/./f.txt", 0, 4)?;
    assert_eq!(
        events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        &[(Op::Read, "/a/f.txt".to_owned())]
    );

    let denied_events: Arc<Mutex<Vec<Op>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&denied_events);
    let sealed = VfsRef::builder()
        .mount("/", TestFs::seeded(&[("/f.txt", "data")]))
        .policy(DenyReads)
        .on_op(move |event: OpEvent<'_>| {
            recorded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(event.op());
        })
        .build();
    let denied = access(&sealed)?;
    assert!(matches!(
        denied.read_range("/f.txt", 0, 4),
        Err(VfsError::PermissionDenied { .. })
    ));
    assert!(
        denied_events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty(),
        "a denied read_range fired the sink"
    );
    Ok(())
}

#[test]
fn a_read_range_passes_its_offset_and_length_through_unclamped() -> Result<(), VfsError> {
    let ranges: Ranges = Arc::new(Mutex::new(Vec::new()));
    let vfs = handle(RangeSpy {
        memory: MemoryBackend::new(),
        ranges: Arc::clone(&ranges),
    });
    let access = access(&vfs)?;
    access.write("/f.txt", b"hello")?;
    // A length one past 16 MiB: were it clamped, or turned into a
    // whole-file read, the backend would not record it.
    let bytes = access.read_range("/f.txt", 3, 16_777_217)?;
    assert_eq!(bytes, b"lo");
    assert_eq!(
        ranges
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        &[(3, 16_777_217)]
    );
    Ok(())
}

#[test]
fn a_read_range_slices_on_the_memory_backend() -> Result<(), VfsError> {
    let access = access(&VfsRef::default())?;
    access.write("/f.txt", b"hello world")?;
    assert_eq!(access.read_range("/f.txt", 6, 5)?, b"world");
    assert_eq!(access.read_range("/f.txt", 6, 100)?, b"world");
    assert!(access.read_range("/f.txt", 100, 5)?.is_empty());
    Ok(())
}
