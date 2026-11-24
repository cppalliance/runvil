// Gives a running Gateway CheapSeek as its chat model and sends it one chat
// request, for the install cells of installers.yml.
//
// Usage: node chat.mjs <discovery-file> <cheapseek-url>
//   <discovery-file>  the file a running Gateway writes its port and key to,
//                     ~/.runvil/run/gateway.json
//   <cheapseek-url>   the base URL a running CheapSeek prints, such as
//                     http://127.0.0.1:54321/v1
//
// CheapSeek (https://github.com/cppalliance/cheapseek) is a test server that
// speaks the OpenAI chat API and computes each reply from fixed rules.
//
// The Gateway's first start writes a config that holds speech models and no
// chat model. This script adds CheapSeek to that config as an endpoint with
// one model, through the routes the Gateway's Settings page saves with, so
// the rest of the config stays as the first start wrote it.
//
// Needs Node 22 or later and no install.
import { writeSync } from "node:fs";
import { readFile } from "node:fs/promises";

// The name of the endpoint and of its model in the Gateway's config, and
// the name CheapSeek gives the model it serves.
const NAME = "cheapseek";
// The Gateway applies a config change after the speech download its first
// start began, which is about 600 MB.
const LIMIT_MS = 10 * 60 * 1000;

// The line is written before the exit: a line queued on a pipe can be lost
// when the process exits.
function fail(message) {
  writeSync(2, `chat: ${message}\n`);
  process.exit(1);
}

const [discoveryFile, cheapseekUrl] = process.argv.slice(2);
if (!discoveryFile || !cheapseekUrl || process.argv.length !== 4) {
  fail(`usage: chat.mjs <discovery-file> <cheapseek-url>; got ${process.argv.length - 2} arguments`);
}

const { port, api_key: key } = JSON.parse(await readFile(discoveryFile, "utf8"));
const origin = `http://127.0.0.1:${port}`;

// One request to the Gateway. Anything but a 2xx answer with a JSON body
// ends the script.
async function call(method, path, body) {
  let response;
  let text;
  try {
    response = await fetch(origin + path, {
      method,
      headers: { authorization: `Bearer ${key}`, "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(LIMIT_MS),
    });
    text = await response.text();
  } catch (error) {
    fail(`${method} ${origin}${path} did not answer: ${error.cause?.message ?? error.message}`);
  }
  if (!response.ok) {
    fail(`${method} ${origin}${path} answered ${response.status}, required 200: ${text}`);
  }
  return JSON.parse(text);
}

const config = await call("GET", "/admin/config");
config.endpoint.push({ id: NAME, protocol: "openai", base_url: cheapseekUrl, api_key: "" });
config.model.push({
  name: NAME,
  description: "CheapSeek, a test server that answers by rule",
  context: 32768,
  upstream: NAME,
  endpoints: [NAME],
});
await call("PUT", "/admin/config", config);
await call("POST", "/admin/config-apply");

const listed = await call("GET", "/v1/models");
const model = listed.data.find((entry) => entry.id === NAME);
if (model?.kind !== "chat") {
  fail(`${origin}/v1/models must list the chat model ${NAME}; it lists ${JSON.stringify(listed.data)}`);
}

const reply = await call("POST", "/v1/chat/completions", {
  model: NAME,
  messages: [{ role: "user", content: "Hello." }],
});
const answer = reply.choices?.[0]?.message?.content;
if (typeof answer !== "string" || answer.trim() === "") {
  fail(`the Gateway's reply must hold text in choices[0].message.content; it is ${JSON.stringify(reply)}`);
}
console.log(`chat: the Gateway at ${origin} answered a chat request with ${JSON.stringify(answer)}`);
