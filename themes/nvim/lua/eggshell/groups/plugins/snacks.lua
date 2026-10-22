-- snacks.nvim: windows, picker and explorer, dashboard, notifier, indent,
-- input, words, statuscolumn, zen, dim.

return function(c)
  local function chip()
    return { fg = c.chip_fg, bg = c.chip_bg, bold = true }
  end
  local hl = {
    SnacksNormal = { fg = c.fg, bg = c.float },
    SnacksNormalNC = { fg = c.fg, bg = c.float },
    SnacksWinBar = { fg = c.secondary, bold = true },
    SnacksWinBarNC = { fg = c.faint },
    SnacksTitle = { link = "FloatTitle" },
    SnacksFooter = { link = "FloatFooter" },
    SnacksFooterKey = chip(),
    SnacksFooterDesc = { fg = c.dim, bg = c.float },
    SnacksWinKey = { fg = c.focus },
    SnacksWinKeySep = { fg = c.guide },
    SnacksWinKeyDesc = { fg = c.secondary },
    SnacksWinSeparator = { link = "WinSeparator" },
    SnacksBackdrop = { bg = c.shadow },

    -- The picker's current row is where you are (grey); a picked item is
    -- what you have selected (blue).
    SnacksPicker = { fg = c.fg, bg = c.float },
    SnacksPickerBorder = { fg = c.border, bg = c.float },
    SnacksPickerTitle = { link = "FloatTitle" },
    SnacksPickerFooter = { link = "FloatFooter" },
    SnacksPickerCursorLine = { bg = c.current },
    SnacksPickerList = { fg = c.fg, bg = c.float },
    SnacksPickerListBorder = { fg = c.border, bg = c.float },
    SnacksPickerListTitle = { link = "FloatTitle" },
    SnacksPickerListCursorLine = { bg = c.current },
    SnacksPickerInput = { fg = c.primary, bg = c.float },
    SnacksPickerInputBorder = { fg = c.border, bg = c.float },
    SnacksPickerInputTitle = chip(),
    SnacksPickerInputSearch = { fg = c.focus },
    SnacksPickerBox = { fg = c.fg, bg = c.float },
    SnacksPickerBoxBorder = { fg = c.border, bg = c.float },
    SnacksPickerBoxTitle = { link = "FloatTitle" },
    SnacksPickerPreview = { fg = c.fg, bg = c.float },
    SnacksPickerPreviewBorder = { fg = c.border, bg = c.float },
    SnacksPickerPreviewTitle = { link = "FloatTitle" },
    SnacksPickerPreviewCursorLine = { bg = c.current },
    SnacksPickerMatch = { fg = c.focus, bold = true },
    SnacksPickerSearch = { link = "Search" },
    SnacksPickerPrompt = { fg = c.focus },
    SnacksPickerSelected = { fg = c.focus },
    SnacksPickerUnselected = { fg = c.guide },
    SnacksPickerToggle = { fg = c.search_fg, bg = c.search_bg },
    SnacksPickerSpecial = { fg = c.secondary },
    SnacksPickerLabel = { fg = c.secondary },
    SnacksPickerTotals = { fg = c.faint },
    SnacksPickerFile = { fg = c.fg },
    SnacksPickerDir = { fg = c.dim },
    SnacksPickerDirectory = { fg = c.primary },
    SnacksPickerPathHidden = { fg = c.faint },
    SnacksPickerPathIgnored = { fg = c.guide },
    SnacksPickerLink = { fg = c.dim },
    SnacksPickerLinkBroken = { fg = c.red },
    SnacksPickerDimmed = { fg = c.faint },
    SnacksPickerRow = { fg = c.faint },
    SnacksPickerCol = { fg = c.faint },
    SnacksPickerTree = { fg = c.guide },
    SnacksPickerComment = { link = "Comment" },
    SnacksPickerDesc = { fg = c.dim },
    SnacksPickerDelim = { fg = c.dim },
    SnacksPickerSpinner = { fg = c.focus },
    SnacksPickerIdx = { fg = c.faint },
    SnacksPickerIcon = { fg = c.dim },
    SnacksPickerTime = { fg = c.dim },
    SnacksPickerBufNr = { fg = c.faint },
    SnacksPickerBufFlags = { fg = c.faint },
    SnacksPickerFileType = { fg = c.dim },
    SnacksPickerKeymapMode = { fg = c.dim },
    SnacksPickerKeymapLhs = { fg = c.primary },
    SnacksPickerKeymapRhs = { fg = c.dim },
    SnacksPickerRegister = { fg = c.literal },
    SnacksPickerGitCommit = { fg = c.faint },
    SnacksPickerGitBranch = { fg = c.secondary },
    SnacksPickerGitBranchCurrent = { fg = c.primary, bold = true },
    SnacksPickerGitDate = { fg = c.dim },
    SnacksPickerGitAuthor = { fg = c.secondary },
    SnacksPickerGitType = { fg = c.secondary, bold = true },
    SnacksPickerGitScope = { fg = c.dim, italic = true },
    SnacksPickerGitIssue = { fg = c.literal },
    SnacksPickerGitBreaking = { fg = c.red, bold = true },
    SnacksPickerGitDetached = { fg = c.orange },
    SnacksPickerGitStatus = { fg = c.dim },
    SnacksPickerGitStatusAdded = { fg = c.green },
    SnacksPickerGitStatusModified = { fg = c.orange },
    SnacksPickerGitStatusDeleted = { fg = c.red },
    SnacksPickerGitStatusRenamed = { fg = c.blue },
    SnacksPickerGitStatusCopied = { fg = c.blue },
    SnacksPickerGitStatusUntracked = { fg = c.faint },
    SnacksPickerGitStatusIgnored = { fg = c.guide },
    SnacksPickerGitStatusUnmerged = { fg = c.red },
    SnacksPickerGitStatusStaged = { fg = c.green },
    SnacksPickerPickWin = chip(),
    SnacksPickerPickWinCurrent = chip(),
    SnacksPickerLspEnabled = { fg = c.secondary },
    SnacksPickerLspAttached = { fg = c.green },
    SnacksPickerLspAttachedBuf = { fg = c.blue },
    SnacksPickerLspDisabled = { fg = c.faint },
    SnacksPickerLspUnavailable = { fg = c.red },

    SnacksDashboardNormal = { fg = c.fg, bg = c.editor },
    SnacksDashboardHeader = { fg = c.secondary },
    SnacksDashboardTitle = { fg = c.primary, bold = true },
    SnacksDashboardDesc = { fg = c.fg },
    SnacksDashboardIcon = { fg = c.dim },
    SnacksDashboardKey = { fg = c.focus, bold = true },
    SnacksDashboardFile = { fg = c.fg },
    SnacksDashboardDir = { fg = c.faint },
    SnacksDashboardFooter = { fg = c.dim },
    SnacksDashboardSpecial = { fg = c.secondary },

    SnacksNotifierHistory = { fg = c.fg, bg = c.float },
    SnacksNotifierHistoryTitle = { fg = c.primary, bold = true },
    SnacksNotifierHistoryDateTime = { fg = c.faint },
    SnacksNotifierMinimal = { fg = c.fg, bg = c.float },

    SnacksIndent = { fg = c.guide },
    SnacksIndentBlank = { fg = c.guide },
    SnacksIndentScope = { fg = c.scope },
    SnacksIndentChunk = { fg = c.scope },

    SnacksInputNormal = { fg = c.fg, bg = c.float },
    SnacksInputBorder = { fg = c.border, bg = c.float },
    SnacksInputTitle = chip(),
    SnacksInputIcon = { fg = c.focus },
    SnacksInputPrompt = { fg = c.focus },

    SnacksStatusColumnMark = { fg = c.secondary },
    SnacksZenIcon = { fg = c.dim },
    SnacksDim = { fg = c.faint },
    SnacksScratchTitle = { link = "FloatTitle" },
  }

  for i = 1, 8 do
    hl["SnacksIndent" .. i] = { fg = c.guide }
  end

  -- Notifications keep a quiet border; the level shows in icon and title.
  local levels = {
    Error = c.red,
    Warn = c.orange,
    Info = c.blue,
    Debug = c.faint,
    Trace = c.faint,
  }
  for level, fg in pairs(levels) do
    hl["SnacksNotifier" .. level] = { fg = c.fg, bg = c.float }
    hl["SnacksNotifierBorder" .. level] = { fg = c.border, bg = c.float }
    hl["SnacksNotifierIcon" .. level] = { fg = fg }
    hl["SnacksNotifierTitle" .. level] = { fg = fg, bold = true }
    hl["SnacksNotifierFooter" .. level] = { fg = c.faint }
  end
  return hl
end
