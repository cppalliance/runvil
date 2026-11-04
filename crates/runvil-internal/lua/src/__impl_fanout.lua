-- The `fanout` shim for a section VM: Lua over the task
-- protocol (`spawn`, `join_any`, `cancel`), so every wait inside a fanout
-- is an ordinary yield and the scheduler keeps no fanout state of its own.
--
-- The Engine installs this after the coroutine prelude (`__impl_coro.lua`)
-- and installs the returned function as `tasks.fanout`. The chunk
-- arguments are privileged captures, never globals: `yield` wraps
-- coroutine.yield, `var_snapshot` is the Engine helper returning the hidden
-- `var` data table as a plain deep copy, `helpers` is the prelude's shared
-- trio (`raise(kind, fields)` builds and raises the structured error
-- table, `fail(result)` raises an envelope's failure value, and
-- `engine_type(value)` names a value's type as Lua's `type()` does, with
-- an integer named "integer"),
-- `collection_members(collection)` enumerates a collection as `(members)`
-- or `(nil, message)` - the array part in order, then the hash part as
-- `{ key, value }` pairs sorted by key - and `render_item(item)` renders a
-- member as `{{ item }}` would. The last three captures reach the chain's
-- current model: `get_selection()` returns its handle, or nil when the
-- caller supplied no model, `set_selection(handle_or_nil)` sets it (nil
-- clears it) and refuses any other value, and `is_model_handle(value)`
-- reports whether a value is a model handle.
local yield, var_snapshot, helpers,
  collection_members, render_item,
  get_selection, set_selection, is_model_handle = ...

local raise, fail = helpers.raise, helpers.fail

-- The base functions the shim uses, captured at install so its behavior does
-- not move when author code rebinds the base globals.
local setmetatable, type, select = setmetatable, type, select

-- One fanout arm's result: a frozen, methodless object whose fields
-- are `text`, `ok`, `item`, and `exhausted`, with `tostring` giving the
-- text so a `table.concat` over the results keeps working. Writes raise:
-- the result is the arm's record, not the author's scratch space. The
-- seal has three parts: the fields live in a hidden `__index` table the
-- author cannot reach, `__newindex` refuses every assignment, and
-- `__metatable` hands `getmetatable` a decoy (containing only `__tostring`,
-- so the hardened `table.concat` still recognizes the result as
-- renderable) and makes `setmetatable` refuse to replace the guard. The
-- one remaining bypass would be `rawset`, which the VM's hardening pass
-- removes from the globals before any author code runs.
local function fanout_result(item, text, ok, exhausted)
  local fields = { text = text, ok = ok, item = item, exhausted = exhausted }
  local function render() return text end
  return setmetatable({}, {
    __index = fields,
    __newindex = function() error("tasks.fanout results are read-only", 2) end,
    __tostring = render,
    __metatable = { __tostring = render },
  })
end

-- The incomplete stub an exhausted arm's slot receives: one stuck arm must
-- not kill its siblings' evidence, so its text says what happened.
local function exhausted_stub(item)
  return "## " .. render_item(item) .. "\n\nUNKNOWN\n\n(section incomplete: tool loop exhausted)"
end

-- tasks.fanout(worker, collection) and tasks.fanout(model, worker,
-- collection): run `worker` once per collection member as a task chain and
-- return the results in collection order. The model-first form takes a
-- model handle first and runs every arm on it: it sets the chain's
-- current model to the handle before the spawns, so each arm copies it when
-- it is dispatched, and puts the saved model back before the first join and
-- before a spawn failure is re-raised. No author code runs in the parent
-- between the switch and the restore, and a model switch is not an event,
-- so the switch cannot be observed. The first argument is checked before
-- anything is switched. The
-- members enumerate (array part in order, then the hash part as
-- `{ key, value }` pairs sorted by key), an empty collection raises before
-- any spawn, and every arm is spawned up front - before any join - so no
-- arm is ordered after a sibling: arms never see each other's writes,
-- whatever the scheduler's admission order. Each arm is spawned with the
-- member as its `item` and its 1-based position as `sys.index`; a spawn
-- refusal, such as the depth cap's, re-raises here as the retained typed
-- error. The scheduler admits the arms under the chain's
-- concurrency limit, and `join_any` over the live set delivers the arms
-- as they end. A `tool_loop_exhausted` arm
-- becomes the incomplete stub and the fanout continues; any other arm
-- failure cancels the live arms and re-raises. No arm outlives the call:
-- every arm is delivered or cancelled before the function returns or
-- raises.
local function fanout(...)
  local argc = select("#", ...)
  local model, worker, collection
  if argc == 2 then
    worker, collection = ...
  elseif argc == 3 then
    model, worker, collection = ...
    if not is_model_handle(model) then
      raise("lua", {
        message = "tasks.fanout: the first argument must be a model handle, got " .. type(model),
      })
    end
  else
    raise("lua", {
      message = "tasks.fanout takes (worker, collection) or (model, worker, collection)",
    })
  end
  local members, message = collection_members(collection)
  if not members then
    raise("lua", { message = message })
  end
  local count = #members
  if count == 0 then
    raise("lua", { message = "tasks.fanout over an empty collection: no work is likely a bug" })
  end
  -- One snapshot serves every arm: the caller is suspended inside this
  -- call, so its `var` cannot move between spawns.
  local var = var_snapshot()
  local results = {}
  -- The live arms: `slot_of[id]` is the arm's collection index, `live`
  -- the ids in spawn order (the `join_any` set, so an earlier arm wins a
  -- tie).
  local slot_of, live = {}, {}

  -- Ends every live arm. Best effort on an error path: a refused cancel
  -- would only mask the failure already being raised.
  local function cancel_live()
    for _, id in ipairs(live) do
      yield({ op = "cancel", task = id })
    end
    live, slot_of = {}, {}
  end

  -- The model-first form switches the chain's current model for the spawns
  -- and puts it back once, whichever path gets there first.
  local switched, saved = false, nil
  if model ~= nil then
    saved = get_selection()
    set_selection(model)
    switched = true
  end
  local function restore()
    if switched then
      switched = false
      set_selection(saved)
    end
  end

  -- Every arm is spawned up front, before the first join: the waits
  -- deliver and join the arms one by one, and a late arm that joined an
  -- earlier one's writes would be ordered after it, making its reads
  -- timing-dependent. The queued arms hold no Lua VM until the scheduler
  -- admits them, so the run's concurrency limit bounds the fanout's
  -- memory.
  for index = 1, count do
    local ok, result = yield({
      op = "spawn",
      target = worker,
      item = members[index],
      index = index,
      var = var,
    })
    if not ok then
      restore()
      cancel_live()
      fail(result)
    end
    slot_of[result] = index
    live[#live + 1] = result
  end
  restore()
  while #live > 0 do
    local ok, task, arm_ok, result = yield({ op = "join_any", tasks = live })
    if not ok then
      cancel_live()
      fail(task)
    end
    local index = slot_of[task]
    slot_of[task] = nil
    local rest = {}
    for _, id in ipairs(live) do
      if id ~= task then rest[#rest + 1] = id end
    end
    live = rest
    local item = members[index]
    if arm_ok then
      results[index] = fanout_result(item, result, true, false)
    elseif type(result) == "table" and result.kind == "tool_loop_exhausted" then
      results[index] = fanout_result(item, exhausted_stub(item), false, true)
    else
      cancel_live()
      fail(result)
    end
  end
  return results
end

return fanout
