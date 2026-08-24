-- Options are automatically loaded before lazy.nvim startup
-- Default options that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/options.lua
-- Add any additional options here

-- nvim-treesitter `main` builds parsers by shelling out to `tree-sitter build`,
-- which compiles with $CC. On Windows LazyVim's requirement check only accepts
-- `cl` or `gcc` on PATH, or an MSVC install under "Program Files (x86)" — this
-- machine has VS 18 under "Program Files" and LLVM clang, so neither is found.
-- Naming a compiler explicitly satisfies the check and the build.
if vim.fn.has("win32") == 1 and vim.env.CC == nil then
  for _, cc in ipairs({ "cl", "gcc", "clang" }) do
    if vim.fn.executable(cc) == 1 then
      vim.env.CC = cc
      break
    end
  end
end
