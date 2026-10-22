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
launch, so a selection change takes effect on the next run.

```lua
models.default("chat")
```

## Conversation

```lua
packs.offer("web")
tools.offer(tools.extras())
while true do
    local text = input.ask()
    messages:user(text)
    pcall(function() return models.loop() end)
end
```
