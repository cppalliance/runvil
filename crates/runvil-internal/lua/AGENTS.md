# runvil-lua

- Engine functions that would create a parser-to-Lua dependency cycle stay here, not in `runvil-prompt`.
- Nothing in this crate performs a tool call. The executor issues each call as an effect, and `prepare_dispatch` (or `prepare_model_dispatch` for a model-issued call) applies the dispatch rules when the answer lands.
