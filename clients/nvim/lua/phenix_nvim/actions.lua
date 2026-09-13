local compose = require("phenix_nvim.compose.buffer")
local compose_model = require("phenix_nvim.compose.model")
local context = require("phenix_nvim.context")
local image = require("phenix_nvim.image")
local runtime = require("phenix_nvim.runtime")
local sessions = require("phenix_nvim.sessions")
local sidebar = require("phenix_nvim.sidebar")
local state = require("phenix_nvim.state")
local util = require("phenix_nvim.util")

local M = {}

local function insert(item)
  local stored = compose_model.add(state.compose, item)
  local win = sidebar.focus_compose()
  compose.insert(state.compose, stored, win)
end

function M.reference()
  local mode = vim.fn.mode(1)
  local item, error
  if mode == "v" or mode == "V" or mode == "\22" then
    item, error = context.visual_selection()
  else
    item = context.current_location()
  end
  if item == nil then
    util.notify(error, vim.log.levels.ERROR)
    return
  end
  insert(item)
end

function M.reference_at(value)
  local item, error = context.typed_reference(value)
  if item == nil then
    util.notify(error, vim.log.levels.ERROR)
    return
  end
  insert(item)
end

function M.reference_picker()
  context.pick_reference(function(item, error)
    if error ~= nil then
      util.notify(error, vim.log.levels.ERROR)
      return
    end
    if item ~= nil then
      insert(item)
    end
  end)
end

function M.attach_image(path)
  local function attach(value)
    if value == nil or value == "" then
      return
    end
    local item, error = image.from_file(value)
    if item == nil then
      util.notify(error, vim.log.levels.ERROR)
      return
    end
    insert(item)
  end
  if path ~= nil then
    attach(path)
  else
    vim.ui.input({ prompt = "Image file: ", completion = "file" }, attach)
  end
end

local function submit(session, content, revision)
  runtime.prompt(session, content, function(_, error)
    if error ~= nil then
      util.notify(vim.inspect(error), vim.log.levels.ERROR)
      return
    end
    if state.compose.revision == revision then
      compose.clear(state.compose)
    end
  end)
end

function M.send()
  local content, error = compose.serialize(state.compose)
  if content == nil then
    util.notify(error, vim.log.levels.ERROR)
    return
  end
  if #content == 0 or (#content == 1 and content[1].kind == "text" and content[1].text == "") then
    util.notify("compose buffer is empty", vim.log.levels.WARN)
    return
  end
  local revision = state.compose.revision
  local session = runtime.active_session()
  if session ~= nil then
    submit(session, content, revision)
    return
  end
  runtime.new_session(function(created, create_error)
    if create_error ~= nil then
      util.notify(vim.inspect(create_error), vim.log.levels.ERROR)
      return
    end
    submit(created, content, revision)
  end)
end

function M.toggle()
  sidebar.toggle()
end

function M.cancel()
  runtime.cancel_active()
end

function M.new_session()
  sessions.new(function(_, error)
    if error ~= nil then
      util.notify(vim.inspect(error), vim.log.levels.ERROR)
    end
  end)
end

function M.choose_session()
  sessions.choose()
end

return M
