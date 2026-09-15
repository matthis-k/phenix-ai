local image = require("phenix_nvim.image")
local model = require("phenix_nvim.compose.model")

local M = {}
local namespace = vim.api.nvim_create_namespace("phenix-compose")
local preview_group = vim.api.nvim_create_augroup("phenix-compose-preview", { clear = true })
local buffer
local markers = {}
local previews = {}
local preview_document
local preview_win

local function valid_window(win)
  return win ~= nil and vim.api.nvim_win_is_valid(win)
end

local function close_preview(id)
  local preview = previews[id]
  if preview ~= nil then
    image.close(preview)
    previews[id] = nil
  end
end

local function close_previews()
  for id in pairs(previews) do
    close_preview(id)
  end
end

local function marker_position(target, item)
  local extmark = markers[item.id]
  if extmark == nil then
    return nil
  end
  local position = vim.api.nvim_buf_get_extmark_by_id(target, namespace, extmark, {})
  if type(position) ~= "table" or #position < 2 then
    return nil
  end
  local row, column = position[1], position[2]
  local marker = M.marker(item)
  local ok, text = pcall(vim.api.nvim_buf_get_text, target, row, column, row, column + #marker, {})
  if not ok or table.concat(text, "\n") ~= marker then
    return nil
  end
  return row, column
end

local function placement(win, row, column)
  local position = vim.fn.screenpos(win, row + 1, column + 1)
  if type(position) ~= "table" or position.row == nil or position.col == nil then
    return nil
  end
  if position.row <= 0 or position.col <= 0 then
    return nil
  end
  return {
    row = position.row,
    col = position.col,
  }
end

function M.refresh_previews(document, win)
  if buffer == nil or not vim.api.nvim_buf_is_valid(buffer) then
    close_previews()
    return
  end
  win = win or preview_win
  if not valid_window(win) or vim.api.nvim_win_get_buf(win) ~= buffer then
    close_previews()
    return
  end

  for id in pairs(previews) do
    local item = model.get(document, id)
    if item == nil or item.kind ~= "image" or markers[id] == nil then
      close_preview(id)
    end
  end

  for id, extmark in pairs(markers) do
    local item = model.get(document, id)
    if item == nil or item.kind ~= "image" then
      close_preview(id)
    else
      local row, column = marker_position(buffer, item)
      local where = row ~= nil and placement(win, row, column) or nil
      if where == nil then
        close_preview(id)
      elseif previews[id] ~= nil then
        if not image.update(previews[id], where) then
          close_preview(id)
          previews[id] = image.preview(item, where)
        end
      else
        previews[id] = image.preview(item, where)
      end
    end
    if extmark == nil then
      close_preview(id)
    end
  end
end

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
      M.refresh_previews(document)
    end,
  })
  vim.api.nvim_create_autocmd("BufWipeout", {
    buffer = buffer,
    callback = function()
      close_previews()
      markers = {}
      preview_document = nil
      preview_win = nil
      buffer = nil
    end,
  })
  return buffer
end

function M.attach_window(document, win)
  local target = M.ensure(document)
  if not valid_window(win) or vim.api.nvim_win_get_buf(win) ~= target then
    return
  end
  preview_document = document
  preview_win = win
  vim.api.nvim_clear_autocmds({ group = preview_group })
  vim.api.nvim_create_autocmd({ "WinScrolled", "WinResized", "VimResized" }, {
    group = preview_group,
    callback = function()
      vim.schedule(function()
        if preview_document ~= nil then
          M.refresh_previews(preview_document, preview_win)
        end
      end)
    end,
  })
  M.refresh_previews(document, win)
end

function M.detach_window(win)
  if win ~= nil and preview_win ~= win then
    return
  end
  close_previews()
  vim.api.nvim_clear_autocmds({ group = preview_group })
  preview_document = nil
  preview_win = nil
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
  markers[item.id] = vim.api.nvim_buf_set_extmark(target, namespace, row, column, {
    end_row = row,
    end_col = column + #marker,
    hl_group = "Special",
    virt_text = { { label(item), "Comment" } },
    right_gravity = false,
  })
  model.touch(document)
  vim.api.nvim_win_set_cursor(win, { row + 1, column + #marker })
  M.refresh_previews(document, win)
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
  local result, error = M.serialize_text(text, document)
  for id in pairs(markers) do
    if model.get(document, id) == nil then
      close_preview(id)
      markers[id] = nil
    end
  end
  M.refresh_previews(document)
  return result, error
end

function M.clear(document)
  local target = M.ensure(document)
  close_previews()
  markers = {}
  vim.api.nvim_buf_set_lines(target, 0, -1, false, { "" })
  vim.api.nvim_buf_clear_namespace(target, namespace, 0, -1)
  model.clear(document)
end

return M