local M = {}

local function notify_error(message)
  if vim.notify ~= nil then
    vim.notify(message, vim.log.levels.ERROR)
  end
end

local function schema_kind(schema)
  if type(schema) ~= "table" or type(schema.type) ~= "string" then
    return nil, nil, "schema is missing string field type"
  end
  return schema.type, schema.value, nil
end

local function sorted_keys(values)
  local keys = {}
  for key in pairs(values or {}) do
    table.insert(keys, key)
  end
  table.sort(keys)
  return keys
end

local function is_scalar(schema)
  local kind = schema_kind(schema)
  return kind == "string" or kind == "bool" or kind == "i64" or kind == "u64" or kind == "f64"
end

local function unit_variants(schema)
  local kind, variants = schema_kind(schema)
  if kind ~= "variant" or type(variants) ~= "table" then
    return nil
  end
  local names = sorted_keys(variants)
  if #names == 0 then
    return nil
  end
  for _, name in ipairs(names) do
    local variant_kind = schema_kind(variants[name])
    if variant_kind ~= "unit" then
      return nil
    end
  end
  return names
end

local function validate_schema(schema)
  local kind, value, error = schema_kind(schema)
  if error ~= nil then
    return nil, error
  end
  if kind == "string" or kind == "bool" or kind == "i64" or kind == "u64" or kind == "f64" then
    return true
  end
  if kind == "option" then
    if is_scalar(value) then
      return true
    end
    return nil, "optional elicitation values require a scalar item schema"
  end
  if kind == "table" then
    if type(value) ~= "table" then
      return nil, "table schema is missing fields"
    end
    for _, key in ipairs(sorted_keys(value)) do
      local ok, field_error = validate_schema(value[key])
      if not ok then
        return nil, key .. ": " .. field_error
      end
    end
    return true
  end
  if kind == "variant" then
    if unit_variants(schema) ~= nil then
      return true
    end
    return nil, "elicitation variants must contain unit variants only"
  end
  if kind == "list" then
    if is_scalar(value) or unit_variants(value) ~= nil then
      return true
    end
    return nil, "elicitation lists require a supported scalar or unit-variant item schema"
  end
  return nil, "unsupported elicitation schema type " .. kind
end

local function finish_once(callback)
  local settled = false
  return function(...)
    if settled then
      return
    end
    settled = true
    callback(...)
  end
end

local function fallback_select(items, options, callback)
  local done = finish_once(callback)
  local prompt = options.prompt or "Select"
  local lines = { prompt }
  local width = #prompt
  for index, item in ipairs(items) do
    local line = string.format("%d. %s", index, tostring(item))
    table.insert(lines, line)
    width = math.max(width, #line)
  end
  width = math.max(20, math.min(width + 2, math.max(20, vim.o.columns - 4)))
  local height = #lines
  local row = math.max(0, math.floor((vim.o.lines - height) / 2) - 1)
  local col = math.max(0, math.floor((vim.o.columns - width) / 2))
  local buffer = vim.api.nvim_create_buf(false, true)
  vim.bo[buffer].buftype = "nofile"
  vim.bo[buffer].bufhidden = "wipe"
  vim.bo[buffer].swapfile = false
  vim.api.nvim_buf_set_lines(buffer, 0, -1, false, lines)
  vim.bo[buffer].modifiable = false
  local win = vim.api.nvim_open_win(buffer, true, {
    relative = "editor",
    style = "minimal",
    border = "single",
    width = width,
    height = height,
    row = row,
    col = col,
  })
  if #items > 0 then
    vim.api.nvim_win_set_cursor(win, { 2, 0 })
  end

  local function choose(index)
    local choice = items[index]
    done(choice, choice ~= nil and index or nil)
    if vim.api.nvim_win_is_valid(win) then
      pcall(vim.api.nvim_win_close, win, true)
    end
  end

  vim.keymap.set("n", "<CR>", function()
    local line = vim.api.nvim_win_get_cursor(win)[1]
    choose(line - 1)
  end, { buffer = buffer, nowait = true, silent = true })
  vim.keymap.set("n", "<Esc>", function()
    choose(0)
  end, { buffer = buffer, nowait = true, silent = true })
  vim.keymap.set("n", "q", function()
    choose(0)
  end, { buffer = buffer, nowait = true, silent = true })
  for index = 1, math.min(#items, 9) do
    vim.keymap.set("n", tostring(index), function()
      choose(index)
    end, { buffer = buffer, nowait = true, silent = true })
  end
  vim.api.nvim_create_autocmd("WinClosed", {
    pattern = tostring(win),
    once = true,
    callback = function()
      done(nil, nil)
    end,
  })
end

local function select(items, options, callback)
  if vim.ui ~= nil and type(vim.ui.select) == "function" then
    vim.ui.select(items, options, callback)
    return
  end
  fallback_select(items, options, callback)
end

local function input(options, callback)
  if vim.ui ~= nil and type(vim.ui.input) == "function" then
    vim.ui.input(options, callback)
    return
  end
  vim.schedule(function()
    local ok, value = pcall(vim.fn.input, options.prompt or "")
    callback(ok and value or nil)
  end)
end

local function parse_integer(text, unsigned)
  local value = tonumber(text)
  if value == nil or value ~= math.floor(value) then
    return nil, unsigned and "expected unsigned integer" or "expected integer"
  end
  if unsigned and value < 0 then
    return nil, "expected unsigned integer"
  end
  return value
end

local function parse_scalar(schema, text)
  local kind = schema_kind(schema)
  if kind == "string" then
    return text
  end
  if kind == "i64" then
    return parse_integer(text, false)
  end
  if kind == "u64" then
    return parse_integer(text, true)
  end
  if kind == "f64" then
    local value = tonumber(text)
    if value == nil or value ~= value or value == math.huge or value == -math.huge then
      return nil, "expected finite number"
    end
    return value
  end
  if kind == "bool" then
    if text == "true" then
      return true
    end
    if text == "false" then
      return false
    end
    return nil, "expected true or false"
  end
  return nil, "unsupported scalar schema"
end

local function split_list(text)
  local result = {}
  if text:match("^%s*$") then
    return result
  end
  for item in text:gmatch("[^,]+") do
    table.insert(result, vim.trim(item))
  end
  return result
end

local ask

local function retry_input(schema, label, callback)
  input({ prompt = label .. ": " }, function(text)
    if text == nil then
      callback(nil, true)
      return
    end
    local value, error = parse_scalar(schema, text)
    if error ~= nil then
      notify_error(label .. ": " .. error)
      retry_input(schema, label, callback)
      return
    end
    callback(value, false)
  end)
end

local function ask_optional(schema, label, callback)
  local item = schema.value
  local kind = schema_kind(item)
  if kind == "bool" then
    select({ "None", "True", "False" }, { prompt = label }, function(choice)
      if choice == nil then
        callback(nil, true)
      elseif choice == "None" then
        callback(nil, false)
      else
        callback(choice == "True", false)
      end
    end)
    return
  end
  input({ prompt = label .. " (blank = none): " }, function(text)
    if text == nil then
      callback(nil, true)
      return
    end
    if text == "" then
      callback(nil, false)
      return
    end
    local value, error = parse_scalar(item, text)
    if error ~= nil then
      notify_error(label .. ": " .. error)
      ask_optional(schema, label, callback)
      return
    end
    callback(value, false)
  end)
end

local function ask_table(fields, label, callback)
  local keys = sorted_keys(fields)
  local result = {}
  local index = 1
  local function next_field()
    local key = keys[index]
    if key == nil then
      callback(result, false)
      return
    end
    local field_label = label == "" and key or (label .. "." .. key)
    ask(fields[key], field_label, function(value, cancelled, error)
      if error ~= nil or cancelled then
        callback(nil, cancelled, error)
        return
      end
      if value ~= nil then
        result[key] = value
      end
      index = index + 1
      next_field()
    end)
  end
  next_field()
end

local function ask_variant(schema, label, callback)
  local variants = unit_variants(schema)
  if variants == nil then
    callback(nil, false, "unsupported_schema: elicitation variants must contain unit variants only")
    return
  end
  select(variants, { prompt = label }, function(choice)
    if choice == nil then
      callback(nil, true)
      return
    end
    callback({ kind = choice }, false)
  end)
end

local function ask_list(item, label, callback)
  local variants = unit_variants(item)
  input({ prompt = label .. " (comma-separated): " }, function(text)
    if text == nil then
      callback(nil, true)
      return
    end
    local result = {}
    for _, token in ipairs(split_list(text)) do
      if variants ~= nil then
        local allowed = false
        for _, variant in ipairs(variants) do
          if token == variant then
            allowed = true
            break
          end
        end
        if not allowed then
          notify_error(label .. ": unknown value " .. token)
          ask_list(item, label, callback)
          return
        end
        table.insert(result, { kind = token })
      else
        local value, error = parse_scalar(item, token)
        if error ~= nil then
          notify_error(label .. ": " .. error)
          ask_list(item, label, callback)
          return
        end
        table.insert(result, value)
      end
    end
    callback(result, false)
  end)
end

ask = function(schema, label, callback)
  local kind, value, error = schema_kind(schema)
  if error ~= nil then
    callback(nil, false, "unsupported_schema: " .. error)
    return
  end
  if kind == "string" or kind == "i64" or kind == "u64" or kind == "f64" then
    retry_input(schema, label, callback)
  elseif kind == "bool" then
    select({ "True", "False" }, { prompt = label }, function(choice)
      if choice == nil then
        callback(nil, true)
      else
        callback(choice == "True", false)
      end
    end)
  elseif kind == "option" then
    ask_optional(schema, label, callback)
  elseif kind == "table" then
    ask_table(value, label, callback)
  elseif kind == "variant" then
    ask_variant(schema, label, callback)
  elseif kind == "list" then
    ask_list(value, label, callback)
  else
    callback(nil, false, "unsupported_schema: unsupported elicitation schema type " .. kind)
  end
end

function M.supports(schema)
  local ok, error = validate_schema(schema)
  if ok then
    return true
  end
  return false, "unsupported_schema: " .. error
end

function M.permission(request, done)
  select({ "Allow once", "Deny" }, {
    prompt = "Phenix permission: " .. (request.description or "Allow this action?"),
  }, function(choice)
    if choice == "Allow once" then
      done({ kind = "AllowOnce" })
    elseif choice == "Deny" then
      done({ kind = "Deny" })
    else
      done({ kind = "Cancelled" })
    end
  end)
end

function M.elicitation(request, done)
  local supported, error = M.supports(request.schema)
  if not supported then
    done(nil, error)
    return
  end
  ask(request.schema, request.message or "Phenix input", function(value, cancelled, prompt_error)
    if prompt_error ~= nil then
      done(nil, prompt_error)
    elseif cancelled then
      done({ kind = "Cancelled" })
    else
      done({ kind = "Accepted", value = value })
    end
  end)
end

return M
