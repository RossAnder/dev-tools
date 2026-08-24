-- Keymaps are automatically loaded on the VeryLazy event
-- Default keymaps that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/keymaps.lua
-- Add any additional keymaps here

-- LSP colour presentations (see lua/util/colors.lua). <leader>cp is taken by
-- markdown-preview, hence the capitals.
vim.keymap.set("n", "<leader>cP", function()
  vim.lsp.document_color.color_presentation()
end, { desc = "Colour presentations (pick)" })
vim.keymap.set("n", "<leader>cO", function()
  require("util.colors").cycle()
end, { desc = "Cycle colour format (oklch/hex/rgba)" })
