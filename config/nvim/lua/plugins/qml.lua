-- QML / Quickshell support for the eggshell project.
-- See /home/ross/Dev/eggshell/docs/plans/eggshell-phase-1-bootstrap.md task 4.
--
-- WINDOWS DELTAS vs. the Linux copy of this file:
--   * qmlformat is resolved from PATH instead of the Arch absolute path
--     /usr/lib/qt6/bin/qmlformat. Put Qt's bin dir (e.g.
--     C:\Qt\6.x.y\msvc2022_64\bin) on PATH, or set QMLFORMAT below.
--   * the <leader>Q* Quickshell keymaps are gated off — Quickshell is a
--     Wayland compositor shell and does not exist on Windows.
--   * qmlls likewise needs Qt's bin dir on PATH; without it the server just
--     fails to start and the rest of the QML setup (treesitter, snippets)
--     still works.

local is_windows = vim.fn.has("win32") == 1

-- Absolute path on Arch (where `dev-env.sh` may not be sourced under headless
-- nvim); bare name on Windows so PATH resolution finds qmlformat.exe.
local qmlformat_cmd = is_windows and "qmlformat" or "/usr/lib/qt6/bin/qmlformat"

vim.filetype.add({
  extension = { qmldir = "qmldir" },
})

local spec = {
  -- 1. nvim-lspconfig: register qmlls (Arch ships `qmlls`, NOT `qmlls6`;
  --    on Windows it is qmlls.exe in Qt's bin dir, resolved via PATH).
  {
    "neovim/nvim-lspconfig",
    opts = {
      servers = {
        qmlls = {
          cmd = { "qmlls" },
          filetypes = { "qml", "qmljs" },
          root_markers = { ".qmlls.ini", ".git" },
        },
      },
    },
  },

  -- 2. nvim-treesitter: ensure qmljs parser is installed.
  --    The parser auto-registers for the `qml` filetype on neovim 0.10+;
  --    no manual vim.treesitter.language.register call needed.
  {
    "nvim-treesitter/nvim-treesitter",
    opts = function(_, opts)
      opts.ensure_installed = opts.ensure_installed or {}
      vim.list_extend(opts.ensure_installed, { "qmljs" })
    end,
  },

  -- 3. conform.nvim: enable the built-in qmlformat formatter for qml filetype.
  {
    "stevearc/conform.nvim",
    opts = {
      formatters_by_ft = {
        qml = { "qmlformat" },
      },
      formatters = {
        qmlformat = {
          command = qmlformat_cmd,
        },
      },
    },
  },

  -- 4. LuaSnip: load custom snippets directory. LazyVim does NOT auto-load it.
  {
    "L3MON4D3/LuaSnip",
    config = function(_, opts)
      require("luasnip").setup(opts or {})
      require("luasnip.loaders.from_lua").lazy_load({
        paths = { vim.fn.stdpath("config") .. "/snippets" },
      })
    end,
  },
}

-- 5. Filetype-gated keymaps under the <leader>Q* prefix
--    (LazyVim reserves <leader>q for quit/sessions; use capital Q).
--    Quickshell is Linux/Wayland-only, so these are skipped on Windows.
if not is_windows then
  table.insert(spec, {
    "neovim/nvim-lspconfig",
    keys = {
      { "<leader>Qr", "<cmd>!qs -c eggshell reload<CR>",              desc = "Quickshell: reload",     ft = "qml" },
      { "<leader>Ql", "<cmd>!qs -c eggshell ipc show<CR>",            desc = "Quickshell: list IPC",   ft = "qml" },
      { "<leader>Qp", "<cmd>!qs -c eggshell ipc call shell ping<CR>", desc = "Quickshell: ping shell", ft = "qml" },
    },
  })
end

return spec
