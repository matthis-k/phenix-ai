local model = require("phenix_nvim.compose.model")

local M = {}
local namespace = vim.api.nvim_create_namespace("phenix-compose")
local buffer

function M.ensure(document)
  if buffer ~= nil and vim.api.nvim_buf_is_valid(buffer) then
    return buffer
  end
  buffer = vim.api.nvim_create_buf(false, true)
  vim.bo[buffer].buftype = "nofile"
  vim.bo[buffer].bufhidden = "hide"
  vim.bo[buffer].swapfile = false
  vim.bo[buffer].filetype = "markdown"
  vim.api.nvim_buf_set_name(buffer, "phenix://compose")
  vim.api.nvim_create_autocmd({ "TextChanged", "TextChangedI" }, {
    buffer = buffer,
    callback = function()
      model.touch(document)
    end,
  })
  return buffer
end

function M.marker(item)
  return "⟦phenix:" .. item.id .. "⟧"
end

local function label(item)
  if item.kind == "selection" and item.source then
    return string.format(" %s:%d-%d", item.kind, item.source.start_line + 1, item.source.end_line + 1)
  end
  return " " .. item.kind
end

function M.insert(document, item, win)
  local target = M.ensure(document)
  win = win or vim.api.nvim_get_current_win()
  local cursor = vim.api.nvim_win_get_cursor(win)
  local row = cursor[1] - 1
  local column = cursor[2]
  local marker = M.marker(item)
  vim.api.nvim_buf_set_text(target, row, column, row, column, { marker })
  vim.api.nvim_buf_set_extmark(target, namespace, row, column, {
    end_row = row,
    end_col = column + #marker,
    hl_group = "Special",
    virt_text = { { label(item), "Comment" } },
    right_gravity = false,
  })
  model.touch(document)
  vim.api.nvim_win_set_cursor(win, { row + 1, column + #marker })
end

function M.serialize_text(text, document)
  local result = {}
  local active = {}
  local cursor = 1
  while true do
    local first, last, id = text:find("⟦phenix:([%w_%-]+)⟧", cursor)
    if first == nil then
      break
    end
    if first > cursor then
      table.insert(result, { kind = "text", text = text:sub(cursor, first - 1) })
    end
    local item = model.get(document, id)
    if item == nil then
      return nil, "compose marker references missing item " .. id
    end
    active[id] = true
    table.insert(result, vim.deepcopy(item))
    cursor = last + 1
  end
  if cursor <= #text then
    table.insert(result, { kind = "text", text = text:sub(cursor) })
  end
  model.reconcile(document, active)
  return result
end

function M.serialize(document)
  local target = M.ensure(document)
  local text = table.concat(vim.api.nvim_buf_get_lines(target, 0, -1, false), "\n")
  return M.serialize_text(text, document)
end

function M.clear(document)
  local target = M.ensure(document)
  vim.api.nvim_buf_set_lines(target, 0, -1, false, { "" })
  vim.api.nvim_buf_clear_namespace(target, namespace, 0, -1)
  model.clear(document)
end

return M
