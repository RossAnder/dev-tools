-- lazy.nvim specs for a LazyVim config; `scripts/themes.sh deploy nvim` links
-- this file into lua/plugins/. The plugin directory is this file's own, found
-- through the link. It makes the scheme available and dresses bufferline; the
-- colorscheme itself is chosen in your own config, because LazyVim merges
-- `colorscheme` from every spec and the last file alphabetically wins.

local dir = vim.fs.dirname(vim.uv.fs_realpath(debug.getinfo(1, "S").source:sub(2)))

return {
  { dir = dir, name = "eggshell", lazy = false, priority = 1000 },
  {
    "akinsho/bufferline.nvim",
    optional = true,
    opts = function(_, opts)
      opts.highlights = require("eggshell.bufferline").highlights
    end,
  },
}
