-- render-markdown.nvim. Headings differ by weight of surface, not by hue:
-- the top two levels sit on the current-row grey, the rest on raised.

return function(c)
  local hl = {
    RenderMarkdownCode = { bg = c.float },
    RenderMarkdownCodeBorder = { bg = c.float },
    RenderMarkdownCodeInline = { fg = c.secondary, bg = c.float },
    RenderMarkdownCodeInfo = { fg = c.dim },
    RenderMarkdownCodeFallback = { fg = c.fg },
    RenderMarkdownQuote = { fg = c.dim },
    RenderMarkdownInlineHighlight = { fg = c.search_fg, bg = c.search_bg },
    RenderMarkdownBullet = { fg = c.dim },
    RenderMarkdownDash = { fg = c.divider },
    RenderMarkdownSign = { link = "SignColumn" },
    RenderMarkdownMath = { link = "@markup.math" },
    RenderMarkdownIndent = { fg = c.guide },
    RenderMarkdownHtmlComment = { link = "Comment" },
    RenderMarkdownLink = { fg = c.focus },
    RenderMarkdownLinkTitle = { fg = c.focus, underline = true },
    RenderMarkdownWikiLink = { fg = c.focus },
    RenderMarkdownUnchecked = { fg = c.dim },
    RenderMarkdownChecked = { fg = c.green },
    RenderMarkdownTodo = { fg = c.yellow },
    RenderMarkdownTableHead = { fg = c.divider },
    RenderMarkdownTableRow = { fg = c.divider },
    RenderMarkdownSuccess = { fg = c.green },
    RenderMarkdownInfo = { fg = c.blue },
    RenderMarkdownHint = { fg = c.cyan },
    RenderMarkdownWarn = { fg = c.orange },
    RenderMarkdownError = { fg = c.red },
  }
  for level = 1, 6 do
    hl["RenderMarkdownH" .. level] = { fg = c.primary, bold = true }
    hl["RenderMarkdownH" .. level .. "Bg"] = { bg = level <= 2 and c.current or c.float }
  end
  return hl
end
