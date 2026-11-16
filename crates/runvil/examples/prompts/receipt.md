---
name: receipt
description: Calculate a receipt using a typed Rust tool
runvil: 0
uses:
  - examples/pricing
output: { path: receipt.txt, description: The calculated receipt }
---

# Receipt

## Calculate

```lua
local total = tools.call('pricing/total', {unit_price_cents = 1250, quantity = 3})
store.write('receipt.txt', string.format('3 items: %d cents', total.total_cents))
```
