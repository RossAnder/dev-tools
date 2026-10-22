-- flash.nvim. The label is the one thing to read, so it is the chip.

return function(c)
  return {
    FlashBackdrop = { fg = c.faint },
    FlashMatch = { fg = c.search_fg, bg = c.search_bg },
    FlashCurrent = { fg = c.search_fg, bg = c.search_bg, bold = true, underline = true },
    FlashLabel = { fg = c.chip_fg, bg = c.chip_bg, bold = true },
    FlashPrompt = { fg = c.fg, bg = c.float },
    FlashPromptIcon = { fg = c.focus, bg = c.float },
    FlashCursor = { fg = c.bg, bg = c.primary },
  }
end
