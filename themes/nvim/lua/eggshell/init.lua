local M = {}

---@class eggshell.Options
local defaults = {
  -- Leave the editor's own background unset, so the terminal's shows through.
  -- Floats keep their raised surface.
  transparent = false,
  -- Give inactive windows the recessed surface.
  dim_inactive = false,
  -- `italics = false` turns all three off.
  italics = { comments = true, keywords = false, parameters = true },
  ---@type fun(palette: table)?  edit the variant's palette in place, or return a new one
  on_palette = nil,
  ---@type fun(hl: table<string, vim.api.keyset.highlight>, palette: table)?
  on_highlights = nil,
}

M.options = vim.deepcopy(defaults)

local modules = {
  "editor",
  "syntax",
  "treesitter",
  "lsp",
  "plugins.blink",
  "plugins.bufferline",
  "plugins.crates",
  "plugins.dap",
  "plugins.flash",
  "plugins.gitsigns",
  "plugins.grug_far",
  "plugins.lazy",
  "plugins.mason",
  "plugins.mini",
  "plugins.misc",
  "plugins.noice",
  "plugins.render_markdown",
  "plugins.snacks",
  "plugins.todo_comments",
  "plugins.trouble",
  "plugins.which_key",
}

local cache = {}
local applied ---@type string?

---@param opts? eggshell.Options
function M.setup(opts)
  opts = opts or {}
  if opts.italics == false then
    opts.italics = { comments = false, keywords = false, parameters = false }
  end
  M.options = vim.tbl_deep_extend("force", vim.deepcopy(defaults), opts)
  cache = {}
end

local function variant()
  return vim.o.background == "light" and "light" or "dark"
end

--- The palette for a variant (default: the current 'background'), after on_palette.
---@param v? "dark"|"light"
function M.palette(v)
  local p = vim.deepcopy(require("eggshell.palette")[v or variant()])
  if M.options.on_palette then
    p = M.options.on_palette(p) or p
  end
  return p
end

--- The resolved roles for a variant, as the group files see them.
---@param v? "dark"|"light"
function M.colors(v)
  v = v or variant()
  return require("eggshell.colors").roles(M.palette(v), v, M.options)
end

local function build(v)
  local p = M.palette(v)
  local c = require("eggshell.colors").roles(p, v, M.options)
  local hl = {}
  for _, name in ipairs(modules) do
    for group, spec in pairs(require("eggshell.groups." .. name)(c, M.options)) do
      hl[group] = spec
    end
  end
  if M.options.on_highlights then
    M.options.on_highlights(hl, p)
  end
  return { hl = hl, terminal = c.terminal }
end

function M.load()
  local v = variant()
  if vim.g.colors_name then
    vim.cmd("hi clear")
    if vim.fn.exists("syntax_on") == 1 then
      vim.cmd("syntax reset")
    end
  end
  vim.g.colors_name = "eggshell"

  cache[v] = cache[v] or build(v)
  local set = vim.api.nvim_set_hl
  for group, spec in pairs(cache[v].hl) do
    set(0, group, spec)
  end
  for i, colour in ipairs(cache[v].terminal) do
    vim.g["terminal_color_" .. (i - 1)] = colour
  end
  applied = v

  -- Nvim reloads the scheme itself when 'background' changes; this catches a
  -- change that arrives without that reload, and does nothing otherwise.
  vim.api.nvim_create_autocmd("OptionSet", {
    group = vim.api.nvim_create_augroup("eggshell", { clear = true }),
    pattern = "background",
    callback = function()
      if vim.g.colors_name == "eggshell" and applied ~= variant() then
        vim.cmd.colorscheme("eggshell")
      end
    end,
  })
end

return M
