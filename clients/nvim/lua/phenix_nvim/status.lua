local runtime = require("phenix_nvim.runtime")

local M = {}

function M.get()
  return runtime.status()
end

return M
