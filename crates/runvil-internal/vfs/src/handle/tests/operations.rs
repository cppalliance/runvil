//! Tests for the capability's file operations: reads and writes through
//! the handle, the empty-anchor refusal, and UTF-8 reads.

use super::*;

#[test]
fn an_access_reads_and_writes_through_the_handle() -> Result<(), VfsError> {
    let access = access(&handle(TestFs::default()))?;
    access.write("/notes/a.txt", b"hello")?;
    assert_eq!(access.read("/notes/a.txt")?, b"hello");
    assert!(access.exists("/notes/a.txt")?);
    Ok(())
}

#[test]
fn every_acquire_vends_a_process_unique_identity() -> Result<(), VfsError> {
    let vfs = handle(TestFs::default());
    let first = access(&vfs)?;
    let second = access(&vfs)?;
    assert_ne!(first.id, second.id);
    Ok(())
}

#[test]
fn an_empty_anchor_on_an_empty_file_is_refused_and_leaves_the_file_empty() -> Result<(), VfsError> {
    let access = access(&handle(TestFs::seeded(&[("/f.txt", "")])))?;
    match access.str_replace("/f.txt", "", "x") {
        Err(VfsError::Anchor {
            path,
            anchor,
            count,
        }) => {
            assert_eq!(path, "/f.txt");
            assert!(anchor.is_empty());
            assert_eq!(count, 0);
        }
        other => panic!("expected the empty-anchor refusal, got {other:?}"),
    }
    assert_eq!(access.read("/f.txt")?, b"");
    Ok(())
}

#[test]
fn an_empty_anchor_on_a_non_empty_file_is_refused_and_leaves_it_unchanged() -> Result<(), VfsError>
{
    let access = access(&handle(TestFs::seeded(&[("/f.txt", "hello")])))?;
    match access.str_replace("/f.txt", "", "x") {
        Err(VfsError::Anchor {
            path,
            anchor,
            count,
        }) => {
            assert_eq!(path, "/f.txt");
            assert!(anchor.is_empty());
            assert_eq!(count, 0);
        }
        other => panic!("expected the empty-anchor refusal, got {other:?}"),
    }
    assert_eq!(access.read("/f.txt")?, b"hello");
    Ok(())
}

#[test]
fn read_string_rejects_non_utf8() -> Result<(), VfsError> {
    let access = access(&handle(TestFs::default()))?;
    access.write("/bin.dat", &[b'a', 0xff, 0xfe])?;
    match access.read_string("/bin.dat") {
        Err(VfsError::NotUtf8 { path, valid_up_to }) => {
            assert_eq!(path, "/bin.dat");
            assert_eq!(valid_up_to, 1, "the offset of the first invalid byte");
        }
        other => panic!("expected a UTF-8 failure, got {other:?}"),
    }
    Ok(())
}

#[test]
fn the_handle_and_capability_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<VfsRef>();
    assert_send_sync::<Access>();
    assert_send_sync::<AcquireContext>();
}
