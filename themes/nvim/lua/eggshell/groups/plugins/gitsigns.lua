-- gitsigns.nvim. Signs take the state colours; line and word highlights take
-- their tints, word over line marked by the colour on the tint.

return function(c)
  return {
    GitSignsAdd = { fg = c.green, bg = c.editor },
    GitSignsChange = { fg = c.blue, bg = c.editor },
    GitSignsDelete = { fg = c.red, bg = c.editor },
    GitSignsChangedelete = { link = "GitSignsChange" },
    GitSignsTopdelete = { link = "GitSignsDelete" },
    GitSignsUntracked = { fg = c.faint, bg = c.editor },

    GitSignsAddNr = { fg = c.green },
    GitSignsChangeNr = { fg = c.blue },
    GitSignsDeleteNr = { fg = c.red },

    GitSignsAddLn = { bg = c.tint.green },
    GitSignsChangeLn = { bg = c.tint.blue },
    GitSignsAddPreview = { bg = c.tint.green },
    GitSignsDeletePreview = { bg = c.tint.red },
    GitSignsNoEOLPreview = { fg = c.faint },

    GitSignsAddInline = { fg = c.green, bg = c.tint.green, bold = true },
    GitSignsChangeInline = { fg = c.blue, bg = c.tint.blue, bold = true },
    GitSignsDeleteInline = { fg = c.red, bg = c.tint.red, bold = true },
    GitSignsAddLnInline = { fg = c.green, bg = c.tint.green, bold = true },
    GitSignsChangeLnInline = { fg = c.blue, bg = c.tint.blue, bold = true },
    GitSignsDeleteLnInline = { fg = c.red, bg = c.tint.red, bold = true },
    GitSignsDeleteVirtLn = { bg = c.tint.red },
    GitSignsDeleteVirtLnInLine = { fg = c.red, bg = c.tint.red, bold = true },
    GitSignsVirtLnum = { fg = c.faint },

    GitSignsCurrentLineBlame = { fg = c.faint, italic = true },
  }
end
