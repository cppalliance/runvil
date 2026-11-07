# Harness runner

This crate holds the per-run Harness and its effect loop. A Host builds one `Host` at startup with its Packs, its prompt finder, and its timer, and one Harness per run over a parsed prompt and that run's setup: its recorder, inference broker, services, and filesystem. The Harness then steps the Engine, performs each effect, returns each answer, and records the run. It owns no async runtime, so any async runtime can drive a run, and the Host can stop or cancel a run from outside. It is private to the Harness family, and clients reach it through the Harness's public API.
