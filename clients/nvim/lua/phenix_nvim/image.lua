local M = {}

local mime_types = {
  png = "image/png",
  jpg = "image/jpeg",
  jpeg = "image/jpeg",
  gif = "image/gif",
  webp = "image/webp",
}

function M.from_file(path)
  local handle, error = io.open(path, "rb")
  if handle == nil then
    return nil, error
  end
  local bytes = handle:read("*a")
  handle:close()
  local extension = path:match("%.([^.]+)$")
  local mime_type = extension and mime_types[extension:lower()] or nil
  if mime_type == nil then
    return nil, "unsupported image type: " .. path
  end
  return {
    kind = "image",
    name = vim.fn.fnamemodify(path, ":t"),
    path = vim.fn.fnamemodify(path, ":p"),
    mime_type = mime_type,
    bytes = bytes,
  }
end

local function backend()
  local value = vim.ui and vim.ui.img or nil
  if type(value) ~= "table" or type(value.set) ~= "function" or type(value.del) ~= "function" then
    return nil
  end
  return value
end

function M.available()
  return backend() ~= nil
end

function M.preview(image, placement)
  local renderer = backend()
  if renderer == nil then
    return nil
  end
  local ok, id = pcall(renderer.set, image.bytes, placement or {})
  if ok and type(id) == "number" then
    return id
  end
  return nil
end

function M.update(id, placement)
  local renderer = backend()
  if renderer == nil or id == nil then
    return false
  end
  local ok = pcall(renderer.set, id, placement or {})
  return ok
end

function M.close(id)
  local renderer = backend()
  if renderer == nil or id == nil then
    return false
  end
  local ok, found = pcall(renderer.del, id)
  return ok and found ~= false
end

function M.fallback(image)
  return string.format("[image: %s]", image.name)
end

return M