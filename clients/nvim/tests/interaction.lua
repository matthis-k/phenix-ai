local interaction = require("phenix_nvim.interaction")

local original_input = vim.ui.input
local original_select = vim.ui.select
local original_notify = vim.notify

local inputs = {}
local selections = {}
local notices = {}

vim.ui.input = function(_options, callback)
  assert(#inputs > 0, "unexpected elicitation input prompt")
  local value = table.remove(inputs, 1)
  callback(value)
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

local unit = { type = "unit" }
local schema = {
  type = "table",
  value = {
    count = { type = "u64" },
    flags = { type = "list", value = { type = "bool" } },
    mode = {
      type = "variant",
      value = {
        Fast = unit,
        Safe = unit,
      },
    },
    note = { type = "option", value = { type = "string" } },
  },
}

inputs = { "-1", "7", "true, false", "" }
selections = { "Fast" }
local accepted
local accepted_error
interaction.elicitation({ message = "Configure", schema = schema }, function(response, error)
  accepted = response
  accepted_error = error
end)
assert(accepted_error == nil)
assert(accepted.kind == "Accepted")
assert(accepted.value.count == 7)
assert(vim.deep_equal(accepted.value.flags, { true, false }))
assert(accepted.value.mode.kind == "Fast")
assert(accepted.value.note == nil)
assert(#notices == 1 and notices[1]:find("unsigned integer", 1, true))

local supported, support_error = interaction.supports({
  type = "map",
  value = { type = "string" },
})
assert(not supported)
assert(support_error:find("unsupported_schema", 1, true))

local unsupported_response
local unsupported_error
interaction.elicitation({
  message = "Unsupported",
  schema = { type = "callable" },
}, function(response, error)
  unsupported_response = response
  unsupported_error = error
end)
assert(unsupported_response == nil)
assert(unsupported_error:find("unsupported_schema", 1, true))

selections = { false }
local cancelled
vim.ui.select = function(_items, _options, callback)
  callback(nil)
end
interaction.elicitation({
  message = "Confirm",
  schema = { type = "bool" },
}, function(response, error)
  assert(error == nil)
  cancelled = response
end)
assert(cancelled.kind == "Cancelled")

local permission_cancelled
interaction.permission({ description = "write file" }, function(response)
  permission_cancelled = response
end)
assert(permission_cancelled.kind == "Cancelled")

vim.ui.input = original_input
vim.ui.select = original_select
vim.notify = original_notify
