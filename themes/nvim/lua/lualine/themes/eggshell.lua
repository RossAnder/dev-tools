-- lualine reads this file afresh on each ColorScheme and 'background' change.
-- Normal is grey; each other mode marks only its label, in the colour of
-- what that mode does: insert the chip, visual the selection, replace red.

local c = require("eggshell").colors()

local b = { fg = c.secondary, bg = c.current }
local mid = { fg = c.dim, bg = c.bar }

local function mode(a)
  a.gui = "bold"
  return { a = a, b = b, c = mid }
end

return {
  normal = mode({ fg = c.primary, bg = c.border }),
  insert = mode({ fg = c.chip_fg, bg = c.chip_bg }),
  visual = mode({ fg = c.focus, bg = c.selection }),
  replace = mode({ fg = c.red, bg = c.tint.red }),
  command = mode({ fg = c.bg, bg = c.secondary }),
  terminal = mode({ fg = c.search_fg, bg = c.search_bg }),
  inactive = {
    a = { fg = c.faint, bg = c.bar },
    b = { fg = c.faint, bg = c.bar },
    c = { fg = c.faint, bg = c.bar },
  },
}
