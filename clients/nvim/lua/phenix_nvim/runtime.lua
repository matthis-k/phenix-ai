local native = require("phenix")
local util = require("phenix_nvim.util")

local M = {}
local uv = vim.uv or vim.loop

local state = {
  config = nil,
  client = nil,
  sdk = nil,
  application = nil,
  active_session = nil,
  connection = "disconnected",
  error = nil,
  timer = nil,
  pending = {},
  listeners = {},
}

local function emit(kind, value)
  for _, listener in ipairs(vim.deepcopy(state.listeners)) do
    util.safe_call(listener, kind, value)
  end
end

local function stop_timer()
  if state.timer ~= nil then
    state.timer:stop()
    state.timer:close()
    state.timer = nil
  end
end

local function fail(error)
  state.connection = "failed"
  state.error = error
  stop_timer()
  emit("status", M.status())
end

local function start_timer()
  stop_timer()
  local timer = uv.new_timer()
  state.timer = timer
  local interval = state.config.poll_interval_ms
  timer:start(interval, interval, vim.schedule_wrap(function()
    M.tick()
  end))
end

function M.configure(config)
  state.config = vim.deepcopy(config)
end

function M.on_event(listener)
  table.insert(state.listeners, listener)
  local index = #state.listeners
  return function()
    state.listeners[index] = function() end
  end
end

function M.track(request, callback)
  if request == nil then
    util.safe_call(callback, nil, { message = "native request was not created" })
    return
  end
  table.insert(state.pending, { request = request, callback = callback })
end

function M.tick()
  if state.client == nil then
    return
  end

  local budget = state.config.poll_budget
  for _ = 1, budget do
    local ok, event = pcall(state.client.poll, state.client)
    if not ok then
      fail(event)
      return
    end
    if event == nil then
      break
    end
    emit("update", event)
  end

  for index = #state.pending, 1, -1 do
    local item = state.pending[index]
    local ok, complete, value, error = pcall(util.request_poll, item.request)
    if not ok then
      table.remove(state.pending, index)
      util.safe_call(item.callback, nil, complete)
    elseif complete then
      table.remove(state.pending, index)
      util.safe_call(item.callback, value, error)
    end
  end
end

function M.connect(callback)
  if state.connection == "connected" then
    util.safe_call(callback, state, nil)
    return
  end
  local config = state.config or require("phenix_nvim.config").get()
  state.config = config
  state.connection = "connecting"
  state.error = nil

  local ok, client = pcall(native.connect, {
    command = config.command,
    args = config.args,
    env = config.env,
  })
  if not ok then
    fail(client)
    util.safe_call(callback, nil, client)
    return
  end

  state.client = client
  start_timer()
  M.track(client:sdk(), function(sdk, error)
    if error ~= nil then
      fail(error)
      util.safe_call(callback, nil, error)
      return
    end
    state.sdk = sdk
    state.application = client:application()
    state.connection = "connected"
    emit("status", M.status())
    util.safe_call(callback, state, nil)
  end)
end

function M.disconnect()
  stop_timer()
  state.client = nil
  state.sdk = nil
  state.application = nil
  state.active_session = nil
  state.pending = {}
  state.connection = "disconnected"
  state.error = nil
  emit("status", M.status())
end

function M.new_session(callback)
  if state.client == nil then
    util.safe_call(callback, nil, { message = "Phenix is not connected" })
    return
  end
  local request = state.client:sessions():new(vim.fn.getcwd())
  M.track(request, function(session, error)
    if error == nil then
      state.active_session = session
      emit("status", M.status())
    end
    util.safe_call(callback, session, error)
  end)
end

function M.resume_session(session_id, callback)
  if state.client == nil then
    util.safe_call(callback, nil, { message = "Phenix is not connected" })
    return
  end
  M.track(state.client:sessions():resume(session_id, vim.fn.getcwd()), function(session, error)
    if error == nil then
      state.active_session = session
      emit("status", M.status())
    end
    util.safe_call(callback, session, error)
  end)
end

function M.list_sessions(callback)
  if state.client == nil then
    util.safe_call(callback, nil, { message = "Phenix is not connected" })
    return
  end
  M.track(state.client:sessions():list(vim.fn.getcwd()), callback)
end

function M.active_session()
  return state.active_session
end

function M.prompt(session, content, callback)
  local text = {}
  for _, segment in ipairs(content) do
    if segment.kind ~= "text" then
      util.safe_call(callback, nil, {
        message = "multimodal prompt delivery requires the application SDK prompt callable",
      })
      return
    end
    table.insert(text, segment.text)
  end
  M.track(session:prompt(table.concat(text)), callback)
end

function M.cancel_active()
  if state.active_session ~= nil then
    local ok, error = pcall(state.active_session.cancel, state.active_session)
    if not ok then
      util.notify(error, vim.log.levels.ERROR)
    end
  end
end

function M.status()
  return {
    connection = state.connection,
    error = state.error,
    session_id = state.active_session and state.active_session:id() or nil,
    sdk_ready = state.sdk ~= nil,
  }
end

function M.sdk()
  return state.sdk
end

function M.application()
  return state.application
end

return M
