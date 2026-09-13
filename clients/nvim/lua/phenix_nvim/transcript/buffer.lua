local M = {}
local namespace = vim.api.nvim_create_namespace("phenix-transcript")
local buffer
local marks = {}
local attached_windows = {}
local follow_tail = true

local function lines_for(node)
  if node.kind == "message" then
    if node.role == "user" then
      return { "## You", "", node.text, "" }
    end
    return { node.text, "" }
  end
  if node.kind == "tool" then
    local output = node.output and vim.inspect(node.output) or ""
    return {
      "### Tool · " .. tostring(node.callable_id),
      "",
      "`" .. node.state .. "`",
      output,
      "",
    }
  end
  if node.kind == "progress" then
    return { "_" .. (node.message or "working") .. "_", "" }
  end
  return { vim.inspect(node), "" }
end

function M.ensure()
  if buffer ~= nil and vim.api.nvim_buf_is_valid(buffer) then
    return buffer
  end
  buffer = vim.api.nvim_create_buf(false, true)
  vim.bo[buffer].buftype = "nofile"
  vim.bo[buffer].bufhidden = "hide"
  vim.bo[buffer].modifiable = false
  vim.bo[buffer].swapfile = false
  vim.bo[buffer].filetype = "markdown"
  vim.api.nvim_buf_set_name(buffer, "phenix://transcript")
  marks = {}
  attached_windows = {}
  follow_tail = true
  return buffer
end

local function valid_window(win)
  return win ~= nil
    and vim.api.nvim_win_is_valid(win)
    and vim.api.nvim_win_get_buf(win) == M.ensure()
end

local function visible_bottom(win)
  local ok, line = pcall(vim.api.nvim_win_call, win, function()
    return vim.fn.line("w$")
  end)
  return ok and line or 0
end

local function update_follow_tail(win)
  if not valid_window(win) then
    attached_windows[win] = nil
    return
  end
  follow_tail = visible_bottom(win) >= vim.api.nvim_buf_line_count(M.ensure())
end

local function scroll_to_tail()
  if not follow_tail then
    return
  end
  local last = math.max(vim.api.nvim_buf_line_count(M.ensure()), 1)
  for win in pairs(attached_windows) do
    if valid_window(win) then
      pcall(vim.api.nvim_win_set_cursor, win, { last, 0 })
    else
      attached_windows[win] = nil
    end
  end
end

function M.attach_window(win)
  if not valid_window(win) or attached_windows[win] then
    return
  end
  attached_windows[win] = true
  vim.api.nvim_create_autocmd("WinScrolled", {
    pattern = tostring(win),
    callback = function()
      if not valid_window(win) then
        attached_windows[win] = nil
        return true
      end
      update_follow_tail(win)
    end,
  })
  vim.api.nvim_create_autocmd({ "CursorMoved", "CursorMovedI" }, {
    buffer = M.ensure(),
    callback = function()
      if vim.api.nvim_get_current_win() == win then
        update_follow_tail(win)
      end
    end,
  })
end

function M.detach_window(win)
  attached_windows[win] = nil
end

function M.set_follow_tail(enabled)
  follow_tail = enabled == true
  if follow_tail then
    scroll_to_tail()
  end
end

function M.is_following_tail()
  return follow_tail
end

local function replace(start_row, finish_row, lines)
  local target = M.ensure()
  vim.bo[target].modifiable = true
  vim.api.nvim_buf_set_lines(target, start_row, finish_row, false, lines)
  vim.bo[target].modifiable = false
end

function M.render_node(node)
  local target = M.ensure()
  local existing = marks[node.id]
  local lines = lines_for(node)
  local start_row
  if existing ~= nil then
    local position = vim.api.nvim_buf_get_extmark_by_id(target, namespace, existing, { details = true })
    if #position == 0 then
      marks[node.id] = nil
      return M.render_node(node)
    end
    start_row = position[1]
    local finish_row = (position[3].end_row or position[1]) + 1
    replace(start_row, finish_row, lines)
  else
    start_row = vim.api.nvim_buf_line_count(target)
    if start_row == 1 and vim.api.nvim_buf_get_lines(target, 0, 1, false)[1] == "" then
      start_row = 0
      replace(0, 1, lines)
    else
      replace(start_row, start_row, lines)
    end
  end
  local end_row = start_row + math.max(#lines - 1, 0)
  marks[node.id] = vim.api.nvim_buf_set_extmark(target, namespace, start_row, 0, {
    id = existing,
    end_row = end_row,
    end_col = #(lines[#lines] or ""),
    right_gravity = false,
  })
  scroll_to_tail()
end

function M.render_projection(projection)
  local target = M.ensure()
  vim.bo[target].modifiable = true
  vim.api.nvim_buf_set_lines(target, 0, -1, false, {})
  vim.api.nvim_buf_clear_namespace(target, namespace, 0, -1)
  vim.bo[target].modifiable = false
  marks = {}
  for _, id in ipairs(projection.order) do
    M.render_node(projection.nodes[id])
  end
  scroll_to_tail()
end

return M
