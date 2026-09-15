local runtime = require("phenix_nvim.runtime")

local M = {}

local function execution_status(session_projection)
  if type(session_projection) ~= "table" then
    return nil, nil
  end
  local execution_id
  local execution_state
  for index = #(session_projection.updates or {}), 1, -1 do
    local update = session_projection.updates[index]
    local change = update and update.update
    if type(change) == "table" and change.kind == "Execution" then
      local execution = change.update
      if execution_id == nil then
        execution_id = change.execution_id
      end
      if change.execution_id == execution_id
        and type(execution) == "table"
        and execution.kind == "State"
      then
        execution_state = execution.state and execution.state.kind or nil
        break
      end
    end
  end
  return execution_id, execution_state
end

function M.get()
  local result = runtime.status()
  local session_id = runtime.active_session()
  local sessions = runtime.session_state()
  local projection = session_id ~= nil and sessions and sessions.sessions and sessions.sessions[session_id] or nil
  local execution_id, execution_state = execution_status(projection)
  result.execution_id = execution_id
  result.execution_state = execution_state
  return result
end

return M
