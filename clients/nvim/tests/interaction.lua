local interaction = require("phenix_nvim.interaction")

local original_input = vim.ui.input
local original_select = vim.ui.select
local original_notify = vim.notify

local inputs = {}
local selections = {}
local notices = {}

vim.ui.input = function(_options, callback)
  assert(#inputs > 0, "unexpected elicitation input prompt")
  callback(table.remove(inputs, 1))
end

vim.ui.select = function(items, _options, callback)
  assert(#selections > 0, "unexpected elicitation select prompt")
  local value = table.remove(selections, 1)
  if value ~= nil then
    assert(vim.tbl_contains(items, value), "mock selected a value not offered by the form")
  end
  callback(value)
end

vim.notify = function(message)
  table.insert(notices, message)
end

local form = {
  kind = "object",
  fields = {
    { name = "count", schema = { kind = "integer", signed = false } },
    { name = "flags", schema = { kind = "list", item = { kind = "boolean" } } },
    { name = "mode", schema = { kind = "enum", options = { "Fast", "Safe" } } },
    { name = "note", schema = { kind = "string", optional = true } },
  },
}

inputs = { "-1", "7", "true, false", "" }
selections = { "Fast" }
local accepted
local cancelled = false
local reply = {
  accept = function(_, value)
    accepted = value
  end,
  cancel = function()
    cancelled = true
  end,
}
interaction.elicitation({ message = "Configure", form = form }, reply)
assert(not cancelled)
assert(accepted.count == 7)
assert(vim.deep_equal(accepted.flags, { true, false }))
assert(accepted.mode.kind == "Fast")
assert(accepted.note == nil)
assert(#notices == 1 and notices[1]:find("unsigned integer", 1, true))

vim.ui.select = function(_items, _options, callback)
  callback(nil)
end
local elicitation_cancelled = false
interaction.elicitation({
  message = "Confirm",
  form = { kind = "boolean" },
}, {
  accept = function()
    error("cancelled form must not be accepted")
  end,
  cancel = function()
    elicitation_cancelled = true
  end,
})
assert(elicitation_cancelled)

local permission_cancelled = false
interaction.permission({ description = "write file" }, {
  allow_once = function()
    error("cancelled permission must not be allowed")
  end,
  deny = function()
    error("cancelled permission must not be denied")
  end,
  cancel = function()
    permission_cancelled = true
  end,
})
assert(permission_cancelled)

vim.ui.input = original_input
vim.ui.select = original_select
vim.notify = original_notify
