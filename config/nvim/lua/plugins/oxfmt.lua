-- ~/.config/nvim/lua/plugins/oxfmt.lua
-- Format-on-save with oxfmt, matching `bun run format` in projects that use it
-- (proto-ui, books-rs/web).
--
-- conform's built-in oxfmt formatter resolves the binary from the project's
-- node_modules/.bin and runs from the directory holding .oxfmtrc.json, so it
-- picks up each project's own config. In projects without oxfmt installed the
-- formatter is unavailable and conform falls back to LSP formatting as before.
--
-- oxfmt covers Prettier's ground: js/ts/jsx/tsx, svelte, vue, astro, json,
-- css/scss, html, yaml and markdown. Verified against oxfmt 0.60 by formatting
-- one file per type; only Python was left untouched.

return {
  {
    "stevearc/conform.nvim",
    opts = {
      formatters_by_ft = {
        javascript = { "oxfmt" },
        javascriptreact = { "oxfmt" },
        typescript = { "oxfmt" },
        typescriptreact = { "oxfmt" },
        svelte = { "oxfmt" },
        vue = { "oxfmt" },
        astro = { "oxfmt" },
        json = { "oxfmt" },
        jsonc = { "oxfmt" },
        css = { "oxfmt" },
        scss = { "oxfmt" },
        html = { "oxfmt" },
        yaml = { "oxfmt" },

        -- Replaces LazyVim's markdown chain, which is
        -- { "prettier", "markdownlint-cli2", "markdown-toc" }. prettier is not
        -- installed here, so markdownlint-cli2 was the formatter that actually
        -- ran on save -- and its MD010/no-hard-tabs rule rewrites every tab
        -- inside a fenced code block to a single space, mangling tab-indented
        -- code samples (172 such lines in one proto-ui guidebook chapter).
        --
        -- oxfmt aligns markdown tables and normalises emphasis without that.
        -- For a prose repo whose fenced samples are hand-aligned, set
        -- `"embeddedLanguageFormatting": "off"` in its .oxfmtrc.json, or oxfmt
        -- reformats the code inside the fences too (e.g. 0.010 -> 0.01).
        markdown = { "oxfmt" },
        ["markdown.mdx"] = { "oxfmt" },
      },
    },
  },

  -- markdown-toc only does anything for files carrying a <!-- toc --> marker,
  -- and it is no longer in the markdown formatter chain above. LazyVim's
  -- markdown extra pins it in mason's ensure_installed, so without this it
  -- reinstalls itself on the next sync. marksman (LSP) and markdownlint-cli2
  -- (linter) are deliberately left in place.
  {
    "mason-org/mason.nvim",
    opts = function(_, opts)
      opts.ensure_installed = vim.tbl_filter(function(tool)
        return tool ~= "markdown-toc"
      end, opts.ensure_installed or {})
    end,
  },
}
