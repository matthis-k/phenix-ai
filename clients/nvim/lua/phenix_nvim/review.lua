local M = {}

local function decide(review, decision, buffer)
  local runtime = require("phenix_nvim.runtime")
  runtime.decide_review(review.id, review.revision, decision, function(updated, error)
    if error ~= nil then
      vim.notify(vim.inspect(error), vim.log.levels.ERROR)
      return
    end
    if updated ~= nil and vim.api.nvim_buf_is_valid(buffer) then
      vim.notify("Phenix review " .. decision:lower() .. "ed")
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
  local lines = { "# Phenix review", "" }
  for _, file in ipairs(review.files) do
    table.insert(lines, "## " .. (file.uri or "file"))
    if file.conflict then
      table.insert(lines, "**Conflict**")
    end
    for _, hunk in ipairs(file.hunks or {}) do
      table.insert(lines, "```diff")
      vim.list_extend(lines, vim.split(hunk.unified_diff or "", "\n", { plain = true }))
      table.insert(lines, "```")
    end
    table.insert(lines, "")
  end
  local buffer = vim.api.nvim_create_buf(false, true)
  vim.bo[buffer].buftype = "nofile"
  vim.bo[buffer].filetype = "diff"
  vim.api.nvim_buf_set_lines(buffer, 0, -1, false, lines)
  vim.keymap.set("n", "a", function()
    decide(review, "Accept", buffer)
  end, { buffer = buffer, desc = "Accept Phenix review" })
  vim.keymap.set("n", "r", function()
    decide(review, "Reject", buffer)
  end, { buffer = buffer, desc = "Reject Phenix review" })
  vim.cmd("tabnew")
  vim.api.nvim_win_set_buf(0, buffer)
  return buffer
end

return M
