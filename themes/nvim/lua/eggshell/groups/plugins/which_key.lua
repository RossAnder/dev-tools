-- which-key.nvim. Icons follow mini.icons' muted tiers.

return function(c)
  local hl = {
    WhichKey = { fg = c.primary, bold = true },
    WhichKeyGroup = { fg = c.secondary },
    WhichKeyDesc = { fg = c.fg },
    WhichKeySeparator = { fg = c.faint },
    WhichKeyValue = { fg = c.dim },
    WhichKeyNormal = { fg = c.fg, bg = c.float },
    WhichKeyBorder = { fg = c.border, bg = c.float },
    WhichKeyTitle = { link = "FloatTitle" },
    WhichKeyIcon = { fg = c.dim },
  }
  for _, name in ipairs({ "Azure", "Blue", "Cyan", "Green", "Grey", "Orange", "Purple", "Red", "Yellow" }) do
    hl["WhichKeyIcon" .. name] = { link = "MiniIcons" .. name }
  end
  return hl
end
