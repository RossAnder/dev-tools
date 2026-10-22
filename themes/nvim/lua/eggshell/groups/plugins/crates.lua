-- crates.nvim. A current version is unremarkable; only drift is coloured.

return function(c)
  return {
    CratesNvimLoading = { fg = c.faint },
    CratesNvimSearching = { fg = c.faint },
    CratesNvimVersion = { fg = c.dim },
    CratesNvimPreRelease = { fg = c.yellow },
    CratesNvimYanked = { fg = c.red },
    CratesNvimNoMatch = { fg = c.red },
    CratesNvimUpgrade = { fg = c.orange },
    CratesNvimError = { fg = c.red },

    CratesNvimPopupTitle = { fg = c.primary, bold = true },
    CratesNvimPopupPillText = { fg = c.chip_fg, bg = c.chip_bg },
    CratesNvimPopupPillBorder = { fg = c.chip_bg },
    CratesNvimPopupDescription = { fg = c.fg, italic = true },
    CratesNvimPopupLabel = { fg = c.secondary },
    CratesNvimPopupValue = { fg = c.literal },
    CratesNvimPopupUrl = { fg = c.focus, underline = true },
    CratesNvimPopupVersion = { fg = c.fg },
    CratesNvimPopupPreRelease = { fg = c.yellow },
    CratesNvimPopupYanked = { fg = c.red },
    CratesNvimPopupVersionDate = { fg = c.dim, italic = true },
    CratesNvimPopupFeature = { fg = c.fg },
    CratesNvimPopupEnabled = { fg = c.green },
    CratesNvimPopupTransitive = { fg = c.blue },
    CratesNvimPopupNormalDependenciesTitle = { fg = c.primary, bold = true },
    CratesNvimPopupBuildDependenciesTitle = { fg = c.primary, bold = true },
    CratesNvimPopupDevDependenciesTitle = { fg = c.primary, bold = true },
    CratesNvimPopupDependency = { fg = c.fg },
    CratesNvimPopupOptional = { fg = c.dim, italic = true },
    CratesNvimPopupDependencyVersion = { fg = c.dim },
    CratesNvimPopupLoading = { fg = c.faint },
  }
end
