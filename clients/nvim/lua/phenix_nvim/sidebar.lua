local config = require("phenix_nvim.config")
local state = require("phenix_nvim.state")
local compose = require("phenix_nvim.compose.buffer")
local transcript = require("phenix_nvim.transcript.buffer")

local M = {}
local transcript_win
local compose_win

local function valid(win)
  return win ~= nil and vim.api.nvim_win_is_valid(win)
end

function M.is_open()
  return valid(transcript_win) and valid(compose_win)
end

function M.remember_cursor()
  if valid(compose_win) then
    state.remembered_compose_cursor = vim.api.nvim_win_get_cursor(compose_win)
  end
end

function M.open()
  if M.is_open() then
    return compose_win
  end
  local options = config.get()
  vim.cmd(options.side == "left" and "topleft vsplit" or "botright vsplit")
  transcript_win = vim.api.nvim_get_current_win()
  vim.api.nvim_win_set_width(transcript_win, options.width)
  vim.api.nvim_win_set_buf(transcript_win, transcript.ensure())
  transcript.attach_window(transcript_win)

  vim.cmd("belowright split")
  compose_win = vim.api.nvim_get_current_win()
  vim.api.nvim_win_set_height(compose_win, options.compose_height)
  vim.api.nvim_win_set_buf(compose_win, compose.ensure(state.compose))
  pcall(vim.api.nvim_win_set_cursor, compose_win, state.remembered_compose_cursor)

  vim.api.nvim_create_autocmd("WinLeave", {
    callback = function(args)
      if args.match == tostring(compose_win) or vim.api.nvim_get_current_win() == compose_win then
        M.remember_cursor()
      end
    end,
    once = true,
  })
  return compose_win
end

function M.close()
  M.remember_cursor()
  if valid(transcript_win) then
    transcript.detach_window(transcript_win)
  end
  for _, win in ipairs({ compose_win, transcript_win }) do
    if valid(win) then
      pcall(vim.api.nvim_win_close, win, true)
    end
  end
  compose_win = nil
  transcript_win = nil
end

function M.toggle()
  if M.is_open() then
    M.close()
  else
    M.open()
  end
end

function M.focus_compose()
  local win = M.open()
  vim.api.nvim_set_current_win(win)
  pcall(vim.api.nvim_win_set_cursor, win, state.remembered_compose_cursor)
  return win
end

function M.buffers()
  return transcript.ensure(), compose.ensure(state.compose)
end

return M
