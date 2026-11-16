# Harness runner

This crate implements the Harness. A Host configures its prompt finder, Packs, broker, recorder, and timer once. `Host::run(name)` returns a `RunBuilder` that owns a snapshot of that Host and its run settings. Await it for a complete report, call `text()` or `output()` for a single result, or `prepare()` for a Harness whose control can send messages, stop work, or cancel it.

Each run gets a fresh filesystem and its own Pack instances. Unless the Host supplies a recorder, the run records in memory and releases that history when it ends. The default timer does not block the executor. The Harness owns no async runtime, so any executor can drive a run. This crate is private to the Harness family; applications reach it through `runvil`.
