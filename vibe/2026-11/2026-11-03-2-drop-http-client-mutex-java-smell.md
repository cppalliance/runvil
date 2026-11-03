---
name: Drop the HTTP client Mutex
overview: Workshop's inference broker and search provider each cache one Gateway client per gateway generation behind a Mutex, only to keep a reqwest::Client's connection pool alive across rounds. Share one process-wide reqwest::Client inside harness-gateway-client instead, so building a GatewayBroker or GatewaySearch is a few clones, and delete both caches.
todos:
  - id: shared-client
    content: "Turn catalog.rs's catalog_client into a crate-wide transport::shared_client and use it in GatewayChat::new, GatewayChat::keyless, GatewaySearch::with_timeout, and the catalog fetch; add a test that two separately built clients reuse one connection"
    status: completed
  - id: drop-broker-cache
    content: "Remove WorkshopBroker's Arc<Mutex<Option<(u64, GatewayBroker)>>>; current() builds a GatewayBroker from the usable gateway on every call; update the module doc"
    status: completed
  - id: drop-search-cache
    content: "Remove GatewaySearchProvider's Mutex<Option<(u64, GatewaySearch)>>; client() builds a GatewaySearch on every search; update the module doc"
    status: completed
  - id: gates
    content: "Run fmt, clippy (both halves), nextest for harness-gateway-client and the workshop crates, the docs gate, and the docs-claims scan"
    status: completed
isProject: false
---

# Drop the HTTP client Mutex

## Problem

`crates/workshop/server/src/agents/broker.rs` holds
`cached: Arc<Mutex<Option<(u64, GatewayBroker)>>>`, and
`crates/workshop/server/src/agents/search.rs` holds
`cached: Mutex<Option<(u64, GatewaySearch)>>`. Each stores the Gateway client
built for the last gateway generation and rebuilds it when the generation
changes.

The caches exist for one reason: `GatewayChat::new`, `GatewayChat::keyless`,
and `GatewaySearch::with_timeout` each call `reqwest::Client::new()`, which
builds a fresh connection pool and TLS configuration. Without the cache,
every round would pay a new connection and lose keep-alive.

## Rejected: a lock-free mutable cell

`InferenceBroker` is `Send + Sync`
(`crates/harness-internal/runner/src/performers.rs`), and `WorkshopBroker` is
cloned into `models()` futures that run on any worker thread. `Cell` and
`RefCell` are `!Sync`, so they do not compile here. `ArcSwapOption` (already a
workspace dependency) would drop the `Mutex` but keep the `Arc`, the cache,
and the generation bookkeeping.

## Rejected: build the broker where the gateway is published

`workshop-gateway` would gain a dependency on `harness-gateway-client`, which
widens that crate's surface for a cache the next option makes unnecessary.

## Chosen: one shared reqwest client

`catalog.rs` already keeps a process-wide `OnceLock<reqwest::Client>`
(`catalog_client`, MODEL-018). Generalize it to `transport::shared_client`
and use it everywhere the crate builds a client. A `reqwest::Client` clone is
an `Arc` clone, so `GatewayBroker::new` and `GatewaySearch::new` become cheap,
and the Workshop types build one per call from `UsableGateway`, which they
already read on every call.

Facility order (AGENTS.md Principles): this reuses the existing shared-client
facility and extends it from the catalog fetch to every request the crate
sends, which serves every caller of `GatewayChat` and `GatewaySearch`, not
only Workshop.

### Behavior

- Every Gateway request the process sends shares one connection pool. reqwest
  keys the pool by scheme, host, and port, so a replaced gateway at a new
  address gets its own connections and the old ones close when idle.
- No per-client configuration is lost: the receive timeout, the byte cap, and
  the search timeout live on the wrapper types and apply per request.
- A replaced gateway serves the next round and search as before, now without
  any generation comparison.
- The shared client spans tokio runtimes when a process runs more than one.
  Workshop runs one runtime; the catalog fetch already made this choice.

### Tests

- New, `harness-gateway-client`: two `GatewayChat` values built separately
  against one mock gateway send their rounds over the same TCP connection
  (the server records each request's peer address).
- Kept, `workshop-server`: the replacement tests in `broker-tests.rs` and
  `search-tests.rs` still prove the next round and search reach the new
  gateway.

## Outcome

- `GatewayChat::new`, `GatewayChat::keyless`, `GatewaySearch::with_timeout`,
  and the catalog fetch all use `transport::shared_client`.
- `WorkshopBroker` and `GatewaySearchProvider` hold only the `Registry`;
  `UsableGateway` lost its `generation` field, which had no other reader.
- The new pool test fails with `reqwest::Client::new()` restored in
  `GatewayChat` (distinct peer ports) and passes with the shared client.
- Gates: fmt, both clippy runs, the docs gate, and docs-claims pass. The
  non-Workshop suite passes (4126). The Workshop suite passes except
  `realtime_relay::overload::stalled_browser_cleanup_is_bounded_after_gateway_disconnect`,
  which fails identically on unmodified `master` and is unrelated.
