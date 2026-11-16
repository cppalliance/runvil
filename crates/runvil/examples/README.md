# Harness examples

Run these commands from the repository root. Each example below runs locally
without credentials, a Gateway, or a model service, and asserts its result.
Prompt paths are resolved from the crate directory, independently of the
process's current directory.

| Command | Demonstrates | Expected output |
|---|---|---|
| `cargo run -p runvil --example run-prompt` | Declared input and output files | `Hello, desk.` |
| `cargo run -p runvil --example typed-tool` | A typed async Rust tool, a structured result in Lua, and a declared output file | `3 items: 3750 cents` |
| `cargo run -p runvil --example local-broker` | An `InferenceBroker`, its model catalog, model selection, and `models.infer` | `Echo: Hello from the Harness` |
| `cargo run -p runvil --example interactive-run` | Prepared runs, queued messages, cancellation, and an in-memory recorder | `run 1: Ada wants to build a small Host.` followed by `run 2: canceled and recorded` |

The [typed tool](typed-tool.rs) registers a Pack with `PackRegistry::define`.
Its `LineItem` type supplies both the argument schema and decoding; `LineTotal`
becomes structured data that the [receipt prompt](prompts/receipt.md) reads.
The tool marks its calculated result as trusted. Tools returning external text
should use `OutputTrust::Untrusted`.

The [local broker](local-broker.rs) is a deterministic echo implementation for
learning the broker interface, not a language model. The [echo prompt](prompts/echo.md)
sends `args` through `models.infer`; the broker returns a `Completion` without
making a network request.

The [interactive example](interactive-run.rs) queues two messages before driving
the run, so it needs no terminal input. A UI can keep the same control handle
and send messages while the run is pending. Its [prompt](prompts/messages.md)
receives them with `tasks.receive()`. The example also cancels a second run
before execution and verifies that both outcomes reached the recorder.
`MemoryRecorder` retains history only until it is dropped.
