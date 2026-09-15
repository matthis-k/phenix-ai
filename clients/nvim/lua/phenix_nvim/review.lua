local M = {}

local function state_kind(review)
  return type(review.state) == "table" and review.state.kind or nil
end

local function render(review, buffer)
  if not vim.api.nvim_buf_is_valid(buffer) then
    return
  end
  local state = state_kind(review) or "Pending"
  local lines = {
    "# Phenix review",
    "",
    "State: " .. state,
  }
  if state == "Conflicted" and review.state.message ~= nil then
    table.insert(lines, "Conflict: " .. review.state.message)
  end
  table.insert(lines, "")
  for _, file in ipairs(review.files or {}) do
    table.insert(lines, "## " .. (file.uri or "file"))
    if file.conflict ~= nil then
      table.insert(lines, "Conflict: " .. tostring(file.conflict))
      table.insert(lines, "")
    end
    for _, hunk in ipairs(file.hunks or {}) do
      table.insert(lines, "```diff")
      vim.list_extend(lines, vim.split(hunk.unified_diff or "", "\n", { plain = true }))
      table.insert(lines, "```")
    end
    table.insert(lines, "")
  end
  vim.bo[buffer].modifiable = true
  vim.api.nvim_buf_set_lines(buffer, 0, -1, false, lines)
  vim.bo[buffer].modifiable = false
end

local function decide(review, decision, buffer, updated_callback)
  if state_kind(review) ~= "Pending" then
    return
  end
  local runtime = require("phenix_nvim.runtime")
  runtime.decide_review(review.id, review.revision, decision, function(updated, error)
    if error ~= nil then
      vim.notify(vim.inspect(error), vim.log.levels.ERROR)
      return
    end
    if updated ~= nil then
      render(updated, buffer)
      updated_callback(updated)
      local state = state_kind(updated)
      if state == "Conflicted" then
        vim.notify("Phenix review conflicted", vim.log.levels.WARN)
      elseif state == "Accepted" then
        vim.notify("Phenix review accepted")
      elseif state == "Rejected" then
        vim.notify("Phenix review rejected")
      end
    end
  end)
end

function M.open(review)
  if type(review) ~= "table"
    or type(review.id) ~= "string"
    or type(review.revision) ~= "number"
    or type(review.files) ~= "table"
  then
    return nil, "invalid structured review"
  end
  local current = vim.deepcopy(review)
  local buffer = vim.api.nvim_create_buf(false, true)
  vim.bo[buffer].buftype = "nofile"
  vim.bo[buffer].bufhidden = "wipe"
  vim.bo[buffer].swapfile = false
  vim.bo[buffer].filetype = "diff"
  render(current, buffer)
  vim.keymap.set("n", "a", function()
    decide(current, "Accept", buffer, function(updated)
      current = updated
    end)
  end, { buffer = buffer, desc = "Accept Phenix review" })
  vim.keymap.set("n", "r", function()
    decide(current, "Reject", buffer, function(updated)
      current = updated
    end)
  end, { buffer = buffer, desc = "Reject Phenix review" })
  vim.cmd("tabnew")
  vim.api.nvim_win_set_buf(0, buffer)
  return buffer
end

return M
