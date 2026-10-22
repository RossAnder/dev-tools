-- Legacy syntax groups, `:h group-name`. Code is grey by brightness; only
-- literal values take the blue.

return function(c, o)
  local it = o.italics
  return {
    Comment = { fg = c.dim, italic = it.comments },

    Constant = { fg = c.literal },
    String = { fg = c.focus },
    Character = { fg = c.focus },
    Number = { fg = c.literal },
    Boolean = { fg = c.literal },
    Float = { fg = c.literal },

    Identifier = { fg = c.fg },
    Function = { fg = c.primary },

    Statement = { fg = c.secondary, italic = it.keywords },
    Conditional = { link = "Statement" },
    Repeat = { link = "Statement" },
    Label = { fg = c.secondary },
    Operator = { fg = c.dim },
    Keyword = { link = "Statement" },
    Exception = { link = "Statement" },

    PreProc = { fg = c.secondary },
    Include = { link = "PreProc" },
    Define = { link = "PreProc" },
    Macro = { link = "PreProc" },
    PreCondit = { link = "PreProc" },

    Type = { fg = c.primary },
    StorageClass = { fg = c.secondary, italic = it.keywords },
    Structure = { fg = c.secondary, italic = it.keywords },
    Typedef = { fg = c.secondary, italic = it.keywords },

    -- Plugins link UI text to Special, so it stays grey; escapes are literals.
    Special = { fg = c.secondary },
    SpecialChar = { fg = c.literal },
    Tag = { fg = c.primary },
    Delimiter = { fg = c.dim },
    SpecialComment = { fg = c.secondary, italic = it.comments },
    Debug = { fg = c.secondary },

    Underlined = { underline = true },
    Bold = { bold = true },
    Italic = { italic = true },
    Ignore = { fg = c.faint },
    Error = { fg = c.red },
    Todo = { fg = c.chip_fg, bg = c.chip_bg, bold = true },

    Added = { fg = c.green },
    Changed = { fg = c.blue },
    Removed = { fg = c.red },

    -- Legacy help and diff syntax, still used where no parser is installed.
    helpHyperTextJump = { fg = c.focus, underline = true },
    helpHyperTextEntry = { fg = c.primary, bold = true },
    helpHeader = { fg = c.primary, bold = true },
    helpSectionDelim = { fg = c.guide },
    helpCommand = { fg = c.secondary },
    helpExample = { fg = c.fg },
    helpOption = { fg = c.focus },
    diffAdded = { link = "Added" },
    diffChanged = { link = "Changed" },
    diffRemoved = { link = "Removed" },
    diffFile = { fg = c.primary, bold = true },
    diffLine = { fg = c.cyan },
    diffIndexLine = { fg = c.dim },
    diffOldFile = { fg = c.red },
    diffNewFile = { fg = c.green },
  }
end
