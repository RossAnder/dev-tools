-- blink.cmp. Kinds are grey by role; only value kinds take the literal blue.

return function(c)
  local hl = {
    BlinkCmpMenu = { fg = c.fg, bg = c.float },
    BlinkCmpMenuBorder = { fg = c.border, bg = c.float },
    BlinkCmpMenuSelection = { bg = c.current, bold = true },
    BlinkCmpScrollBarThumb = { bg = c.guide },
    BlinkCmpScrollBarGutter = { bg = c.float },
    BlinkCmpLabel = { fg = c.fg },
    BlinkCmpLabelDeprecated = { fg = c.faint, strikethrough = true },
    BlinkCmpLabelMatch = { fg = c.focus, bold = true },
    BlinkCmpLabelDetail = { fg = c.dim },
    BlinkCmpLabelDescription = { fg = c.dim },
    BlinkCmpSource = { fg = c.faint },
    BlinkCmpGhostText = { fg = c.faint },
    BlinkCmpDoc = { fg = c.fg, bg = c.float },
    BlinkCmpDocBorder = { fg = c.border, bg = c.float },
    BlinkCmpDocSeparator = { fg = c.divider, bg = c.float },
    BlinkCmpDocCursorLine = { bg = c.current },
    BlinkCmpSignatureHelp = { fg = c.fg, bg = c.float },
    BlinkCmpSignatureHelpBorder = { fg = c.border, bg = c.float },
    BlinkCmpSignatureHelpActiveParameter = { link = "LspSignatureActiveParameter" },
    BlinkCmpKind = { fg = c.dim },
  }

  local kinds = {
    secondary = {
      "Class", "Constructor", "Enum", "Event", "Field", "Function", "Interface",
      "Keyword", "Method", "Module", "Operator", "Property", "Struct",
      "TypeParameter", "Variable",
    },
    dim = { "File", "Folder", "Reference", "Snippet", "Text" },
    literal = { "Color", "Constant", "EnumMember", "Unit", "Value" },
  }
  for role, names in pairs(kinds) do
    for _, kind in ipairs(names) do
      hl["BlinkCmpKind" .. kind] = { fg = c[role] }
    end
  end
  return hl
end
