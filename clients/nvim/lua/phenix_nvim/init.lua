local actions = require("phenix_nvim.actions")
local config = require("phenix_nvim.config")
local runtime = require("phenix_nvim.runtime")
local status = require("phenix_nvim.status")

local M = {
  actions = actions,
  status = status,
}

function M.setup(options)
  local resolved = config.setup(options)
  runtime.configure(resolved)
  if resolved.auto_connect then
    runtime.connect()
  end
  return M
end

M.connect = runtime.connect
M.disconnect = runtime.disconnect
M.reference = actions.reference
M.reference_at = actions.reference_at
M.reference_picker = actions.reference_picker
M.send = actions.send
M.toggle = actions.toggle
M.cancel = actions.cancel
M.new_session = actions.new_session
M.choose_session = actions.choose_session
M.attach_image = actions.attach_image

return M
