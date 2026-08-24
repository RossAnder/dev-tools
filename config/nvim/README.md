# nvim

Ross's Neovim config (LazyVim-based), mirrored from `~/.config/nvim` on the
Arch box, with the platform-specific bits adapted so the same tree runs on
native Windows as well as under WSL.

## Setup

Native Windows and WSL are two independent Neovim installs — separate config
dirs, separate plugin trees, separate Mason toolchains. Set up whichever you
actually edit in; doing both is fine and they won't interfere.

Assumes Neovim 0.12+ is already installed on the side you're setting up.
Confirm with `nvim --version` — on 0.11 this config throws at startup
(`vim.lsp.document_color` in `lua/config/autocmds.lua`, `vim.pos` /
`vim.range.lsp` in `lua/util/colors.lua`).

### Native Windows

Config lives in `%LOCALAPPDATA%\nvim`; plugin/state data in `%LOCALAPPDATA%\nvim-data`.

```powershell
# 1. back up anything already there
Move-Item $env:LOCALAPPDATA\nvim      $env:LOCALAPPDATA\nvim.bak      -EA SilentlyContinue
Move-Item $env:LOCALAPPDATA\nvim-data $env:LOCALAPPDATA\nvim-data.bak -EA SilentlyContinue

# 2. tooling lazy.nvim and LazyVim expect on PATH
winget install Git.Git BurntSushi.ripgrep.MSVC sharkdp.fd zig.zig
#    zig doubles as the C compiler for nvim-treesitter parser builds;
#    Visual Studio Build Tools works too if you already have it.

# 3. copy this directory in (from the dev-tools repo root)
Copy-Item -Recurse .\config\nvim $env:LOCALAPPDATA\nvim

# 4. first launch — lazy.nvim bootstraps itself and installs the locked set
nvim
```

Then set your terminal to a Nerd Font (Windows Terminal → Settings → your
profile → Appearance → Font face, e.g. *JetBrainsMono Nerd Font*), or every
LazyVim icon renders as a box.

### WSL

WSL is Linux, so the platform guards fall back to their Linux branches and you
get the original behaviour. Config goes in `~/.config/nvim` inside the distro —
**not** on `/mnt/c`, where plugin loading and treesitter builds crawl.

```bash
# 1. back up anything already there
mv ~/.config/nvim ~/.config/nvim.bak 2>/dev/null
mv ~/.local/share/nvim ~/.local/share/nvim.bak 2>/dev/null

# 2. tooling (Debian/Ubuntu; swap in your distro's package manager)
sudo apt install -y git build-essential ripgrep fd-find unzip

# 3. copy this directory in (from the dev-tools repo root)
mkdir -p ~/.config/nvim && cp -r config/nvim/. ~/.config/nvim/

# 4. first launch
nvim
```

Two WSL-specific things worth doing:

- **Clipboard.** Install `win32yank` on the Windows side
  (`winget install equalsraf.win32yank`); WSL inherits Windows' PATH, so
  Neovim's built-in provider picks it up and `"+y` / `"+p` cross the boundary.
  Check with `:checkhealth vim.provider`.
- **Opening links.** `sudo apt install wslu` gives you `wslview`, which
  markdown-preview (`<leader>cp`) and `gx` need to reach the Windows browser.

If you use QML in WSL, note that `lua/plugins/qml.lua`'s Linux branch hardcodes
the Arch path `/usr/lib/qt6/bin/qmlformat`. Debian/Ubuntu install it to the same
place (`apt install qt6-declarative-dev-tools`) but do not put that directory on
PATH — verify with `ls /usr/lib/qt6/bin/qmlformat` and edit `qmlformat_cmd` in
that file if your distro puts it elsewhere.

### Both

`lazy-lock.json` is committed, so the first launch reproduces the exact plugin
revisions from the Arch machine. `:Lazy update` moves to HEAD when you want it
(`checker.enabled = true` already polls silently without notifying).

Watch `:Lazy` finish, then run `:checkhealth` and `:Mason` to confirm the LSP
servers and formatters installed.

## Prerequisites beyond the setup steps

Per-language, installed on demand rather than upfront:

| Need | Why |
| --- | --- |
| `rustup component add rust-analyzer` | rustaceanvim (`lua/plugins/rustacean.lua`) |
| `bun` or `node` per project | oxfmt / oxlint resolve from the project's `node_modules/.bin` |
| Qt bin dir on PATH (native) or `/usr/lib/qt6/bin` present (WSL) | `qmlls` / `qmlformat`; skip entirely if you don't touch QML |

## Windows deltas from the Linux original

Two files diverge; everything else is byte-identical to `~/.config/nvim`.

- **`lua/plugins/rustacean.lua`** — drops the
  `cmd = { "env", "LD_PRELOAD=", … , "rust-analyzer" }` lines. `env` and
  `LD_PRELOAD` are Linux-only. They were also nested under `settings`, where
  rustaceanvim never reads them, so they were dead config on Linux too — hence
  dropped rather than relocated to `server.cmd`, which would newly activate a
  wrapper that has never been in effect.
- **`lua/plugins/qml.lua`** — `qmlformat` resolves from PATH instead of the
  Arch absolute path `/usr/lib/qt6/bin/qmlformat`, and the `<leader>Q*`
  Quickshell keymaps are gated off (Quickshell is a Wayland compositor shell
  with no Windows build). Both are behind a `vim.fn.has("win32")` check, so the
  file still behaves as before when run on Linux.

`lua/plugins/example.lua` is the untouched LazyVim starter sample — it
short-circuits with `if true then return {} end` and loads nothing.

## Layout

```
init.lua              -- requires config.lazy
lazyvim.json          -- enabled LazyVim extras (dap, rust, svelte, tailwind, ts, …)
lazy-lock.json        -- pinned plugin revisions
lua/config/           -- lazy bootstrap, options, keymaps, autocmds
lua/plugins/          -- per-plugin overrides (colours, kanso theme, oxfmt, oxlint, qml, rust)
lua/util/colors.lua   -- LSP colour-presentation cycling (<leader>cO)
snippets/qml.lua      -- LuaSnip Quickshell/QML snippet pack
```
