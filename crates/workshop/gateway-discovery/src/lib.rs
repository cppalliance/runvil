//! The Gateway discovery contract, as Workshop reads it.
//!
//! Workshop attaches to an already-running Gateway instead of starting a
//! second one. It does that by reading the `gateway.json` file the Gateway
//! writes into the run directory (`<home>/.runvil/run`) after a successful
//! bind: the file holds the loopback port, the bearer key, the pid, the boot
//! epoch, the version, and the start time. This crate is the reader half of
//! that protocol - read, validate, resolve, settle a launch race, wait for
//! health, and ask the Gateway to exit - and it is synchronous and
//! runtime-agnostic: no tokio, axum, or reqwest.
//!
//! The contract itself is written down in `CONTRACT.md` at the crate root,
//! which is the authority for both halves. The Gateway's writer lives in the
//! Gateway family; this crate never names it.
//!
//! The flow:
//!
//! 1. A reader ([`resolve`]) attaches only when the file is live: the pid
//!    is alive, one OS process boot with a `runvil-gateway` image brackets a
//!    same-socket health and bearer proof, and the file includes a boot
//!    identity. Anything else is stale and the file is deleted.
//!    [`ValidatedConnection`] holds that point-in-time proof without
//!    exposing a forgeable constructor.
//! 2. Launch races take [`launch_or_attach`]: the `gateway.json.lock`
//!    advisory lock elects one parent launcher; losers attach to the winner.
//! 3. A reader holding a [`ValidatedConnection`] asks the Gateway to exit
//!    with [`request_shutdown`], which posts its bearer key to
//!    `POST /shutdown`.
//!
//! URLs normalize to a literal `127.0.0.1`, never `localhost`, and probes
//! send the bound address as the `Host` header, matching the Gateway's
//! loopback `Host` allowlist.
//!
//! # Writing the file
//!
//! Production Workshop never writes `gateway.json`: the Gateway owns that
//! half, and a second writer would let Workshop forge a live endpoint.
//! Workshop's test doubles do need to write one, so the write and remove
//! functions and the process-lifetime lease are compiled only under the
//! `test-fixtures` feature.
//!
//! ## Invariants
//!
//! - Tier: vocabulary; may depend on no other `workshop-*` crate. It needs
//!   no async runtime, so Workshop can hold the contract without adopting
//!   one here. `cargo test -p build-xtask` enforces the product and
//!   container boundaries.
//! - The discovery file, the two lock file names, the stale rule, and the
//!   three HTTP calls are exactly what `CONTRACT.md` says. A behavior change
//!   here is a contract change, and `CONTRACT.md` changes with it.
//! - A bearer key is never written to logs or `Debug` output. Every error
//!   path that could echo a rejected request's status line redacts it.
//! - Reading the file tolerates fields this crate does not know, so a newer
//!   Gateway never breaks an older Workshop.
//! - No writer ships in a production build: the file is the Gateway's to
//!   write.

#[cfg(feature = "test-fixtures")]
mod atomic;
mod cancellation;
mod error;
mod file;
mod health;
mod lock;
mod paths;
mod shutdown;
mod stale;
mod sys;
mod validated;

pub use crate::cancellation::CancellationToken;
pub use crate::error::SidecarError;
pub use crate::file::GatewayDiscoveryFile;
#[cfg(feature = "test-fixtures")]
pub use crate::file::remove_if_mine;
pub use crate::health::{HealthError, ProbeError, wait_for_health, wait_for_health_cancellable};
#[cfg(feature = "test-fixtures")]
pub use crate::lock::GatewayInstanceLease;
pub use crate::lock::{LaunchDecision, LaunchLock, launch_or_attach, launch_or_attach_cancellable};
pub use crate::paths::{
    GATEWAY_DISCOVERY_FILE_NAME, GATEWAY_LOG_FILE_NAME, INSTANCE_LOCK_FILE_NAME, LOCK_FILE_NAME,
    default_run_dir, gateway_discovery_file_path, gateway_log_path, instance_lock_file_path,
    lock_file_path, run_dir,
};
pub use crate::shutdown::{ShutdownError, request_shutdown, request_shutdown_before};
#[cfg(any(test, feature = "test-fixtures"))]
#[doc(hidden)]
pub use crate::stale::resolve_for_test;
pub use crate::stale::{Resolution, StaleReason, is_running, resolve, resolve_cancellable};
pub use crate::validated::{ValidatedConnection, ValidationError};
