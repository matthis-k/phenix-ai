local runtime = require("phenix_nvim.runtime")
local util = require("phenix_nvim.util")

local M = {}

function M.new(callback)
  runtime.new_session(callback)
end

function M.resume(session_id, callback)
  runtime.resume_session(session_id, callback)
end

function M.choose()
  runtime.list_sessions(function(result, error)
    if error ~= nil then
      util.notify(vim.inspect(error), vim.log.levels.ERROR)
      return
    end
    local sessions = result and (result.sessions or result) or {}
    vim.ui.select(sessions, {
      prompt = "Phenix sessions",
      format_item = function(item)
        return item.title or item.session_id or item.id or vim.inspect(item)
      end,
    }, function(item)
      if item ~= nil then
        runtime.resume_session(item.session_id or item.id, function(_, resume_error)
          if resume_error ~= nil then
            util.notify(vim.inspect(resume_error), vim.log.levels.ERROR)
          end
        end)
      end
    end)
  end)
end

return M
