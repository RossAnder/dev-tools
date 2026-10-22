-- Diagnostics, LSP UI and semantic tokens.

return function(c)
  local hl = {
    LspReferenceText = { bg = c.reference },
    LspReferenceRead = { bg = c.reference },
    LspReferenceWrite = { bg = c.reference, underline = true, sp = c.dim },
    LspReferenceTarget = { link = "LspReferenceText" },
    LspInlayHint = { fg = c.faint, bg = c.float },
    LspCodeLens = { fg = c.faint },
    LspCodeLensSeparator = { fg = c.guide },
    LspSignatureActiveParameter = { fg = c.focus, bold = true, underline = true },

    DiagnosticUnnecessary = { fg = c.faint },
    DiagnosticDeprecated = { strikethrough = true },

    -- Semantic tokens: nvim's default links, made explicit where they miss
    -- (macro is not a literal; modifier links a capture that no longer exists).
    ["@lsp.type.boolean"] = { link = "@boolean" },
    ["@lsp.type.builtinType"] = { link = "@type.builtin" },
    ["@lsp.type.class"] = { link = "@type" },
    ["@lsp.type.comment"] = { link = "@comment" },
    ["@lsp.type.const"] = { link = "@constant" },
    ["@lsp.type.decorator"] = { link = "@attribute" },
    ["@lsp.type.deriveHelper"] = { link = "@attribute" },
    ["@lsp.type.enum"] = { link = "@type" },
    ["@lsp.type.enumMember"] = { link = "@constant" },
    ["@lsp.type.escapeSequence"] = { link = "@string.escape" },
    ["@lsp.type.event"] = { link = "@type" },
    ["@lsp.type.formatSpecifier"] = { link = "@punctuation.special" },
    ["@lsp.type.function"] = { link = "@function" },
    ["@lsp.type.generic"] = { link = "@variable" },
    ["@lsp.type.interface"] = { link = "@type" },
    ["@lsp.type.keyword"] = { link = "@keyword" },
    ["@lsp.type.lifetime"] = { link = "@label" },
    ["@lsp.type.macro"] = { link = "@function.macro" },
    ["@lsp.type.method"] = { link = "@function.method" },
    ["@lsp.type.modifier"] = { link = "@keyword.modifier" },
    ["@lsp.type.namespace"] = { link = "@module" },
    ["@lsp.type.number"] = { link = "@number" },
    ["@lsp.type.operator"] = { link = "@operator" },
    ["@lsp.type.parameter"] = { link = "@variable.parameter" },
    ["@lsp.type.property"] = { link = "@property" },
    ["@lsp.type.regexp"] = { link = "@string.regexp" },
    ["@lsp.type.selfKeyword"] = { link = "@variable.builtin" },
    ["@lsp.type.selfTypeKeyword"] = { link = "@variable.builtin" },
    ["@lsp.type.string"] = { link = "@string" },
    ["@lsp.type.struct"] = { link = "@type" },
    ["@lsp.type.type"] = { link = "@type" },
    ["@lsp.type.typeAlias"] = { link = "@type.definition" },
    ["@lsp.type.typeParameter"] = { link = "@type.definition" },
    ["@lsp.type.unresolvedReference"] = { undercurl = true, sp = c.red },
    -- Left empty so treesitter's finer variable captures show through.
    ["@lsp.type.variable"] = {},

    ["@lsp.mod.deprecated"] = { strikethrough = true },
    ["@lsp.typemod.comment.documentation"] = { link = "@comment.documentation" },
    ["@lsp.typemod.class.defaultLibrary"] = { link = "@type.builtin" },
    ["@lsp.typemod.enum.defaultLibrary"] = { link = "@type.builtin" },
    ["@lsp.typemod.struct.defaultLibrary"] = { link = "@type.builtin" },
    ["@lsp.typemod.type.defaultLibrary"] = { link = "@type.builtin" },
    ["@lsp.typemod.typeAlias.defaultLibrary"] = { link = "@type.builtin" },
    ["@lsp.typemod.enumMember.defaultLibrary"] = { link = "@constant.builtin" },
    ["@lsp.typemod.function.defaultLibrary"] = { link = "@function.builtin" },
    ["@lsp.typemod.method.defaultLibrary"] = { link = "@function.builtin" },
    ["@lsp.typemod.macro.defaultLibrary"] = { link = "@function.macro" },
    ["@lsp.typemod.variable.defaultLibrary"] = { link = "@variable.builtin" },
    ["@lsp.typemod.keyword.async"] = { link = "@keyword.coroutine" },
  }

  local levels = {
    Error = { c.red, c.tint.red },
    Warn = { c.orange, c.tint.orange },
    Info = { c.blue, c.tint.blue },
    Hint = { c.cyan, c.tint.cyan },
    Ok = { c.green, c.tint.green },
  }
  for level, col in pairs(levels) do
    local fg, tint = col[1], col[2]
    hl["Diagnostic" .. level] = { fg = fg }
    hl["DiagnosticVirtualText" .. level] = { fg = fg, bg = tint }
    hl["DiagnosticVirtualLines" .. level] = { fg = fg }
    hl["DiagnosticUnderline" .. level] = { undercurl = true, sp = fg }
    hl["DiagnosticFloating" .. level] = { fg = fg, bg = c.float }
    hl["DiagnosticSign" .. level] = { fg = fg, bg = c.editor }
  end

  return hl
end
