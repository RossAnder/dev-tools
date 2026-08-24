-- ~/.config/nvim/lua/plugins/oxlint.lua
-- oxlint as a language server, so the diagnostics in the buffer are the same
-- ones `bun run lint` reports -- no second lint config to keep in sync.
--
-- nvim-lspconfig ships the `oxlint` config, and its `cmd` prefers the project's
-- own node_modules/.bin/oxlint before falling back to one on PATH -- so a
-- project always lints against the exact version it depends on, which is what
-- keeps this in step with `bun run lint`.
--
-- `mason = false` says "do not make Mason a prerequisite for enabling this".
-- LazyVim may still install a global copy as a fallback; that copy only gets
-- used in a project that has no local oxlint.
--
-- It attaches only in a workspace with .oxlintrc.json / .oxlintrc.jsonc /
-- oxlint.config.ts, which is what keeps it out of unrelated projects.
--
-- Type-aware rules switch on by themselves when `tsgolint` is resolvable and
-- the project's .oxlintrc.json mentions "typescript".
--
-- Attaching also defines :LspOxlintFixAll, which applies every safe autofix in
-- the current buffer.

return {
  {
    "neovim/nvim-lspconfig",
    opts = {
      servers = {
        oxlint = {
          mason = false,
        },
      },
    },
  },
}
