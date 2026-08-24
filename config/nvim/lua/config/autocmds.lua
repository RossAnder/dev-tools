-- Autocmds are automatically loaded on the VeryLazy event
-- Default autocmds that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/autocmds.lua
--
-- Add any additional autocmds here
-- with `vim.api.nvim_create_autocmd`
--
-- Or remove existing autocmds by their group name (which is prefixed with `lazyvim_` for the defaults)
-- e.g. vim.api.nvim_del_augroup_by_name("lazyvim_wrap_spell")

-- Zed-style inline colour swatches for LSP document colours (tailwindcss,
-- cssls, svelte). Style options: "background" | "foreground" | "virtual".
vim.lsp.document_color.enable(true, nil, { style = "virtual" })

-- When a client provides document colours for a buffer, let the LSP own colour
-- rendering there — otherwise mini.hipatterns paints the same values twice.
vim.api.nvim_create_autocmd("LspAttach", {
  group = vim.api.nvim_create_augroup("lsp_owns_document_colors", {}),
  callback = function(ev)
    local client = vim.lsp.get_client_by_id(ev.data.client_id)
    if client and client:supports_method("textDocument/documentColor") then
      vim.b[ev.buf].minihipatterns_disable = true
      if package.loaded["mini.hipatterns"] then
        require("mini.hipatterns").disable(ev.buf)
      end
    end
  end,
})
