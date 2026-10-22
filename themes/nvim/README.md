# eggshell for Neovim

A colorscheme in the shell's palette (`../palette.toml`; roles in
`../README.md`). It follows `'background'`: `dark` loads the dark palette,
`light` the light one, and `:set background=light` switches a loaded scheme.
A lualine theme of the same name is included.

## Install

`scripts/themes.sh deploy nvim` links `lazyvim.lua`, which holds the plugin
and bufferline specs below, into `~/.config/nvim/lua/plugins/eggshell.lua`.
By hand, in a LazyVim config:

```lua
{ dir = "~/Dev/eggshell/themes/nvim", name = "eggshell", lazy = false, priority = 1000 },
```

Then choose it where your colorscheme is set today. LazyVim merges
`colorscheme` from every spec and the file last in alphabetical order wins, so
set it in one place only:

```lua
{ "LazyVim/LazyVim", opts = { colorscheme = "eggshell" } },
```

LazyVim's lualine uses `theme = "auto"`, which picks up `lualine/themes/eggshell.lua`.

bufferline builds its icon highlights from its own `highlights` option, so the
tab strip only matches when that option comes from the scheme:

```lua
{
  "akinsho/bufferline.nvim",
  opts = function(_, opts)
    opts.highlights = require("eggshell.bufferline").highlights
  end,
},
```

The function is re-read on every `:colorscheme` and returns nothing under
another scheme.

## Options

Call `setup` before `:colorscheme`, for example from the spec's `opts`:

```lua
require("eggshell").setup({
  transparent = false, -- leave the editor background unset; floats stay raised
  dim_inactive = false, -- inactive windows take the recessed surface
  italics = { comments = true, keywords = false, parameters = true }, -- or false
  on_palette = function(palette) end, -- edit the variant's tokens (palette.toml names)
  on_highlights = function(hl, palette) end, -- edit the group table before it is set
})
```

`require("eggshell").colors()` returns the resolved roles for the current
variant, and `palette()` its tokens.
