//! [`ScopeHandle`], the authority over one scope: forking, joining, and
//! ending its identities, deriving their store views, and ending the
//! scope itself.

use std::fmt;
use std::sync::{Arc, Weak};

#[cfg(doc)]
use super::VfsRef;
use super::{Access, Scope};
use crate::error::VfsError;
use crate::observe::Origin;
use crate::traits::ExecId;

/// The authority over one scope, which [`VfsRef::acquire_scope`] returns
/// beside the scope's root [`Access`].
///
/// No other call makes one, and an `Access` never yields one, so code that
/// holds only accesses - a tool given its call's access, a caller given a
/// store view - cannot fork, join, or end an identity, derive a store
/// view, or end the scope. Every method that takes an `Access` panics on
/// one from another scope, so a scope handle acts on its own scope alone.
/// A scope handle never keeps its scope alive.
#[derive(Clone)]
pub struct ScopeHandle(Weak<Scope>);

impl fmt::Debug for ScopeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ScopeHandle").finish_non_exhaustive()
    }
}

impl ScopeHandle {
    /// The scope handle on `scope`, for the acquisition that started it.
    pub(super) fn new(scope: &Arc<Scope>) -> ScopeHandle {
        ScopeHandle(Arc::downgrade(scope))
    }

    /// Returns the capability for a new concurrent thread of execution,
    /// forked from `parent`.
    ///
    /// The child gets a fresh [`ExecId`] in this scope, and the spawn is
    /// the fork: the child shares a snapshot of the parent's vector clock
    /// plus its own entry, while the parent's entry advances, so the
    /// parent's later accesses are not ordered before the child's.
    /// Everything the parent did before the spawn happens before the
    /// child's first step. `origin` labels the child's operation events,
    /// as in [`VfsRef::acquire`].
    ///
    /// # Errors
    /// Returns [`VfsError::ScopeEnded`] when the scope or `parent`'s tool
    /// call has ended, or an error when the backend refuses to acquire the
    /// child's identity. A refused acquire still advances `parent`'s own
    /// clock entry, which orders nothing new.
    ///
    /// # Panics
    /// Panics when `parent` belongs to another scope.
    #[track_caller]
    pub fn spawn(&self, parent: &Access, origin: Origin) -> Result<Access, VfsError> {
        self.assert_governs(parent);
        parent.spawn(origin)
    }

    /// Merges `child`'s final clock into `owner`'s: the join, the matching
    /// edge of [`ScopeHandle::spawn`]'s fork. Everything `child` did - its
    /// writes included - happens before `owner`'s next step, so the owner's
    /// reads of the child's files no longer conflict. A child whose identity
    /// is still live contributes its current clock; a child whose last
    /// access dropped contributes its final clock, which the scope keeps for
    /// exactly this. A child ended through [`ScopeHandle::end_access`] keeps
    /// no seen snapshot, so joining it again contributes only its own entry
    /// and fork chain. An identity this scope never held - a timer's, or
    /// another scope's - joins nothing.
    pub fn join(&self, owner: ExecId, child: ExecId) {
        if let Some(scope) = self.0.upgrade() {
            scope.join(owner, child);
        }
    }

    /// Ends `child`, the identity of one tool call: merges its clock into
    /// `owner`'s, as [`ScopeHandle::join`] does, when `owner` is `Some`, and
    /// drops its seen snapshot. Every access still holding `child` - the
    /// tool's own, a store view derived from it - refuses its next
    /// operation, spawn, or store view with [`VfsError::ScopeEnded`] while
    /// the scope stays open. An identity this scope never held is left
    /// alone, and the call is a no-op once every access in the scope has
    /// dropped.
    pub fn end_access(&self, owner: Option<ExecId>, child: ExecId) {
        if let Some(scope) = self.0.upgrade() {
            scope.end(owner, child);
        }
    }

    /// Derives the store view from `access`: an ordinary [`Access`] rooted
    /// at the declared store root of the [`VfsRef`] `access` came from,
    /// whose operations reach the store's own mount alone. The view keeps
    /// `access`'s identity and scope - so one chain never conflicts with
    /// itself through its view - applies the store's strict logical-path
    /// rules, and reports error paths as the caller supplied them.
    ///
    /// # Errors
    /// Returns [`VfsError::NoStore`] when that `VfsRef` declares no store,
    /// or [`VfsError::ScopeEnded`] when the scope or `access`'s tool call
    /// has ended.
    ///
    /// # Panics
    /// Panics when `access` belongs to another scope.
    #[track_caller]
    pub fn store_view(&self, access: &Access) -> Result<Access, VfsError> {
        self.assert_governs(access);
        access.store_view()
    }

    /// Probes the declared store before a run starts: derives the store
    /// view from `access` and stats the store root through it, which makes
    /// the view's router acquire the store backend. A root the backend
    /// reports [`VfsError::NotFound`] passes, so a real directory created
    /// lazily still runs.
    ///
    /// # Errors
    /// Returns [`VfsError::NoStore`] when the [`VfsRef`] `access` came from
    /// declares no store, or an error when the store backend refuses the
    /// session or fails the stat.
    ///
    /// # Panics
    /// Panics when `access` belongs to another scope.
    #[track_caller]
    pub fn probe_store(&self, access: &Access) -> Result<(), VfsError> {
        self.assert_governs(access);
        access.store_view()?.probe_root()
    }

    /// Ends the scope: its claims stop conflicting at once, and every
    /// access still held in it - a store view the caller kept past the run,
    /// a forwarded mount session - refuses its next operation, spawn, or
    /// store view with [`VfsError::ScopeEnded`]. Idempotent, and a no-op
    /// once every access in the scope has dropped.
    pub fn end(&self) {
        if let Some(scope) = self.0.upgrade() {
            scope.close();
        }
    }

    /// Asserts that `access` belongs to this scope handle's scope.
    #[track_caller]
    fn assert_governs(&self, access: &Access) {
        assert!(
            self.0
                .upgrade()
                .is_some_and(|scope| Arc::ptr_eq(&scope, &access.scope)),
            "the access belongs to another scope than this scope handle's"
        );
    }
}

#[cfg(test)]
mod tests;
