local M = {}

function M.new()
  return {
    items = {},
    next_id = 1,
    revision = 0,
  }
end

function M.touch(document)
  document.revision = document.revision + 1
end

function M.add(document, item)
  local value = vim.deepcopy(item)
  value.id = value.id or ("ref-" .. document.next_id)
  document.next_id = document.next_id + 1
  document.items[value.id] = value
  M.touch(document)
  return value
end

function M.get(document, id)
  return document.items[id]
end

function M.remove(document, id)
  if document.items[id] ~= nil then
    document.items[id] = nil
    M.touch(document)
  end
end

function M.reconcile(document, active)
  local removed = false
  for id in pairs(document.items) do
    if not active[id] then
      document.items[id] = nil
      removed = true
    end
  end
  if removed then
    M.touch(document)
  end
end

function M.clear(document)
  document.items = {}
  M.touch(document)
end

return M
