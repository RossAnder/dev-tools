-- noice.nvim. The cmdline popup is a raised float with a visible border and
-- its kind as a chip in the title; search gets no warning colour.

return function(c)
  local function chip()
    return { fg = c.chip_fg, bg = c.chip_bg, bold = true }
  end
  local hl = {
    NoiceCmdline = { fg = c.fg, bg = c.editor },
    NoiceCmdlineIcon = { fg = c.focus },
    NoiceCmdlineIconSearch = { fg = c.focus },
    NoiceCmdlinePrompt = { fg = c.primary, bold = true },
    NoiceCmdlinePopup = { fg = c.fg, bg = c.float },
    NoiceCmdlinePopupBorder = { fg = c.border, bg = c.float },
    NoiceCmdlinePopupBorderSearch = { fg = c.border, bg = c.float },
    NoiceCmdlinePopupTitle = chip(),
    NoiceConfirm = { fg = c.fg, bg = c.float },
    NoiceConfirmBorder = { fg = c.border, bg = c.float },
    NoiceCursor = { link = "Cursor" },
    NoiceMini = { fg = c.dim, bg = c.float },
    NoicePopup = { fg = c.fg, bg = c.float },
    NoicePopupBorder = { fg = c.border, bg = c.float },
    NoicePopupmenu = { link = "Pmenu" },
    NoicePopupmenuBorder = { link = "PmenuBorder" },
    NoicePopupmenuMatch = { link = "PmenuMatch" },
    NoicePopupmenuSelected = { link = "PmenuSel" },
    NoiceScrollbar = { link = "PmenuSbar" },
    NoiceScrollbarThumb = { link = "PmenuThumb" },
    NoiceSplit = { fg = c.fg, bg = c.float },
    NoiceSplitBorder = { fg = c.border, bg = c.float },
    NoiceVirtualText = { link = "DiagnosticVirtualTextInfo" },

    NoiceFormatProgressDone = { fg = c.chip_fg, bg = c.chip_bg },
    NoiceFormatProgressTodo = { fg = c.dim, bg = c.current },
    NoiceFormatEvent = { fg = c.faint },
    NoiceFormatKind = { fg = c.faint },
    NoiceFormatDate = { fg = c.dim },
    NoiceFormatConfirm = { bg = c.current },
    NoiceFormatConfirmDefault = { fg = c.chip_fg, bg = c.chip_bg, bold = true },
    NoiceFormatTitle = { fg = c.primary, bold = true },
    NoiceFormatLevelDebug = { fg = c.faint },
    NoiceFormatLevelTrace = { fg = c.faint },
    NoiceFormatLevelOff = { fg = c.faint },
    NoiceFormatLevelInfo = { link = "DiagnosticVirtualTextInfo" },
    NoiceFormatLevelWarn = { link = "DiagnosticVirtualTextWarn" },
    NoiceFormatLevelError = { link = "DiagnosticVirtualTextError" },
    NoiceLspProgressSpinner = { fg = c.focus },
    NoiceLspProgressTitle = { fg = c.dim },
    NoiceLspProgressClient = { fg = c.secondary, bold = true },
    NoiceCompletionItemKindDefault = { fg = c.dim },
  }
  for _, kind in ipairs({ "Cmdline", "Search", "Filter", "Lua", "Help", "Calculator", "Input", "IncRename" }) do
    hl["NoiceCmdlinePopupTitle" .. kind] = chip()
  end
  return hl
end
