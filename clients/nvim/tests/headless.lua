local native = require("phenix")
assert(native.interface_id == "phenix.application@1")

local frontend = require("phenix_nvim")
frontend.setup({ auto_connect = false })
assert(type(frontend.reference) == "function")
assert(type(frontend.reference_at) == "function")
assert(type(frontend.reference_picker) == "function")
assert(type(frontend.send) == "function")

local context = require("phenix_nvim.context")
local direct = assert(context.typed_reference("file:///tmp/reference.txt"))
local typed = assert(context.typed_reference("@file:///tmp/reference.txt"))
assert(direct.kind == "resource")
assert(vim.deep_equal(direct, typed), "picker and @ references must share one typed constructor")

local model = require("phenix_nvim.compose.model")
local buffer = require("phenix_nvim.compose.buffer")
local document = model.new()
local source_a = {
  kind = "selection",
  source = { uri = "file:///a.rs", start_line = 1, end_line = 2 },
  snapshot = "A",
}
local a = model.add(document, source_a)
source_a.snapshot = "mutated"
assert(a.snapshot == "A", "selection snapshots must be immutable copies")
local b = model.add(document, {
  kind = "selection",
  source = { uri = "file:///b.rs", start_line = 3, end_line = 4 },
  snapshot = "B",
})
local serialized = assert(buffer.serialize_text(
  buffer.marker(a) .. "question A\n" .. buffer.marker(b) .. "question B",
  document
))
assert(#serialized == 4)
assert(serialized[1].snapshot == "A")
assert(serialized[2].text == "question A\n")
assert(serialized[3].snapshot == "B")
assert(serialized[4].text == "question B")

local transcript = require("phenix_nvim.transcript.model")
local projection = transcript.new()
assert(transcript.apply(projection, {
  sequence = 1,
  kind = "message",
  id = "assistant-1",
  role = "assistant",
  text = "hello",
}))
assert(transcript.apply(projection, {
  sequence = 2,
  kind = "text_delta",
  id = "assistant-1",
  text = " world",
}))
assert(projection.nodes["assistant-1"].text == "hello world")
assert(transcript.apply(projection, {
  sequence = 3,
  kind = "tool_call",
  call_id = "call-1",
  callable_id = "tools.read",
  input = "README.md",
}))
assert(transcript.apply(projection, {
  sequence = 4,
  kind = "tool_result",
  call_id = "call-1",
  output = "done",
}))
assert(projection.nodes["call-1"].state == "completed")
local _, gap = transcript.apply(projection, {
  sequence = 6,
  kind = "progress",
  id = "progress-1",
  message = "bad gap",
})
assert(gap ~= nil, "version gaps must fail instead of being guessed")

local sidebar = require("phenix_nvim.sidebar")
sidebar.open()
local transcript_buffer, compose_buffer = sidebar.buffers()
assert(transcript_buffer ~= compose_buffer, "transcript and compose must use separate buffers")

local transcript_view = require("phenix_nvim.transcript.buffer")
local transcript_win = vim.fn.bufwinid(transcript_buffer)
assert(transcript_win > 0, "transcript buffer must be visible")
vim.bo[transcript_buffer].modifiable = true
local lines = {}
for index = 1, 200 do
  lines[index] = "line " .. index
end
vim.api.nvim_buf_set_lines(transcript_buffer, 0, -1, false, lines)
vim.bo[transcript_buffer].modifiable = false
vim.api.nvim_win_set_cursor(transcript_win, { 1, 0 })
vim.api.nvim_win_call(transcript_win, function()
  vim.cmd("normal! zt")
end)
vim.api.nvim_exec_autocmds("WinScrolled", { pattern = tostring(transcript_win) })
assert(not transcript_view.is_following_tail(), "manual scrolling away from the end must disable follow-tail")
vim.api.nvim_win_set_cursor(transcript_win, { 200, 0 })
vim.api.nvim_win_call(transcript_win, function()
  vim.cmd("normal! zb")
end)
vim.api.nvim_exec_autocmds("WinScrolled", { pattern = tostring(transcript_win) })
assert(transcript_view.is_following_tail(), "returning to the end must re-enable follow-tail")
sidebar.close()
