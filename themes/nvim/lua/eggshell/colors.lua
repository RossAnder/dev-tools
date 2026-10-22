-- Roles from themes/README.md, resolved against one variant of the palette.
-- Group files read these names, never palette tokens, so a role that differs
-- between dark and light is decided once, here.

local M = {}

---@param p table one variant of eggshell.palette, after on_palette
---@param variant "dark"|"light"
---@param o table options
function M.roles(p, variant, o)
  local dark = variant == "dark"
  local n, b, a, t = p.neutral, p.blue, p.accent, p.tint
  local blue = dark and a.blue or b.blue
  local recess = dark and n.black or n.sunk
  local none = "NONE"

  local c = {
    variant = variant,
    none = none,

    -- Surfaces. `bg` stays solid for text drawn on a fill (the cursor);
    -- `editor`, `strip` and `bar` are what `transparent` clears.
    bg = n.base,
    editor = o.transparent and none or n.base,
    strip = o.transparent and none or recess,
    float = n.raised,
    bar = o.transparent and none or n.raised,
    current = dark and n.active or n.sunk,
    -- The editor's cursor line is on screen all the time, so in dark it sits a
    -- step below a list's current row; references must still show on it.
    cursorline = dark and n.raised or n.sunk,
    reference = dark and n.active or n.hairline,
    divider = n.hairline,
    border = n.edge,
    guide = n.mute,
    scope = dark and n.faint or n.dim,
    shadow = n.black,

    -- Text.
    primary = n.primary,
    fg = n.soft,
    secondary = n.secondary,
    dim = n.dim,
    faint = n.faint,

    -- Blue: selection, focus, literals.
    selection = dark and b.field or b.lavender,
    focus = dark and b.lavender or b.morpho,
    literal = blue,
    chip_bg = dark and b.lavender or b.morpho,
    chip_fg = dark and b.morpho or n.white,
    search_bg = dark and b.night or b.wash,
    search_fg = dark and b.lavender or b.morpho,
    info_bg = dark and b.night or b.wash,

    -- State.
    red = a.red,
    orange = a.orange,
    yellow = a.yellow,
    green = a.green,
    cyan = a.cyan,
    blue = blue,
    tint = {
      red = t.red,
      orange = t.orange,
      yellow = t.yellow,
      green = t.green,
      cyan = t.cyan,
      blue = dark and b.night or b.wash,
    },
  }

  c.inactive = o.dim_inactive and recess or c.editor

  -- File-type icons are the one decorative use of hue, and they appear on
  -- every row of every tree, tab and picker, so dark takes the dim tier: the
  -- hue identifies the type while the icon stays below the text. Light takes
  -- the accents. Magenta keeps no role, so purple icons fall back to grey.
  local tier = dark and p.dim or a
  c.icon = {
    red = tier.red,
    orange = tier.orange,
    yellow = tier.yellow,
    green = tier.green,
    cyan = tier.cyan,
    blue = dark and tier.blue or blue,
    azure = dark and tier.cyan or c.focus,
    purple = n.secondary,
    grey = n.dim,
  }

  if dark then
    c.terminal = {
      n.active, a.red, a.green, a.yellow, a.blue, a.magenta, a.cyan, n.soft,
      n.faint, p.bright.red, p.bright.green, p.bright.yellow, b.lavender,
      p.bright.magenta, p.bright.cyan, n.white,
    }
  else
    c.terminal = {
      n.soft, a.red, a.green, a.yellow, b.blue, a.magenta, a.cyan, n.faint,
      n.dim, a.red, a.green, a.yellow, b.morpho, a.magenta, a.cyan, n.sunk,
    }
  end

  return c
end

return M
