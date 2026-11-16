---
name: greet
description: Greets the operator by name
runvil: 0
input: { path: name.txt, description: The operator's name }
output: { path: reply.txt, description: The greeting }
---

# Greet

## Answer

```lua
store.write('reply.txt', 'Hello, ' .. store.read('name.txt') .. '.')
```
