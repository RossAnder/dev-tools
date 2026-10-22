-- todo-comments.nvim. The keyword is a chip; the text after it stays a
-- comment. todo-comments defines these with `hi def`, so these win.

return function(c, o)
  local tags = {
    FIX = { c.red, c.tint.red },
    WARN = { c.orange, c.tint.orange },
    HACK = { c.orange, c.tint.orange },
    NOTE = { c.search_fg, c.info_bg },
    TODO = { c.chip_fg, c.chip_bg },
    PERF = { c.secondary, c.current },
    TEST = { c.secondary, c.current },
  }
  local hl = {}
  for tag, col in pairs(tags) do
    local fg, bg = col[1], col[2]
    -- The chip's text colour reads poorly as a sign on the editor background.
    local sign = tag == "TODO" and c.focus or fg
    hl["TodoBg" .. tag] = { fg = fg, bg = bg, bold = true }
    hl["TodoFg" .. tag] = { fg = c.dim, italic = o.italics.comments }
    hl["TodoSign" .. tag] = { fg = sign, bg = c.editor }
  end
  return hl
end
