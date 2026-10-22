-- lazy.nvim's UI. Load reasons are grey labels, not a key of hues.

return function(c)
  local hl = {
    LazyNormal = { fg = c.fg, bg = c.float },
    LazyH1 = { fg = c.chip_fg, bg = c.chip_bg, bold = true },
    LazyH2 = { fg = c.primary, bold = true },
    LazyButton = { fg = c.secondary, bg = c.current },
    LazyButtonActive = { fg = c.chip_fg, bg = c.chip_bg, bold = true },
    LazyComment = { fg = c.dim },
    LazyCommit = { fg = c.faint },
    LazyCommitIssue = { fg = c.literal },
    LazyCommitType = { fg = c.secondary, bold = true },
    LazyCommitScope = { fg = c.dim, italic = true },
    LazyDimmed = { fg = c.faint },
    LazyProp = { fg = c.dim },
    LazyValue = { fg = c.focus },
    LazyNoCond = { fg = c.orange },
    LazyLocal = { fg = c.secondary },
    LazyProgressDone = { fg = c.focus },
    LazyProgressTodo = { fg = c.guide },
    LazySpecial = { fg = c.dim },
    LazyTaskOutput = { fg = c.fg },
    LazyError = { fg = c.red },
    LazyWarning = { fg = c.orange },
    LazyInfo = { fg = c.blue },
    LazyDir = { fg = c.dim },
    LazyUrl = { fg = c.focus, underline = true },
  }
  for _, reason in ipairs({
    "Runtime", "Plugin", "Event", "Keys", "Start", "Source", "Ft", "Cmd", "Import", "Require",
  }) do
    hl["LazyReason" .. reason] = { fg = c.secondary }
  end
  return hl
end
