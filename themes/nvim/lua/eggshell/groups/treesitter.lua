-- Treesitter captures, `:h treesitter-highlight-groups`.

return function(c, o)
  local it = o.italics
  return {
    ["@variable"] = { fg = c.fg },
    ["@variable.builtin"] = { fg = c.secondary, italic = true },
    ["@variable.parameter"] = { fg = c.fg, italic = it.parameters },
    ["@variable.parameter.builtin"] = { fg = c.secondary, italic = true },
    ["@variable.member"] = { fg = c.fg },

    ["@constant"] = { fg = c.literal },
    ["@constant.builtin"] = { fg = c.literal },
    ["@constant.macro"] = { fg = c.literal },

    ["@module"] = { fg = c.secondary },
    ["@module.builtin"] = { fg = c.secondary, italic = true },
    ["@label"] = { fg = c.secondary },

    ["@string"] = { fg = c.focus },
    ["@string.documentation"] = { fg = c.secondary, italic = it.comments },
    ["@string.regexp"] = { fg = c.literal },
    ["@string.escape"] = { fg = c.literal },
    ["@string.special"] = { fg = c.literal },
    ["@string.special.symbol"] = { fg = c.literal },
    ["@string.special.path"] = { fg = c.focus },
    ["@string.special.url"] = { fg = c.focus, underline = true },

    ["@character"] = { fg = c.focus },
    ["@character.special"] = { fg = c.literal },

    ["@boolean"] = { fg = c.literal },
    ["@number"] = { fg = c.literal },
    ["@number.float"] = { fg = c.literal },

    ["@type"] = { fg = c.primary },
    ["@type.builtin"] = { fg = c.secondary, italic = true },
    ["@type.definition"] = { fg = c.primary },

    ["@attribute"] = { fg = c.secondary },
    ["@attribute.builtin"] = { fg = c.secondary, italic = true },
    ["@property"] = { fg = c.fg },

    ["@function"] = { fg = c.primary },
    ["@function.builtin"] = { fg = c.secondary, italic = true },
    ["@function.call"] = { fg = c.primary },
    ["@function.macro"] = { fg = c.secondary },
    ["@function.method"] = { fg = c.primary },
    ["@function.method.call"] = { fg = c.primary },
    ["@constructor"] = { fg = c.primary },
    ["@operator"] = { fg = c.dim },

    ["@keyword"] = { fg = c.secondary, italic = it.keywords },
    ["@keyword.coroutine"] = { link = "@keyword" },
    ["@keyword.function"] = { link = "@keyword" },
    ["@keyword.operator"] = { link = "@keyword" },
    ["@keyword.import"] = { link = "@keyword" },
    ["@keyword.type"] = { link = "@keyword" },
    ["@keyword.modifier"] = { link = "@keyword" },
    ["@keyword.repeat"] = { link = "@keyword" },
    ["@keyword.return"] = { link = "@keyword" },
    ["@keyword.debug"] = { link = "@keyword" },
    ["@keyword.exception"] = { link = "@keyword" },
    ["@keyword.conditional"] = { link = "@keyword" },
    ["@keyword.conditional.ternary"] = { fg = c.dim },
    ["@keyword.directive"] = { link = "@keyword" },
    ["@keyword.directive.define"] = { link = "@keyword" },

    ["@punctuation.delimiter"] = { fg = c.dim },
    ["@punctuation.bracket"] = { fg = c.dim },
    ["@punctuation.special"] = { fg = c.dim },

    ["@comment"] = { link = "Comment" },
    ["@comment.documentation"] = { fg = c.secondary, italic = it.comments },
    ["@comment.error"] = { fg = c.red, bg = c.tint.red, bold = true },
    ["@comment.warning"] = { fg = c.orange, bg = c.tint.orange, bold = true },
    ["@comment.todo"] = { fg = c.chip_fg, bg = c.chip_bg, bold = true },
    ["@comment.note"] = { fg = c.search_fg, bg = c.info_bg, bold = true },

    ["@markup.strong"] = { bold = true },
    ["@markup.italic"] = { italic = true },
    ["@markup.strikethrough"] = { strikethrough = true },
    ["@markup.underline"] = { underline = true },
    ["@markup.heading"] = { fg = c.primary, bold = true },
    ["@markup.heading.1"] = { link = "@markup.heading" },
    ["@markup.heading.2"] = { link = "@markup.heading" },
    ["@markup.heading.3"] = { link = "@markup.heading" },
    ["@markup.heading.4"] = { link = "@markup.heading" },
    ["@markup.heading.5"] = { link = "@markup.heading" },
    ["@markup.heading.6"] = { link = "@markup.heading" },
    ["@markup.quote"] = { fg = c.dim, italic = true },
    ["@markup.math"] = { fg = c.literal },
    ["@markup.link"] = { fg = c.focus },
    ["@markup.link.label"] = { fg = c.focus, underline = true },
    -- The label carries the link; the url beside it is a caption.
    ["@markup.link.url"] = { fg = c.dim, underline = true },
    ["@markup.raw"] = { fg = c.secondary },
    ["@markup.raw.block"] = { fg = c.fg },
    ["@markup.list"] = { fg = c.dim },
    ["@markup.list.checked"] = { fg = c.green },
    ["@markup.list.unchecked"] = { fg = c.dim },

    ["@diff.plus"] = { fg = c.green },
    ["@diff.minus"] = { fg = c.red },
    ["@diff.delta"] = { fg = c.blue },

    ["@tag"] = { fg = c.primary },
    ["@tag.builtin"] = { fg = c.primary },
    ["@tag.attribute"] = { fg = c.secondary },
    ["@tag.delimiter"] = { fg = c.dim },

    ["@conceal"] = { fg = c.dim },
  }
end
