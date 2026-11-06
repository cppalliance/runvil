//! The test VM's access fixture: a capability beside the handle on its
//! scope, the pair a section VM's setup takes.

use std::sync::Arc;

use runvil_vfs::{Access, Origin, ScopeHandle, VfsRef};

/// A test VM's access capability beside the handle on its scope, which
/// derives the access's store view.
#[derive(Clone)]
pub(crate) struct ScopedAccess {
    /// The capability the VM's `store` table operates through.
    pub(crate) access: Arc<Access>,
    /// The handle on the capability's scope.
    pub(crate) scope: ScopeHandle,
}

impl ScopedAccess {
    /// The root capability `vfs` acquires under `origin`, beside the handle
    /// on the scope it starts.
    #[track_caller]
    pub(crate) fn acquire(vfs: &VfsRef, origin: Origin) -> ScopedAccess {
        let (access, scope) = vfs.acquire_scope(origin).expect("the test handle acquires");
        ScopedAccess {
            access: Arc::new(access),
            scope,
        }
    }
}

/// A fresh default handle's access capability: the store is declared at
/// the root and the vended identity is the test's own, so seeding through
/// the store view and the VM's store ops never meet a second live identity.
#[track_caller]
pub(crate) fn fresh_access() -> ScopedAccess {
    ScopedAccess::acquire(&VfsRef::default(), Origin::new("lua test fixture"))
}
