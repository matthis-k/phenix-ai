local native = require("phenix")
local interaction = require("phenix_nvim.interaction")
local util = require("phenix_nvim.util")

local M = {}
local uv = vim.uv or vim.loop

local required_operations = {
  "session_create",
  "session_list",
  "session_resume",
  "session_close",
  "prompt",
  "cancel",
  "interaction_handlers_set",
  "review_decide",
}

local required_capabilities = {
  "phenix.application.capability.discovery@1",
  "phenix.application.capability.sessions@1",
  "phenix.application.capability.session-list@1",
  "phenix.application.capability.session-resume@1",
  "phenix.application.capability.prompt@1",
  "phenix.application.capability.sdk@1",
  "phenix.application.capability.capabilities@1",
  "phenix.application.capability.interaction@1",
  "phenix.application.capability.permission@1",
  "phenix.application.capability.elicitation@1",
  "phenix.application.capability.review@1",
}

local state = {
  config = nil,
  client = nil,
  sdk = nil,
  application = nil,
  active_session_id = nil,
  session_state = nil,
  session_observable_version = nil,
  session_stop = nil,
  subscription = nil,
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

local function stop_subscription()
  state.subscription = nil
  local stop = state.session_stop
  state.session_stop = nil
  if stop == nil then
    return
  end
  local ok, request = pcall(stop)
  if ok and request ~= nil then
    M.track(request)
  end
end

local function fail(error)
  stop_subscription()
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

local function session_resource()
  local phenix = state.sdk and state.sdk.phenix
  local sessions = phenix and phenix.sessions
  local resource = sessions and sessions.state
  if type(resource) ~= "table" or type(resource.get) ~= "function" or type(resource.listen) ~= "function" then
    return nil, "missing SDK callable phenix.sessions.state.get/listen"
  end
  return resource
end

local function required_application_error(capabilities)
  for _, name in ipairs(required_operations) do
    if type(state.application[name]) ~= "function" then
      return "missing application operation " .. name
    end
  end
  for _, capability in ipairs(required_capabilities) do
    if capabilities[capability] ~= true then
      return "missing negotiated application capability " .. capability
    end
  end
  return nil
end

function M.defer(start)
  if type(native.defer) ~= "function" then
    error("native Phenix binding does not support deferred callbacks")
  end
  return native.defer(start)
end

local function permission_handler(request)
  return M.defer(function(resolve, _reject)
    vim.schedule(function()
      local ok = pcall(interaction.permission, request, resolve)
      if not ok then
        resolve({ kind = "Cancelled" })
      end
    end)
  end)
end

local function elicitation_handler(request)
  return M.defer(function(resolve, reject)
    vim.schedule(function()
      local ok, error = pcall(interaction.elicitation, request, function(response, form_error)
        if form_error ~= nil then
          reject(form_error)
        else
          resolve(response)
        end
      end)
      if not ok then
        reject(tostring(error))
      end
    end)
  end)
end

local function install_interaction_handlers(callback)
  M.track(state.application.interaction_handlers_set({
    handlers = {
      permission = permission_handler,
      elicitation = elicitation_handler,
    },
  }), callback)
end

local function replace_root_changes(value, changes)
  local next_value = value
  for _, item in ipairs(changes) do
    if item.kind ~= "Replace" or type(item.change) ~= "table" then
      return nil
    end
    local path = item.change.path
    if type(path) ~= "table" or type(path.segments) ~= "table" or #path.segments ~= 0 then
      return nil
    end
    next_value = item.change.value
  end
  return next_value
end

local function begin_subscription(callback)
  local resource, resource_error = session_resource()
  if resource == nil then
    util.safe_call(callback, nil, { message = resource_error })
    return
  end

  stop_subscription()
  local subscription = {}
  state.subscription = subscription
  local function delivery(received)
    if state.subscription ~= subscription then
      return nil
    end
    local payload = received:payload()
    local version = received:version()
    if type(payload) ~= "table" or type(version) ~= "number" then
      M.refresh_session_state()
      return nil
    end
    if payload.kind == "Full" then
      state.session_state = payload.value
      state.session_observable_version = version
      emit("sessions", state.session_state)
      return nil
    end
    if payload.kind ~= "Diff" or received:from_version() ~= state.session_observable_version then
      M.refresh_session_state()
      return nil
    end
    local next_value = replace_root_changes(state.session_state, payload.changes)
    if next_value == nil then
      M.refresh_session_state()
      return nil
    end
    state.session_state = next_value
    state.session_observable_version = version
    emit("sessions", state.session_state)
    return nil
  end

  local request = resource.listen({
    path = { segments = {} },
    scope = { kind = "recursive" },
    mode = { kind = "diff" },
    initial = { kind = "full" },
    listener = delivery,
  })
  M.track(request, function(stop, error)
    if state.subscription ~= subscription then
      return
    end
    if error ~= nil then
      state.subscription = nil
      util.safe_call(callback, nil, error)
      return
    end
    state.session_stop = stop
    util.safe_call(callback, state.session_state, nil)
  end)
end

function M.refresh_session_state(callback)
  stop_subscription()
  local resource, resource_error = session_resource()
  if resource == nil then
    util.safe_call(callback, nil, { message = resource_error })
    return
  end
  M.track(resource.get(), function(result, error)
    if error ~= nil then
      util.safe_call(callback, nil, error)
      return
    end
    if type(result) ~= "table" or type(result.version) ~= "number" or result.value == nil then
      util.safe_call(callback, nil, { message = "invalid phenix.sessions.state.get response" })
      return
    end
    state.session_state = result.value
    state.session_observable_version = result.version
    emit("sessions", state.session_state)
    begin_subscription(callback)
  end)
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
    local capabilities = client:capabilities()
    local application_error = required_application_error(capabilities)
    local _, resource_error = session_resource()
    if application_error ~= nil or resource_error ~= nil then
      local message = application_error or resource_error
      fail({ message = message })
      util.safe_call(callback, nil, { message = message })
      return
    end
    install_interaction_handlers(function(_, handler_error)
      if handler_error ~= nil then
        fail(handler_error)
        util.safe_call(callback, nil, handler_error)
        return
      end
      M.refresh_session_state(function(_, state_error)
        if state_error ~= nil then
          fail(state_error)
          util.safe_call(callback, nil, state_error)
          return
        end
        state.connection = "connected"
        emit("status", M.status())
        util.safe_call(callback, state, nil)
      end)
    end)
  end)
end

function M.disconnect()
  stop_subscription()
  stop_timer()
  state.client = nil
  state.sdk = nil
  state.application = nil
  state.active_session_id = nil
  state.session_state = nil
  state.session_observable_version = nil
  state.pending = {}
  state.connection = "disconnected"
  state.error = nil
  emit("status", M.status())
end

local function require_application(callback)
  if state.application == nil or state.connection == "failed" or state.connection == "disconnected" then
    util.safe_call(callback, nil, { message = "Phenix is not connected" })
    return false
  end
  return true
end

function M.new_session(callback)
  if not require_application(callback) then
    return
  end
  M.track(state.application.session_create({
    working_directory = vim.fn.getcwd(),
    title = nil,
  }), function(session, error)
    if error == nil then
      state.active_session_id = session.session_id
      emit("status", M.status())
    end
    util.safe_call(callback, session, error)
  end)
end

function M.resume_session(session_id, callback)
  if not require_application(callback) then
    return
  end
  M.track(state.application.session_resume({
    session_id = session_id,
    after_sequence = nil,
  }), function(snapshot, error)
    if error == nil then
      state.active_session_id = snapshot.session.session_id
      emit("status", M.status())
    end
    util.safe_call(callback, snapshot, error)
  end)
end

function M.list_sessions(callback)
  if not require_application(callback) then
    return
  end
  M.track(state.application.session_list({ cursor = nil }), callback)
end

function M.close_session(session_id, callback)
  if not require_application(callback) then
    return
  end
  M.track(state.application.session_close({ session_id = session_id }), function(result, error)
    if error == nil and state.active_session_id == session_id then
      state.active_session_id = nil
      emit("status", M.status())
    end
    util.safe_call(callback, result, error)
  end)
end

function M.active_session()
  return state.active_session_id
end

local function application_content(segments)
  local content = {}
  for _, segment in ipairs(segments) do
    if segment.kind == "text" then
      table.insert(content, { kind = "Text", text = segment.text })
    elseif segment.kind == "resource" or segment.kind == "location" or segment.kind == "selection" then
      local source = segment.source or {}
      if type(source.uri) ~= "string" or source.uri == "" then
        return nil, "resource is missing a source URI"
      end
      table.insert(content, {
        kind = "Resource",
        uri = source.uri,
        mime_type = segment.snapshot and "text/plain" or nil,
        text = segment.snapshot,
      })
    elseif segment.kind == "image" then
      table.insert(content, {
        kind = "Image",
        mime_type = segment.mime_type,
        data = segment.bytes,
      })
    else
      return nil, "unsupported compose item " .. tostring(segment.kind)
    end
  end
  return content
end

function M.prompt(session_id, segments, callback)
  if not require_application(callback) then
    return
  end
  local content, content_error = application_content(segments)
  if content == nil then
    util.safe_call(callback, nil, { message = content_error })
    return
  end
  M.track(state.application.prompt({
    session_id = session_id,
    content = content,
  }), callback)
end

function M.cancel_active()
  if state.application == nil or state.active_session_id == nil then
    return
  end
  M.track(state.application.cancel({ session_id = state.active_session_id }), function(_, error)
    if error ~= nil then
      util.notify(vim.inspect(error), vim.log.levels.ERROR)
    end
  end)
end

function M.decide_review(review_id, expected_revision, decision, callback)
  if type(review_id) ~= "string" or review_id == "" then
    util.safe_call(callback, nil, { message = "review id is required" })
    return
  end
  if type(expected_revision) ~= "number" then
    util.safe_call(callback, nil, { message = "review revision is required" })
    return
  end
  if decision ~= "Accept" and decision ~= "Reject" then
    util.safe_call(callback, nil, { message = "invalid review decision" })
    return
  end
  if not require_application(callback) then
    return
  end
  if type(state.application.review_decide) ~= "function" then
    util.safe_call(callback, nil, { message = "review decisions are unavailable" })
    return
  end
  M.track(state.application.review_decide({
    review_id = review_id,
    expected_revision = expected_revision,
    decision = { kind = decision },
  }), callback)
end

function M.status()
  return {
    connection = state.connection,
    error = state.error,
    session_id = state.active_session_id,
    sdk_ready = state.sdk ~= nil,
  }
end

function M.sdk()
  return state.sdk
end

function M.application()
  return state.application
end

function M.session_state()
  return state.session_state
end

return M
