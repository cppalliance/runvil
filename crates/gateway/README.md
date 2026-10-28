# The gateway's API principle

The gateway serves an API that other programs already know how to call. Each route follows the industry standard it is modeled on, where one exists, so a program written against that standard works against the gateway, and a client written against the gateway works against any server that implements the standard.

## The rule

Standards first. The chat, embeddings, models, speech, and transcription routes follow OpenAI's API: `POST /v1/chat/completions`, `POST /v1/embeddings`, `GET /v1/models`, `POST /v1/audio/speech`, and `POST /v1/audio/transcriptions`. The live stream on `/v1/realtime` follows OpenAI's Realtime transcription protocol: the same session object, the same audio buffer events, and the same `conversation.item.input_audio_transcription.delta` and `conversation.item.input_audio_transcription.completed` events.

`/v1/rerank` is the one route where OpenAI publishes no shape, so it follows the rerank shape that Jina and Cohere publish and that vLLM also serves. A client written for that shape works against the gateway.

Interoperability is checked in both directions. A standard client pointed at the gateway needs no special case, and a gateway client pointed at another server that speaks the standard needs none either.

## Extensions, and nothing else

Where a standard exists and the gateway needs something the standard lacks, the wire shape is the standard plus the smallest addition that makes the feature possible. Everything around the addition stays the standard's: route, methods, request fields, event names, error envelope.

Every addition carries three properties:

- It is named as an extension, with a name inside the standard's own namespace, so a reader of the standard's vocabulary can tell what it extends.
- It is documented beside the standard shape it extends, so both are read in one place.
- It is opt-in wherever the protocol offers a way to ask. A client that does not ask never receives the extra field or event.

A few routes have no standard behind them. `GET /v1/audio/voices` follows the convention the OpenAI-compatible servers converged on, and `/v1/tools/web_search` and the `/admin` routes are ours alone. Those count as extensions from the start, and the same three properties apply to them.

## Asking what a server offers

A client has to know whether a server offers an extension before it relies on one, and the asking has to be harmless to a server that has never heard of the gateway. An absent answer means the standard alone, never that something is broken.

The one nonstandard request has two candidate shapes. A dedicated capability endpoint is the preferred one, because it costs one request at connect time and every answer sits in one place. A response header on a call the client already makes costs no request, but it ties the answer to a call the client might never make, so it waits as a later convenience. Either way the plain server answers with a not-found, a no-content, or an empty result, which the client already tolerates. The endpoint is part of the intended arrangement the way the spec is. This is the rule the gateway follows when it adds one, and no such endpoint exists yet.

The probe only reports. The feature turns on through the negotiated include token the protocol already has, and only when the probe said the server has it. A server that answers the probe and then refuses the token has a fault on its side, and the client calls that standard only, never an error to show the user.

So a missing or negative answer produces an ordinary session, one that appends deltas and completes an item, with no second attempt, no error banner, and no probe attached to each request. Nothing the client shows suggests that a step failed, because none did.

Guessing does not replace asking. A version string alone is not evidence, an extension request sent first with recovery after the error is not a probe, a 404 is not a hard failure, and no probe may change the shape of a standard response.

## The live hypothesis

The extension the gateway depends on most is the live hypothesis in realtime transcription.

OpenAI's transcription stream is append-only. The `delta` events add text to an item, and the `completed` event closes it. Nothing in that shape can retract a word already sent. The gateway revises earlier words as more audio arrives and as later passes listen again, so it needs a way to say that part of the transcript changed.

A replacement snapshot is the smallest addition that supports revision: the item's full current text, a revision number, the audio span the snapshot covers, and how that text divides into finalized, agreed, and tentative parts. The client rebuilds the take from the snapshot, then patches its document word by word against the text it showed before, so a revision costs a few small edits and leaves the rest of the document alone.

The snapshot is the extra. That division is information the standard path never needs, because a standard client only appends, and a fallback ignores it.

A session asks for the snapshot through `session.include`, the field OpenAI already uses to negotiate optional output, by naming `item.input_audio_transcription.hypothesis`. A session that never names it receives plain deltas and a completed transcript.

## Nothing clever reaches the wire

The gateway runs several speech models and decides which result wins. That machinery stays behind the API. Which model ran, how the gateway reconciled them, and how long any of it took never appear in a response or an event. A caller sees a transcription endpoint that happens to be more accurate, or earlier, than one model alone would be. It never sees a model pick.

## The fallback is a requirement

A client pointed at a server without the extension must still transcribe, with the plainer behavior the standard gives. It falls back to the standard and keeps working with less.

A feature that only works against the gateway is a bug in the design. The test is whether the client still works when the extension is absent.

## What this rules out

- A parallel dialect of our own where a standard shape would do.
- Our own event or field names where a standard one exists.
- Removing an extension without a version step.
- An extension a caller cannot ignore.

## Checking a change

Before a route or a wire type changes, answer four questions:

1. Does the standard shape still hold?
2. Is the addition the smallest one that makes the feature possible, and is it opt-in?
3. Does the client still work without the addition?
4. Is the addition named as an extension in the spec?

The spec belongs with the gateway. The intended arrangement is a spec generated from the gateway's source rather than written by hand, with CI comparing the two so a route or a field cannot drift from its description. That keeps the principle in a check instead of in someone's memory.
