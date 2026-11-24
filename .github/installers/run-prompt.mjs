// Launches one agent over the agents socket of a running Workshop and
// requires that its run ends with text, for the install cells of
// installers.yml and for update.sh.
//
// Usage: node run-prompt.mjs <workshop-port> <agent>
//   <workshop-port>  the loopback port Workshop serves its page on
//   <agent>          the name of a prompt in Workshop's agents folder,
//                    ~/.runvil/agents unless Workshop's settings name another
//
// The agents socket is the WebSocket Workshop's own page launches agents
// over. Workshop sends the list of agents when a client connects, and then
// one frame for each event of a launched run.
//
// Needs Node 22 or later and no install.
import { writeSync } from "node:fs";

// Writes one line and exits. The line is written before the exit: a line
// queued on a pipe can be lost when the process exits.
function end(code, message) {
  writeSync(code === 0 ? 1 : 2, `run-prompt: ${message}\n`);
  process.exit(code);
}

function fail(message) {
  end(1, message);
}

const [port, agent] = process.argv.slice(2);
if (!port || !agent || process.argv.length !== 4) {
  fail(`usage: run-prompt.mjs <workshop-port> <agent>; got ${process.argv.length - 2} arguments`);
}

const url = `ws://127.0.0.1:${port}/agents/ws`;
// The kind of every frame received, for the message of a failure.
const seen = [];
let text = "";
let ended = false;

setTimeout(() => {
  fail(`the run of ${agent} must end within 120 seconds; frames so far: [${seen.join(", ")}]`);
}, 120_000);

const socket = new WebSocket(url);
socket.addEventListener("error", (event) => {
  fail(`${url} failed: ${event.message || "the connection was refused or dropped"}`);
});
socket.addEventListener("close", () => {
  if (!ended) {
    fail(`${url} closed before the run of ${agent} ended; frames so far: [${seen.join(", ")}]`);
  }
});
socket.addEventListener("message", ({ data }) => {
  const frame = JSON.parse(data);
  const kind = frame.type === "agent_event" ? frame.event.kind : frame.type;
  seen.push(kind);
  if (kind === "agents") {
    if (!frame.agents.includes(agent)) {
      fail(`Workshop must list the agent ${agent}; it lists ${JSON.stringify(frame.agents)}`);
    }
    socket.send(JSON.stringify({ type: "launch", agent }));
  } else if (kind === "error") {
    fail(`the run of ${agent} failed: ${frame.message}`);
  } else if (kind === "agent_message") {
    text = frame.event.content ?? "";
  } else if (kind === "run_ended") {
    if (text.trim() === "") {
      fail(`the run of ${agent} must end with text; it ended after the frames [${seen.join(", ")}]`);
    }
    ended = true;
    end(0, `the run of ${agent} ended with the text ${JSON.stringify(text)}`);
  }
});
