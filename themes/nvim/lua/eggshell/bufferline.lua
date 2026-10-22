-- bufferline.nvim derives its icon groups from its own `highlights` option,
-- not from the BufferLine* groups, so the strip only matches when that option
-- comes from here. Pass `highlights` as bufferline's option; it is re-read on
-- every ColorScheme and yields nothing under another scheme.

local M = {}

--- The strip in bufferline's option keys.
---@param c table eggshell roles
function M.theme(c)
  local strip, sel = c.strip, c.editor
  -- Fresh tables per key: bufferline writes each entry's group name into it.
  local function base()
    return { fg = c.secondary, bg = strip }
  end
  local function visible()
    return { fg = c.fg, bg = strip }
  end
  local function selected()
    return { fg = c.primary, bg = sel, bold = true, italic = false }
  end
  local function state(fg)
    return { fg = fg, bg = strip }
  end
  local function state_sel(fg)
    return { fg = fg, bg = sel, bold = true, italic = false }
  end

  local t = {
    fill = { fg = c.dim, bg = strip },
    background = base(),
    buffer = base(),
    buffer_visible = visible(),
    buffer_selected = selected(),
    duplicate = { fg = c.faint, bg = strip, italic = true },
    duplicate_visible = { fg = c.dim, bg = strip, italic = true },
    duplicate_selected = { fg = c.secondary, bg = sel, italic = true },
    numbers = { fg = c.faint, bg = strip },
    numbers_visible = { fg = c.faint, bg = strip },
    numbers_selected = { fg = c.secondary, bg = sel, bold = true, italic = false },
    close_button = { fg = c.faint, bg = strip },
    close_button_visible = { fg = c.faint, bg = strip },
    close_button_selected = { fg = c.secondary, bg = sel },
    modified = state(c.orange),
    modified_visible = state(c.orange),
    modified_selected = { fg = c.orange, bg = sel },
    separator = { fg = c.divider, bg = strip },
    separator_visible = { fg = c.divider, bg = strip },
    separator_selected = { fg = c.divider, bg = sel },
    offset_separator = { fg = c.divider, bg = strip },
    indicator_visible = { fg = c.divider, bg = strip },
    indicator_selected = { fg = c.focus, bg = sel },
    tab = { fg = c.secondary, bg = strip },
    tab_selected = { fg = c.primary, bg = sel, bold = true },
    tab_close = { fg = c.faint, bg = strip },
    tab_separator = { fg = c.divider, bg = strip },
    tab_separator_selected = { fg = c.divider, bg = sel },
    pick = { fg = c.chip_fg, bg = c.chip_bg, bold = true, italic = false },
    pick_visible = { fg = c.chip_fg, bg = c.chip_bg, bold = true, italic = false },
    pick_selected = { fg = c.chip_fg, bg = c.chip_bg, bold = true, italic = false },
    trunc_marker = { fg = c.faint, bg = strip },
    group_separator = { fg = c.dim, bg = strip },
    group_label = { fg = c.bg, bg = c.dim },
    diagnostic = { fg = c.faint, bg = strip },
    diagnostic_visible = { fg = c.faint, bg = strip },
    diagnostic_selected = { fg = c.secondary, bg = sel, bold = true, italic = false },
  }

  -- Buffer names stay grey whatever their diagnostics; only the count takes
  -- the state's colour.
  for level, fg in pairs({ hint = c.cyan, info = c.blue, warning = c.orange, error = c.red }) do
    t[level] = base()
    t[level .. "_visible"] = visible()
    t[level .. "_selected"] = selected()
    t[level .. "_diagnostic"] = state(fg)
    t[level .. "_diagnostic_visible"] = state(fg)
    t[level .. "_diagnostic_selected"] = state_sel(fg)
  end
  return t
end

--- bufferline's `highlights` option.
function M.highlights()
  if vim.g.colors_name ~= "eggshell" then
    return {}
  end
  return M.theme(require("eggshell").colors())
end

return M
