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

function M.preview(image, placement)
  if vim.ui ~= nil and type(vim.ui.img) == "function" then
    local ok, handle = pcall(vim.ui.img, image.path, placement or {})
    if ok then
      return handle
    end
  end
  return nil
end

function M.fallback(image)
  return string.format("[image: %s]", image.name)
end

return M
