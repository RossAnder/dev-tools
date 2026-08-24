-- Inline colour swatches (Zed-style) come from the native LSP document-colour
-- support (nvim 0.12+): tailwindcss-language-server reads the real Tailwind v4
-- theme, svelte-language-server covers <style> blocks. Style is set to
-- "virtual" in config/autocmds.lua.
return {
  -- cssls decorates plain CSS colours (hex, rgb(), hsl(), named) that the
  -- tailwind server ignores.
  {
    "neovim/nvim-lspconfig",
    opts = {
      servers = {
        cssls = {},
      },
    },
  },

  -- The mini-hipatterns extra's tailwind integration matches against a
  -- hardcoded Tailwind v3 palette by Lua regex — wrong colours under v4 and
  -- blind to @theme tokens — so the LSP owns tailwind colours instead.
  -- hex_color stays on for buffers without an LSP colour provider.
  {
    "nvim-mini/mini.hipatterns",
    optional = true,
    opts = {
      tailwind = { enabled = false },
    },
  },
}
