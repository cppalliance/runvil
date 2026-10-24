---
name: chat
description: The built-in Workshop chat agent on the unified runtime.
runvil: 0
uses:
  - web
  - user-input
models:
  chat:
    min_context: 32768
    description: "A conversational frontier model suitable for long-lived operator sessions"
---

# Chat

The built-in chat agent: a transparent pass-through between the operator
and the selected model. The conversation is the section's `messages` list,
retained across turns; the model is the dropdown's selection bound at run
launch, so a selection change takes effect on the next run. When the
conversation outgrows the model's context window, older turns are
summarized so the conversation continues.

```lua
models.default("chat")
```

## Conversation

```lua
tools.offer("web")
tools.offer(tools.packs.extras())
local function compact(reason)
  local first = 1
  while messages[first] and messages[first].role == "system" do
    first = first + 1
  end
  local cut, turns = first, 0
  for i = #messages, first, -1 do
    if messages[i].role == "user" then
      turns = turns + 1
      if turns == 2 then cut = i break end
    end
  end
  if cut == first then return compactors.fail(reason) end
  local lines, room = {}, 48000
  for i = cut - 1, first, -1 do
    local text = json.encode(messages[i].content)
    if #text > 1500 then text = text:sub(1, utf8.offset(text, 0, 1501) - 1) end
    local line = messages[i].role .. ": " .. text
    if #line > room then break end
    room = room - #line
    table.insert(lines, 1, line)
  end
  local summary = models.infer("Summarize this conversation:\n" .. table.concat(lines, "\n"))
  messages:replace(first, cut - 1, {
    role = "user",
    content = "Summary of the earlier conversation: " .. summary,
    meta = { summary = true },
  })
end
while true do
    local text = input.ask()
    messages:user(text)
    pcall(function() return models.loop(compact) end)
end
```
