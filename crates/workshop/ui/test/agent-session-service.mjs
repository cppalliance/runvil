// The agent-session view model (src/services/agent-session.ts) against a
// scripted wire, no DOM: durable events fold into transcript items in
// log order; ephemeral deltas coalesce into pending items by their
// superseding reply id, per channel, and the durable event replaces
// them; late deltas after their round settled are dropped; respond sends
// the operator's text alone, at any time; a new session id resets the
// transcript; errors fold as items. The turn's liveness rides along: an
// injected clock stamps each thought's start and end, `generating` turns
// off at `awaiting_input` and on at the next `user_message`, so a replayed
// log ends in the right state, `started` turns on with the session's
// first report, `run_ended` turns both off, a same-session reattach
// before a send's echo returns to the state the log left, `reconnecting`
// follows its rule, `cancelTurn` settles the service's own view, tool rows
// carry their bound tool id, and errors carry their popup title.
// Run: node test/agent-session-service.mjs
import { writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isDeepStrictEqual } from "node:util";
import * as esbuild from "esbuild";
import { assertNoLeaks } from "./helpers/leak-check.mjs";

const testDir = path.dirname(fileURLToPath(import.meta.url));

const bundle = await esbuild.build({
  stdin: {
    contents: `
      export * as lifecycle from "@workshop/platform/lifecycle";
      export { Emitter } from "@workshop/platform/event";
      export { AgentSessionService } from "./src/services/agent-session.ts";
    `,
    resolveDir: path.join(testDir, ".."),
    loader: "ts",
  },
  bundle: true,
  write: false,
  format: "esm",
  platform: "browser",
  target: "es2022",
  logLevel: "silent",
});

const bundlePath = path.join(os.tmpdir(), "runvil-agent-session-service-test.mjs");
await writeFile(bundlePath, bundle.outputFiles[0].text);
const { lifecycle, Emitter, AgentSessionService } = await import(pathToFileURL(bundlePath).href);

const failures = [];
function check(name, condition) {
  if (!condition) failures.push(name);
}

// The scripted wire: the AgentSocket surface the service consumes, with
// test-side fire methods and send recorders.
function makeWire() {
  const emitters = {
    agents: new Emitter(),
    session: new Emitter(),
    event: new Emitter(),
    delta: new Emitter(),
    error: new Emitter(),
    disconnect: new Emitter(),
  };
  return {
    onAgents: emitters.agents.event,
    onSession: emitters.session.event,
    onEvent: emitters.event.event,
    onDelta: emitters.delta.event,
    onError: emitters.error.event,
    onDisconnect: emitters.disconnect.event,
    launched: [],
    responses: [],
    cancels: 0,
    launchResult: true,
    respondResult: true,
    cancelResult: true,
    launch(agent) {
      this.launched.push(agent);
      return this.launchResult;
    },
    respond(...args) {
      this.responses.push(args);
      return this.respondResult;
    },
    cancelTurn() {
      this.cancels++;
      return this.cancelResult;
    },
    fire: {
      disconnect: () => emitters.disconnect.fire(undefined),
      agents: (list) => emitters.agents.fire(list),
      session: (session, agent = "chat") =>
        emitters.session.fire({ type: "agent_session", session, agent }),
      event: (kind, content, extra = {}) => {
        const { reply, ...eventFields } = extra;
        const frame = {
          type: "agent_event",
          index: 0,
          event: { kind, section: "chat", turn: 0, content, ...eventFields },
        };
        if (reply !== undefined) frame.reply = reply;
        emitters.event.fire(frame);
      },
      delta: (kind, content, reply) =>
        emitters.delta.fire({ type: "agent_delta", kind, content, reply }),
      // The agent parks on its empty inbox, as the durable log says.
      awaiting: () =>
        emitters.event.fire({
          type: "agent_event",
          index: 0,
          event: { kind: "awaiting_input", section: "chat", turn: 0, content: "" },
        }),
      error: (message) => emitters.error.fire(message),
    },
    disposeEmitters: () => {
      for (const emitter of Object.values(emitters)) emitter.dispose();
    },
  };
}

await assertNoLeaks(lifecycle, () => {
  // --- Durable events fold into transcript items in log order --------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    let transcriptFires = 0;
    service.onDidChangeTranscript(() => transcriptFires++);
    wire.fire.event("user_message", "hi there");
    wire.fire.event("agent_thought", "let me see", { model: "llama-3", reply: 0 });
    wire.fire.event("agent_message", "hello", { model: "llama-3", reply: 0 });
    wire.fire.event("tool_call", '[{"id":"call_1","name":"read","arguments":{"path":"a"}}]', {
      model: "llama-3",
      reply: 1,
    });
    wire.fire.event("tool_call_update", "file body", { tool_call_id: "call_1" });
    const kinds = service.items.map((item) => item.kind);
    check(
      "durable events fold in log order",
      isDeepStrictEqual(kinds, ["user", "reasoning", "reply", "tool-call", "tool-result"]),
    );
    check(
      "the user item holds the byte-exact text",
      service.items[0].text === "hi there",
    );
    check(
      "reply and reasoning items record their model label",
      service.items[1].model === "llama-3" && service.items[2].model === "llama-3",
    );
    check(
      "a settled durable item is not pending",
      service.items[1].pending === false && service.items[2].pending === false,
    );
    check(
      "the tool-call batch parses into one row per call",
      isDeepStrictEqual(service.items[3].calls, [
        { id: "call_1", name: "read", args: '{"path":"a"}', tool: null, vendor: null },
      ]),
    );
    check(
      "the tool result keeps its call id and content",
      service.items[4].toolCallId === "call_1" && service.items[4].text === "file body",
    );
    check("every fold fired the transcript change", transcriptFires === 5);
    service.dispose();
  }

  // --- Deltas coalesce by reply id and the durable event replaces them -----

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    wire.fire.delta("text", "Hel", 0);
    wire.fire.delta("text", "lo", 0);
    check(
      "text deltas coalesce into one pending reply",
      service.items.length === 1 &&
        service.items[0].kind === "reply" &&
        service.items[0].pending === true &&
        service.items[0].text === "Hello",
    );
    wire.fire.delta("reasoning", "hmm", 0);
    wire.fire.delta("reasoning", " ok", 0);
    check(
      "reasoning deltas coalesce into their own pending item",
      service.items.length === 2 &&
        service.items[1].kind === "reasoning" &&
        service.items[1].text === "hmm ok",
    );
    wire.fire.event("agent_thought", "hmm ok settled", { model: "m", reply: 0 });
    check(
      "the thought event replaces only the reasoning channel",
      service.items.length === 2 &&
        service.items[0].kind === "reply" &&
        service.items[0].pending === true &&
        service.items[1].kind === "reasoning" &&
        service.items[1].pending === false &&
        service.items[1].text === "hmm ok settled",
    );
    wire.fire.event("agent_message", "Hello there", { model: "m", reply: 0 });
    check(
      "the reply event replaces the coalesced text deltas",
      service.items.length === 2 &&
        service.items[1].kind === "reply" &&
        service.items[1].pending === false &&
        service.items[1].text === "Hello there",
    );
    wire.fire.delta("text", "late", 0);
    check(
      "a late delta after its round settled is dropped",
      service.items.length === 2,
    );
    wire.fire.delta("text", "next", 1);
    check(
      "the next round's deltas open a fresh pending reply",
      service.items.length === 3 && service.items[2].pending === true,
    );
    service.dispose();
  }

  // --- A tool-call batch settles its round's pending deltas ----------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    wire.fire.delta("reasoning", "planning", 0);
    wire.fire.event("tool_call", '[{"id":"c1","name":"search","arguments":{}}]', {
      model: "m",
      reply: 0,
    });
    check(
      "a tool-call batch supersedes its round's pending deltas",
      service.items.length === 1 && service.items[0].kind === "tool-call",
    );
    check(
      "the batch keeps its model label",
      service.items[0].model === "m",
    );
    service.dispose();
  }

  // --- Malformed batch content degrades to the raw text --------------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    wire.fire.event("tool_call", "not json", { model: "m" });
    check(
      "an unparsable tool-call batch keeps the raw text with no rows",
      service.items[0].calls.length === 0 && service.items[0].text === "not json",
    );
    service.dispose();
  }

  // --- Unknown event kinds are tolerated and render nothing ----------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    let fires = 0;
    service.onDidChangeTranscript(() => fires++);
    wire.fire.event("plan", "a future kind");
    check(
      "an unknown event kind folds nothing and fires nothing",
      service.items.length === 0 && fires === 0,
    );
    service.dispose();
  }

  // --- respond sends the operator's text alone, at any time -----------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    wire.fire.session("s1");
    wire.fire.awaiting();
    check("respond answers the waiting agent", service.respond("hi  ") === true);
    wire.fire.event("user_message", "hi  ");
    check("respond steers the working agent too", service.respond("and more") === true);
    check(
      "each message went out as its text alone, byte-exact, with no wait token",
      isDeepStrictEqual(wire.responses, [["hi  "], ["and more"]]),
    );
    check(
      "the text never enters the transcript until the server records it",
      service.items.length === 1 && service.items[0].kind === "user",
    );
    service.dispose();
  }

  // --- A failed respond folds a local error ----------------------------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    wire.fire.awaiting();
    wire.respondResult = false;
    check("a failed send reports false", service.respond("hi") === false);
    check(
      "the failure folds as an error item",
      service.items.length === 1 && service.items[0].kind === "error",
    );
    check("a failed send leaves generating off", service.generating === false);
    service.dispose();
  }

  // --- started: the session's first report, until a new session ---------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    const seen = [];
    service.onDidChangeStarted((value) => seen.push(value));
    check("no session has started at construction", service.started === false);
    wire.fire.session("s1");
    check(
      "an acknowledged session has not started until its run reports",
      service.started === false,
    );
    wire.fire.error("the run has no model yet");
    check("an error is no report of the run's", service.started === false);
    wire.fire.awaiting();
    check("the agent's first wait on its inbox starts it", service.started === true);
    wire.fire.session("s1");
    check("a same-session reattach keeps it started", service.started === true);
    wire.fire.session("s2");
    check("a new session has not started", service.started === false);
    wire.fire.delta("text", "already working", 0);
    check("a working agent's first chunk starts it", service.started === true);
    wire.fire.session("s3");
    wire.fire.event("tool_call", "[]", { reply: 0 });
    check("any durable event of the run starts it", service.started === true);
    check("only real changes fired", isDeepStrictEqual(seen, [true, false, true, false, true]));
    service.dispose();
  }

  // --- run_ended: the run takes no more messages ------------------------------

  {
    const wire = makeWire();
    const clock = { now: 10 };
    const service = new AgentSessionService(wire, () => clock.now);
    wire.fire.session("s1");
    wire.fire.awaiting();
    service.respond("bye");
    wire.fire.event("user_message", "bye");
    wire.fire.delta("reasoning", "wrapping up", 0);
    const started = [];
    const generating = [];
    let transcriptFires = 0;
    service.onDidChangeStarted((value) => started.push(value));
    service.onDidChangeGenerating((value) => generating.push(value));
    service.onDidChangeTranscript(() => transcriptFires++);
    clock.now = 40;
    wire.fire.event("run_ended", "");
    check(
      "run_ended turns generating off",
      service.generating === false && isDeepStrictEqual(generating, [false]),
    );
    check(
      "run_ended turns started off: the run takes no more messages",
      service.started === false && isDeepStrictEqual(started, [false]),
    );
    check("run_ended ends the open thinking", service.items[1].endedAt === 40);
    check(
      "run_ended renders no item, and announces the ended thinking once",
      service.items.length === 2 && transcriptFires === 1,
    );
    service.dispose();
  }

  {
    const wire = makeWire();
    const clock = { now: 10 };
    const service = new AgentSessionService(wire, () => clock.now);
    const started = [];
    service.onDidChangeStarted((value) => started.push(value));
    wire.fire.session("s1");
    wire.fire.event("run_ended", "");
    check(
      "a run whose end is its first report never reads as started",
      service.started === false && isDeepStrictEqual(started, []),
    );
    wire.fire.awaiting();
    service.respond("go");
    service.cancelTurn();
    wire.fire.event("run_ended", "");
    clock.now = 20;
    wire.fire.delta("reasoning", "after the end", 0);
    check(
      "run_ended clears the stop, so a first chunk after it streams",
      service.items[0].startedAt === 20 && service.items[0].endedAt === null,
    );
    service.dispose();
  }

  // --- Session acknowledgments: transcript reset on a new id -----------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    const sessions = [];
    service.onDidChangeSession((frame) => sessions.push(frame.session));
    // A refused launch folds a pre-session error; the session that then
    // starts leaves it out of its feed.
    wire.fire.error("unknown agent: bad");
    wire.fire.session("s1");
    check(
      "the first acknowledgment starts the session's transcript clean",
      service.items.length === 0 && service.session?.session === "s1",
    );
    wire.fire.event("user_message", "one");
    wire.fire.session("s1");
    check("a same-session reattach keeps the transcript", service.items.length === 1);
    wire.fire.session("s2");
    check("a new session id resets the transcript", service.items.length === 0);
    check("every acknowledgment fired", isDeepStrictEqual(sessions, ["s1", "s1", "s2"]));
    service.dispose();
  }

  // --- Errors fold as items and re-fire; agents list snapshots --------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    const heard = [];
    service.onError((message) => heard.push(message));
    wire.fire.error("unknown agent");
    check(
      "an error frame folds as an error item and re-fires",
      service.items[0]?.kind === "error" &&
        service.items[0].message === "unknown agent" &&
        isDeepStrictEqual(heard, ["unknown agent"]),
    );
    wire.fire.agents(["chat", "research"]);
    check(
      "the agents snapshot updates",
      isDeepStrictEqual([...service.agents], ["chat", "research"]),
    );
    check("launch forwards to the wire", service.launch("chat") === true);
    check("the launch named its agent", isDeepStrictEqual(wire.launched, ["chat"]));
    service.dispose();
  }

  // --- Reasoning timing: stamps come from the injected clock -----------------

  {
    const wire = makeWire();
    const clock = { now: 100 };
    const service = new AgentSessionService(wire, () => clock.now);
    wire.fire.delta("reasoning", "hmm", 0);
    check(
      "the first reasoning delta stamps startedAt and leaves endedAt open",
      service.items[0].startedAt === 100 && service.items[0].endedAt === null,
    );
    clock.now = 150;
    wire.fire.delta("reasoning", " ok", 0);
    check(
      "a later reasoning delta keeps the round's startedAt",
      service.items[0].text === "hmm ok" &&
        service.items[0].startedAt === 100 &&
        service.items[0].endedAt === null,
    );
    clock.now = 250;
    wire.fire.delta("text", "other round", 1);
    check(
      "a text delta of another reply leaves the thinking open",
      service.items[0].endedAt === null,
    );
    clock.now = 400;
    wire.fire.delta("text", "Hel", 0);
    check(
      "the same reply's first text delta ends the thinking",
      service.items[0].endedAt === 400 && service.items[0].startedAt === 100,
    );
    clock.now = 600;
    wire.fire.delta("text", "lo", 0);
    check("a later text delta does not move endedAt", service.items[0].endedAt === 400);
    clock.now = 700;
    wire.fire.event("agent_thought", "hmm ok settled", { model: "m", reply: 0 });
    const durable = service.items.find((item) => item.kind === "reasoning" && !item.pending);
    check(
      "the durable thought copies both stamps from the pending item",
      durable !== undefined && durable.startedAt === 100 && durable.endedAt === 400,
    );
    wire.fire.event("agent_thought", "replayed", { model: "m", reply: 7 });
    const replayed = service.items[service.items.length - 1];
    check(
      "a durable thought with no pending item has no stamps",
      replayed.startedAt === null && replayed.endedAt === null,
    );
    service.dispose();
  }

  {
    const wire = makeWire();
    const clock = { now: 1000 };
    const service = new AgentSessionService(wire, () => clock.now);
    wire.fire.delta("reasoning", "planning", 0);
    clock.now = 1300;
    wire.fire.event("agent_thought", "planning", { model: "m", reply: 0 });
    check(
      "a round that never streamed text ends its thinking at the durable thought",
      service.items[0].startedAt === 1000 && service.items[0].endedAt === 1300,
    );

    clock.now = 2000;
    wire.fire.delta("reasoning", "thinking about an error", 1);
    clock.now = 2500;
    wire.fire.error("boom");
    const afterError = service.items.find((item) => item.kind === "reasoning" && item.reply === 1);
    check("an error ends the open thinking", afterError.endedAt === 2500);

    clock.now = 3000;
    wire.fire.delta("reasoning", "waiting on input", 2);
    clock.now = 3400;
    wire.fire.awaiting();
    const afterInput = service.items.find((item) => item.kind === "reasoning" && item.reply === 2);
    check("awaiting_input ends the open thinking", afterInput.endedAt === 3400);
    clock.now = 9000;
    wire.fire.error("again");
    check(
      "an error does not move an already-ended thinking",
      afterInput === service.items.find((item) => item.kind === "reasoning" && item.reply === 2) &&
        service.items.find((item) => item.kind === "reasoning" && item.reply === 1).endedAt === 2500,
    );
    service.dispose();
  }

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    wire.fire.delta("reasoning", "default clock", 0);
    check(
      "without an injected clock the stamp is a finite performance.now() value",
      Number.isFinite(service.items[0].startedAt) && service.items[0].startedAt >= 0,
    );
    service.dispose();
  }

  // --- generating: on at a send and the operator's message, off at a wait ---

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    const seen = [];
    service.onDidChangeGenerating((value) => seen.push(value));
    check("generating is off at construction", service.generating === false);
    wire.fire.session("s1");
    wire.fire.awaiting();
    check("the agent waiting on its inbox leaves generating off", service.generating === false);
    wire.respondResult = false;
    service.respond("fails");
    check("a failed respond does not turn generating on", service.generating === false);
    wire.respondResult = true;
    service.respond("go");
    check("a successful respond turns generating on", service.generating === true);
    wire.fire.event("user_message", "go");
    wire.fire.event("tool_call", '[{"id":"c1","name":"fetch","arguments":{},"tool":"web/fetch"}]', {
      reply: 0,
    });
    check("a tool-call batch alone leaves generating as it was", service.generating === true);
    wire.fire.event("agent_message", "done", { reply: 1 });
    check(
      "an agent_message keeps generating on: a message that lands as the reply does goes on",
      service.generating === true,
    );
    wire.fire.awaiting();
    check("awaiting_input turns generating off", service.generating === false);
    check("only real changes fired", isDeepStrictEqual(seen, [true, false]));
    service.dispose();
  }

  {
    // A replayed log, delivered with no send: each user_message turns
    // generating on and each awaiting_input turns it off, so the replay
    // ends in the state its last entry gives.
    const replay = (kinds) => {
      const wire = makeWire();
      const service = new AgentSessionService(wire);
      wire.fire.session("s1");
      for (const kind of kinds) {
        if (kind === "awaiting_input") wire.fire.awaiting();
        else wire.fire.event(kind, kind, { reply: 0 });
      }
      const generating = service.generating;
      service.dispose();
      return generating;
    };
    check(
      "a replay that ends on the agent's wait ends idle",
      replay(["awaiting_input", "user_message", "agent_message", "awaiting_input"]) === false,
    );
    check(
      "a replay that ends mid-turn ends working",
      replay(["awaiting_input", "user_message", "agent_message", "awaiting_input", "user_message"]) === true,
    );
    check(
      "the next user_message after a wait shows the agent working again",
      replay(["awaiting_input", "user_message", "agent_message"]) === true,
    );
  }

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    wire.fire.session("s1");
    wire.fire.event("user_message", "first turn");
    wire.fire.event("tool_call", '[{"id":"c1","name":"fetch","arguments":{},"tool":"web/fetch"}]', {
      reply: 0,
    });
    // A sent cancel turns generating off; the result of a call it spared
    // shows the agent working again.
    service.cancelTurn();
    check("a sent cancel turns generating off", service.generating === false);
    wire.fire.event("tool_call_update", "page body", { tool_call_id: "c1" });
    check(
      "a tool_call_update answering a model batch call turns generating on",
      service.generating === true,
    );
    wire.fire.awaiting();
    wire.fire.event("user_message", "second turn");
    service.cancelTurn();
    wire.fire.event("tool_call_update", "stray", { tool_call_id: "c1" });
    check(
      "a tool_call_update whose call is in an earlier turn does not turn generating on",
      service.generating === false,
    );
    wire.fire.event("tool_call_update", "no id");
    check("a tool_call_update with no call id does not turn generating on", service.generating === false);
    wire.fire.event("tool_call", '[{"id":"c2","name":"read","arguments":{}}]', { reply: 1 });
    wire.fire.event("tool_call_update", "body", { tool_call_id: "c2" });
    check("a result for the current turn's call turns generating on", service.generating === true);
    wire.fire.error("server said no");
    check("an error turns generating off", service.generating === false);
    wire.fire.event("tool_call_update", "body", { tool_call_id: "c2" });
    check("an answered call turns it back on", service.generating === true);
    wire.fire.session("s1");
    check("a same-session acknowledgment leaves generating alone", service.generating === true);
    wire.fire.session("s2");
    check("an acknowledgment with a new session id turns generating off", service.generating === false);
    service.dispose();
  }

  // --- reconnecting: disconnect on, next acknowledgment off ------------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    const seen = [];
    service.onDidChangeReconnecting((value) => seen.push(value));
    check("reconnecting is off at construction", service.reconnecting === false);
    wire.fire.disconnect();
    check("a disconnect turns reconnecting on", service.reconnecting === true);
    wire.fire.disconnect();
    wire.fire.session("s1");
    check("the next acknowledgment turns reconnecting off", service.reconnecting === false);
    wire.fire.disconnect();
    wire.fire.session("s1");
    check(
      "a same-session reattach acknowledgment also clears it",
      service.reconnecting === false && isDeepStrictEqual(seen, [true, false, true, false]),
    );
    service.dispose();
  }

  // --- A same-session reattach before a send's echo -----------------------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    wire.fire.session("s1");
    wire.fire.awaiting();
    service.respond("maybe lost");
    check("the send turns generating on", service.generating === true);
    wire.fire.disconnect();
    wire.fire.session("s1");
    check(
      "a reattach with the send unechoed returns to waiting, the state awaiting_input left",
      service.generating === false,
    );
    wire.fire.event("user_message", "maybe lost");
    check("an echo the replay brings turns working on again", service.generating === true);
    service.dispose();
  }

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    wire.fire.session("s1");
    wire.fire.awaiting();
    service.respond("echoed");
    wire.fire.event("user_message", "echoed");
    wire.fire.disconnect();
    wire.fire.session("s1");
    check(
      "a reattach after the send's echo keeps the agent working",
      service.generating === true,
    );
    service.respond("steer");
    wire.fire.disconnect();
    wire.fire.session("s1");
    check(
      "a reattach before a steering message's echo keeps working, the state the log left",
      service.generating === true,
    );
    // A new session's log echoes none of the old session's sends: after a
    // cancel there, a reattach keeps the stop instead of restoring the
    // working state its log left.
    wire.fire.awaiting();
    service.respond("then lost");
    wire.fire.session("s2");
    wire.fire.event("tool_call", '[{"id":"c1","name":"read","arguments":{}}]', { reply: 0 });
    wire.fire.event("tool_call_update", "body", { tool_call_id: "c1" });
    service.cancelTurn();
    wire.fire.session("s2");
    check(
      "a new session forgets the old session's unechoed send",
      service.generating === false,
    );
    service.dispose();
  }

  // --- ToolCallRow.tool and vendor: the batch entry's strings, or null --------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    wire.fire.event(
      "tool_call",
      JSON.stringify([
        { id: "a", name: "search", arguments: { query: "x" }, tool: "web/search", vendor: "runvil" },
        { id: "b", name: "grab", arguments: {}, tool: "adhoc/grab" },
        { id: "c", name: "odd", arguments: {}, tool: 5, vendor: 5 },
        { id: "d", name: "empty", arguments: {}, tool: "", vendor: "" },
        { id: "e", name: "nul", arguments: {}, tool: null, vendor: null },
        { id: "f", name: "bare", arguments: {}, tool: "runvil/user-input/ask" },
        { id: "g", name: "unstamped", arguments: {} },
      ]),
      { reply: 0 },
    );
    check(
      "a batch entry's tool string parses onto its row, an adhoc id included",
      isDeepStrictEqual(
        service.items[0].calls.map((row) => row.tool),
        ["web/search", "adhoc/grab", null, null, null, "runvil/user-input/ask", null],
      ),
    );
    check(
      "a batch entry's vendor string parses onto its row",
      isDeepStrictEqual(
        service.items[0].calls.map((row) => row.vendor),
        ["runvil", null, null, null, null, null, null],
      ),
    );
    wire.fire.event("tool_call", "[1, null, [], \"x\"]", { reply: 1 });
    check("malformed entries produce no rows", service.items[1].calls.length === 0);
    service.dispose();
  }

  // --- cancelTurn: forwards, then settles the service's own view --------------

  {
    const wire = makeWire();
    const clock = { now: 10 };
    const service = new AgentSessionService(wire, () => clock.now);
    wire.fire.session("s1");
    wire.fire.awaiting();
    service.respond("go");
    wire.fire.delta("reasoning", "thinking", 0);
    clock.now = 90;
    wire.respondResult = true;
    wire.cancelResult = false;
    check("a cancel on a down socket reports false", service.cancelTurn() === false);
    check(
      "a failed cancel leaves generating and the thinking open",
      service.generating === true && service.items[0].endedAt === null,
    );
    wire.cancelResult = true;
    let transcriptFires = 0;
    service.onDidChangeTranscript(() => transcriptFires++);
    check("a sent cancel reports true", service.cancelTurn() === true);
    check("the cancel frame went to the wire", wire.cancels === 2);
    check("a sent cancel turns generating off", service.generating === false);
    check("a sent cancel ends the open thinking", service.items[0].endedAt === 90);
    check("closing the thinking announced a transcript change", transcriptFires === 1);
    clock.now = 500;
    service.cancelTurn();
    check(
      "a second cancel does not move the ended thinking or announce a change",
      service.items[0].endedAt === 90 && transcriptFires === 1,
    );
    service.dispose();
  }

  // --- A reasoning chunk after Stop opens as an ended thought ------------------

  {
    const wire = makeWire();
    const clock = { now: 10 };
    const service = new AgentSessionService(wire, () => clock.now);
    const last = () => service.items[service.items.length - 1];
    wire.fire.session("s1");
    wire.fire.awaiting();
    wire.respondResult = true;
    wire.cancelResult = true;
    service.respond("go");
    service.cancelTurn();
    clock.now = 40;
    wire.fire.delta("reasoning", "late", 0);
    check(
      "a first reasoning chunk after Stop opens an ended thought with no start",
      last().kind === "reasoning" &&
        last().pending === true &&
        last().startedAt === null &&
        last().endedAt === 40,
    );

    // The agent waiting on its inbox again ends the grace window.
    clock.now = 50;
    wire.fire.awaiting();
    wire.fire.delta("reasoning", "after the wait", 1);
    check(
      "a wait on the inbox after Stop makes the next first chunk stream",
      last().startedAt === 50 && last().endedAt === null,
    );

    // The operator's message restarts the turn's work.
    service.cancelTurn();
    service.respond("again");
    clock.now = 60;
    wire.fire.delta("reasoning", "fresh", 2);
    check(
      "a message after Stop makes the next first chunk stream",
      last().startedAt === 60 && last().endedAt === null,
    );

    // So does the message's durable record, replayed or live.
    service.cancelTurn();
    wire.fire.event("user_message", "replayed");
    clock.now = 65;
    wire.fire.delta("reasoning", "recorded", 7);
    check(
      "a user_message after Stop makes the next first chunk stream",
      last().startedAt === 65 && last().endedAt === null,
    );

    // A same-session reattach acknowledgment ends the grace window too.
    service.cancelTurn();
    wire.fire.session("s1");
    clock.now = 70;
    wire.fire.delta("reasoning", "reattached", 3);
    check(
      "a reattach acknowledgment after Stop makes the next first chunk stream",
      last().startedAt === 70 && last().endedAt === null,
    );

    // Deltas that append to an existing item never restamp it.
    clock.now = 80;
    wire.fire.delta("reasoning", " more", 3);
    check(
      "a later chunk appends without moving the clock stamps",
      last().text === "reattached more" && last().startedAt === 70 && last().endedAt === null,
    );
    service.dispose();
  }

  {
    const wire = makeWire();
    const clock = { now: 10 };
    const service = new AgentSessionService(wire, () => clock.now);
    wire.fire.session("s1");
    wire.fire.awaiting();
    service.respond("go");
    wire.cancelResult = false;
    check("a cancel that fails to send reports false", service.cancelTurn() === false);
    clock.now = 20;
    wire.fire.delta("reasoning", "still thinking", 0);
    check(
      "a failed cancel does not end a new thought: the turn is not stopped",
      service.items[0].startedAt === 20 && service.items[0].endedAt === null,
    );
    service.dispose();
  }

  // --- Error titles: server errors have none; a downed socket is titled -------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    const heard = [];
    service.onError((message) => heard.push(message));
    wire.fire.error("unknown agent: bad");
    check("a server error carries no title", service.items[0].title === null);
    wire.fire.awaiting();
    wire.respondResult = false;
    service.respond("hello");
    const local = service.items[service.items.length - 1];
    check(
      "a send on a down socket folds the titled connection error",
      local.kind === "error" &&
        local.title === "Connection failed" &&
        local.message ===
          "The connection was lost. Please check your network connection and try again.",
    );
    check(
      "the local failure announces the message as shown",
      heard[1] ===
        "The connection was lost. Please check your network connection and try again.",
    );
    service.dispose();
  }

  // --- Disposal severs the wire subscriptions -------------------------------

  {
    const wire = makeWire();
    const service = new AgentSessionService(wire);
    service.dispose();
    wire.fire.event("user_message", "after disposal");
    wire.fire.awaiting();
    check(
      "a disposed service folds nothing",
      service.items.length === 0 && service.started === false && service.generating === false,
    );
  }
});

if (failures.length > 0) {
  console.error(`agent-session-service: ${failures.length} failure(s)`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("agent-session-service: all assertions passed");
process.exit(0);
