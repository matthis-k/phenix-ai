if vim.g.loaded_phenix_nvim == 1 then
  return
end
vim.g.loaded_phenix_nvim = 1

local function action(name)
  return function()
    require("phenix_nvim").actions[name]()
  end
end

vim.api.nvim_create_user_command("PhenixToggle", action("toggle"), {})
vim.api.nvim_create_user_command("PhenixReference", action("reference"), { range = true })
vim.api.nvim_create_user_command("PhenixReferencePick", action("reference_picker"), {})
vim.api.nvim_create_user_command("PhenixReferenceAt", function(options)
  require("phenix_nvim").reference_at(options.args)
end, { nargs = 1, complete = "file" })
vim.api.nvim_create_user_command("PhenixSend", action("send"), {})
vim.api.nvim_create_user_command("PhenixCancel", action("cancel"), {})
vim.api.nvim_create_user_command("PhenixNew", action("new_session"), {})
vim.api.nvim_create_user_command("PhenixSessions", action("choose_session"), {})
vim.api.nvim_create_user_command("PhenixImage", function(options)
  require("phenix_nvim").attach_image(options.args ~= "" and options.args or nil)
end, { nargs = "?", complete = "file" })
