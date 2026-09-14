local image = require("phenix_nvim.image")

local original = vim.ui.img
local calls = {}
vim.ui.img = {
  set = function(value, opts)
    table.insert(calls, { kind = "set", value = value, opts = vim.deepcopy(opts) })
    if type(value) == "number" then
      return value
    end
    return 41
  end,
  get = function(_id)
    return nil
  end,
  del = function(id)
    table.insert(calls, { kind = "del", id = id })
    return true
  end,
}

local attachment = {
  kind = "image",
  name = "snapshot.png",
  path = "/path/is/not/read/again.png",
  mime_type = "image/png",
  bytes = "immutable-snapshot",
}

assert(image.available())
local id = assert(image.preview(attachment, { row = 2, col = 3 }))
assert(id == 41)
assert(calls[1].kind == "set")
assert(calls[1].value == attachment.bytes, "preview must use immutable attachment bytes")
assert(calls[1].opts.row == 2 and calls[1].opts.col == 3)
assert(image.update(id, { row = 5, col = 7 }))
assert(calls[2].kind == "set" and calls[2].value == id)
assert(image.close(id))
assert(calls[3].kind == "del" and calls[3].id == id)
assert(attachment.bytes == "immutable-snapshot", "renderer lifecycle must not mutate attachment bytes")

vim.ui.img = original