local M = {}

function M.pack(...)
  return { n = select("#", ...), ... }
end

function M.notify(message, level)
  vim.notify(message, level or vim.log.levels.INFO, { title = "Phenix" })
end

function M.request_poll(request)
  local result = M.pack(request:poll())
  if result.n == 0 then
    return false
  end
  if result[1] == nil and result[2] ~= nil then
    return true, nil, result[2]
  end
  return true, result[1], nil
end

function M.safe_call(callback, ...)
  if callback == nil then
    return
  end
  local ok, error = pcall(callback, ...)
  if not ok then
    M.notify(error, vim.log.levels.ERROR)
  end
end

return M
