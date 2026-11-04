# Engine Lua runtime

This crate runs each section's Lua code in a fresh, restricted sandbox and installs the Engine globals for the run's store, models, tools, and tasks, and the section's one `messages` conversation. Outside a Pack prelude's install, an Engine call that waits on the outside world suspends the Lua code and hands a request to the executor, so the sandbox performs no I/O of its own; store calls a prelude makes while it installs run directly against the run's store view. Packs can add Lua globals of their own, checked against a reserved-name list.
