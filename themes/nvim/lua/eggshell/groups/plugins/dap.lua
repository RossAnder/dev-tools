-- nvim-dap, nvim-dap-ui and nvim-dap-virtual-text. A stopped line is
-- pending, so it takes caution's yellow, not the selection blue.

return function(c)
  local hl = {
    DapStoppedLine = { bg = c.tint.yellow },
    DapBreakpoint = { fg = c.red },
    DapBreakpointCondition = { fg = c.orange },
    DapBreakpointRejected = { fg = c.faint },
    DapLogPoint = { fg = c.blue },
    DapStopped = { fg = c.yellow },

    DapUINormal = { link = "Normal" },
    DapUIVariable = { fg = c.fg },
    DapUIScope = { fg = c.primary, bold = true },
    DapUIType = { fg = c.secondary },
    DapUIValue = { fg = c.fg },
    DapUIModifiedValue = { fg = c.orange, bold = true },
    DapUIDecoration = { fg = c.faint },
    DapUIThread = { fg = c.secondary },
    DapUIStoppedThread = { fg = c.yellow },
    DapUIFrameName = { fg = c.fg },
    DapUICurrentFrameName = { fg = c.primary, bold = true },
    DapUISource = { fg = c.dim },
    DapUILineNumber = { fg = c.faint },
    DapUIFloatNormal = { link = "NormalFloat" },
    DapUIFloatBorder = { link = "FloatBorder" },
    DapUIWatchesEmpty = { fg = c.faint },
    DapUIWatchesValue = { fg = c.fg },
    DapUIWatchesError = { fg = c.red },
    DapUIBreakpointsPath = { fg = c.secondary },
    DapUIBreakpointsInfo = { fg = c.blue },
    DapUIBreakpointsCurrentLine = { fg = c.primary, bold = true },
    DapUIBreakpointsLine = { fg = c.faint },
    DapUIBreakpointsDisabledLine = { fg = c.guide },
    DapUIWinSelect = { fg = c.chip_fg, bg = c.chip_bg, bold = true },
    DapUIEndofBuffer = { link = "EndOfBuffer" },

    NvimDapVirtualText = { fg = c.faint, italic = true },
    NvimDapVirtualTextChanged = { fg = c.orange, italic = true },
    NvimDapVirtualTextError = { fg = c.red, italic = true },
    NvimDapVirtualTextInfo = { fg = c.blue, italic = true },
  }

  local controls = {
    PlayPause = c.green,
    Restart = c.green,
    Stop = c.red,
    StepOver = c.secondary,
    StepInto = c.secondary,
    StepBack = c.secondary,
    StepOut = c.secondary,
    Unavailable = c.guide,
  }
  for name, fg in pairs(controls) do
    hl["DapUI" .. name] = { fg = fg, bg = c.editor }
    hl["DapUI" .. name .. "NC"] = { fg = fg, bg = c.inactive }
  end
  return hl
end
