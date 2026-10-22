-- :checkhealth, and small plugins LazyVim may pull in.

return function(c)
  return {
    healthSectionDelim = { fg = c.primary, bg = c.current, bold = true },
    healthHeadingChar = { fg = c.guide },

    TreesitterContext = { bg = c.float },
    TreesitterContextLineNumber = { fg = c.faint, bg = c.float },
    TreesitterContextBottom = { underline = true, sp = c.divider },
  }
end
