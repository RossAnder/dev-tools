return {
  {
    "webhooked/kanso.nvim",
    lazy = false,
    priority = 1000,
    config = function()
      require("kanso").setup({
        compile = true,
        theme = "zen",
        background = {
          dark = "zen",
          light = "pearl",
        },
      })
    end,
  },
  {
    "LazyVim/LazyVim",
    opts = {
      colorscheme = "kanso-zen",
    },
  },
}
