-- mason.nvim.

return function(c)
  return {
    MasonNormal = { fg = c.fg, bg = c.float },
    MasonBackdrop = { bg = c.shadow },
    MasonHeader = { fg = c.chip_fg, bg = c.chip_bg, bold = true },
    MasonHeaderSecondary = { fg = c.primary, bg = c.current, bold = true },
    MasonHeading = { fg = c.primary, bold = true },
    MasonHighlight = { fg = c.focus },
    MasonHighlightBlock = { fg = c.chip_fg, bg = c.chip_bg },
    MasonHighlightBlockBold = { fg = c.chip_fg, bg = c.chip_bg, bold = true },
    MasonHighlightSecondary = { fg = c.secondary },
    MasonHighlightBlockSecondary = { fg = c.primary, bg = c.current },
    MasonHighlightBlockBoldSecondary = { fg = c.primary, bg = c.current, bold = true },
    MasonLink = { fg = c.focus, underline = true },
    MasonMuted = { fg = c.faint },
    MasonMutedBlock = { fg = c.secondary, bg = c.current },
    MasonMutedBlockBold = { fg = c.secondary, bg = c.current, bold = true },
    MasonError = { fg = c.red },
    MasonWarning = { fg = c.orange },
  }
end
