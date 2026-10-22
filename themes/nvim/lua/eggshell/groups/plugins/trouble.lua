-- trouble.nvim.

return function(c)
  return {
    TroubleNormal = { fg = c.fg, bg = c.float },
    TroubleNormalNC = { fg = c.fg, bg = c.float },
    TroubleText = { fg = c.fg },
    TroublePreview = { bg = c.reference },
    TroubleFilename = { fg = c.primary, bold = true },
    TroubleBasename = { fg = c.primary, bold = true },
    TroubleDirectory = { fg = c.dim },
    TroubleIconDirectory = { fg = c.dim },
    TroubleSource = { fg = c.faint },
    TroubleCode = { fg = c.faint },
    TroublePos = { fg = c.faint },
    TroubleCount = { fg = c.secondary, bg = c.current },
    TroubleIndent = { fg = c.guide },
    TroubleIndentFoldClosed = { fg = c.dim },
    TroubleIndentFoldOpen = { fg = c.guide },
  }
end
