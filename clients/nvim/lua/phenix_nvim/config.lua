local M = {}

local defaults = {
  command = "phenix-acp",
  args = {},
  env = {},
  auto_connect = false,
  poll_interval_ms = 25,
  poll_budget = 32,
  side = "right",
  width = 56,
  compose_height = 8,
}

local current = vim.deepcopy(defaults)

function M.setup(options)
  current = vim.tbl_deep_extend("force", vim.deepcopy(defaults), options or {})
  return M.get()
end

function M.get()
  return vim.deepcopy(current)
end

return M
