local M = {}

function M.open(review)
  if type(review) ~= "table" or type(review.files) ~= "table" then
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
      vim.list_extend(lines, vim.split(hunk.text or "", "\n", { plain = true }))
      table.insert(lines, "```")
    end
    table.insert(lines, "")
  end
  local buffer = vim.api.nvim_create_buf(false, true)
  vim.bo[buffer].buftype = "nofile"
  vim.bo[buffer].filetype = "markdown"
  vim.api.nvim_buf_set_lines(buffer, 0, -1, false, lines)
  vim.cmd("tabnew")
  vim.api.nvim_win_set_buf(0, buffer)
  return buffer
end

return M
