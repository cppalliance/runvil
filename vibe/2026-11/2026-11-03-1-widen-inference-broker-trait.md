---
name: Widen the InferenceBroker trait
overview: "Code reaches GatewayBroker's streaming round through an inherent method instead of the InferenceBroker trait. Widen the trait so it is the broker's whole public interface, and reduce GatewayBroker's inherent impl to the `new` factory."
todos:
  - id: move-stream-delta
    content: "Move `StreamDelta` from `harness-gateway-client` to the Harness, beside `InferenceBroker`, and re-export it from `harness`"
    status: completed
  - id: widen-trait
    content: "Add a streaming round to `InferenceBroker` as a default method that runs `chat` and sends no pieces"
    status: completed
  - id: gateway-impl-factory-only
    content: "Reduce `impl GatewayBroker` to `new`; implement the streaming round in `impl InferenceBroker for GatewayBroker`"
    status: completed
  - id: callers-through-trait
    content: "Move Workshop's brokers and `broker-tests.rs` onto the trait method; delete `WorkshopBroker::chat_streaming`"
    status: completed
isProject: false
---

# Widen the InferenceBroker trait

## Functional Specification

### Problem

`harness::InferenceBroker` exists so that a broker's users depend on one
interface, not on a concrete broker. `GatewayBroker`
(`crates/harness-gateway-client/src/broker.rs`) breaks that. Besides its
`InferenceBroker` impl, its inherent `impl GatewayBroker` block exposes a
second round path:

- `chat_streaming`, a public inherent method. It runs the same round as
  `InferenceBroker::chat` and also hands each `StreamDelta` to a callback.
- `round`, the private helper that `chat` and `chat_streaming` share

The streaming round can only be reached by naming `GatewayBroker`:

- `WorkshopBroker` (`crates/workshop/server/src/agents/broker.rs`) holds a
  concrete `GatewayBroker` and calls its `chat_streaming`. It then repeats
  the pattern with its own inherent `WorkshopBroker::chat_streaming`, which
  `RunBroker::chat` calls.
- `crates/harness-gateway-client/src/broker-tests.rs` calls
  `GatewayBroker::chat_streaming` directly.

So the trait isn't the whole public interface. Anything that wants
streaming has to bypass it and name the concrete type, and every wrapper
broker has to copy the bypass. The trait doc even says so: "A broker that
shows the reply as it forms streams it on its own."

### Requirements

1. `InferenceBroker` is the broker's whole public interface. Everything a
   caller can do with a broker, including a streaming round, is a trait
   method.
2. The inherent `impl GatewayBroker` block holds only the `new` factory.
   `chat_streaming` and `round` leave it. The round logic lives in
   `impl InferenceBroker for GatewayBroker` or in a private free function.
   The `fmt::Debug` impl stays, because it is a trait impl.
3. No code outside `harness-gateway-client` calls a `GatewayBroker` method
   other than `new`. Workshop holds the broker as an `InferenceBroker` and
   streams through the trait.
4. No other `InferenceBroker` implementor keeps an inherent copy of a trait
   method. `WorkshopBroker::chat_streaming` is removed, and
   `RunBroker::chat` calls the trait method on its inner broker.
5. `broker-tests.rs` calls only `GatewayBroker::new` and trait methods.
   The behaviors it tests now stay tested:
   - pieces reach the callback in wire order
   - `chat` returns the whole reply
   - each round is sent to the model its options name
   - `models` lists the Gateway's catalog

### Design

These are the smallest changes to existing facilities that meet the
requirements.

- **Piece type.** Move `StreamDelta` (`Text`, `Reasoning`, non-exhaustive)
  from `harness-gateway-client/src/wire/delta.rs` to the crate that defines
  `InferenceBroker` (`harness-runner`, `performers.rs`), and re-export it
  from `harness`. The Gateway client's read loop produces this type, and
  the Gateway client stops exporting its own copy.
- **Trait method.** Add one method to `InferenceBroker`, with the same
  arguments as `chat` plus the callback:

  ```rust
  fn chat_streaming(
      &self,
      binding: ModelBinding,
      messages: Vec<Message>,
      tools: Vec<ToolSchema>,
      options: CompletionOptions,
      round: Round,
      on_piece: Arc<dyn Fn(StreamDelta) + Send + Sync>,
  ) -> BoxFuture<Result<Box<Completion>, Error>> {
      self.chat(binding, messages, tools, options, round)
  }
  ```

  The default runs `chat` and sends no pieces. That leaves the trait's many
  test and fixture implementors unchanged. A broker that can stream
  overrides it.
- **Harness behavior.** It doesn't change. The Harness still calls only
  `chat`, still takes only the finished reply, and records no fragment.
  Update the trait doc: replace "streams it on its own" with a pointer to
  `chat_streaming`.
- **Round parameter.** The trait method takes `round`, unlike today's
  inherent method. A wrapper such as `RunBroker` therefore has the round's
  id and origin on every path.

### Non-goals

- Making the Harness stream, or making it record or forward pieces.
- Changing the wire client (`GatewayChat`), the catalog fetch, or the
  request limits.
- Changing what Workshop shows, or how it stamps pieces with a round id.

## Verification

- `impl GatewayBroker { ... }` contains exactly one function, `new`.
- No crate outside `harness-gateway-client` names a `GatewayBroker` method
  other than `new`. `WorkshopBroker` has no inherent `chat_streaming`.
- The docs in `harness-gateway-client/src/lib.rs` and `broker.rs` that link
  `GatewayBroker::chat_streaming` link `InferenceBroker::chat_streaming`
  instead.
- The new trait method and the moved `StreamDelta` appear in the Harness's
  surface. If either reaches the `runvil` facade, regenerate
  `crates/runvil/public-api.txt`.
- These pass:
  - the full suite and the workshop suite
  - clippy, both runs
  - fmt
  - docs
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
