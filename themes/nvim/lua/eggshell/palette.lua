-- themes/palette.toml as Lua, token for token. The only file here that holds
-- colour literals; change a value in palette.toml first, then here.

local M = {}

M.dark = {
  neutral = {
    black = "#000000",
    base = "#0a0a0a",
    raised = "#141414",
    hairline = "#1c1c1c",
    active = "#232323",
    edge = "#2e2e2e",
    mute = "#3d3d3d",
    faint = "#5e5e5e",
    dim = "#7a7a7a",
    secondary = "#9e9e9e",
    soft = "#c8c8c8",
    primary = "#f2f2f2",
    white = "#ffffff",
  },
  blue = {
    night = "#0a1836",
    field = "#12276e",
    morpho = "#092382",
    cobalt = "#1a3bb3",
    lavender = "#c4d5ff",
  },
  accent = {
    red = "#e5715a",
    orange = "#fea450",
    yellow = "#f9cb92",
    green = "#72bf89",
    cyan = "#c4d5ff",
    blue = "#5884f8",
    magenta = "#c8c8c8",
  },
  bright = {
    red = "#fb846c",
    orange = "#fdb270",
    yellow = "#fdd39c",
    green = "#77d695",
    cyan = "#c4d5ff",
    magenta = "#f2f2f2",
  },
  tint = {
    red = "#33150f",
    orange = "#2e1a07",
    yellow = "#2a1c0b",
    green = "#0c2515",
    cyan = "#0a1836",
    magenta = "#141414",
  },
  dim = {
    red = "#9a5143",
    orange = "#a16d40",
    yellow = "#9a7a51",
    green = "#4e7b5b",
    cyan = "#5e73a4",
    blue = "#4e66a3",
    magenta = "#7a7a7a",
  },
}

M.light = {
  neutral = {
    white = "#ffffff",
    raised = "#f7f5f1",
    base = "#f1efed",
    sunk = "#e7e4e0",
    hairline = "#dad7d3",
    edge = "#cdcac6",
    mute = "#bab7b3",
    faint = "#a19e9a",
    dim = "#6b6865",
    secondary = "#4f4d49",
    soft = "#302d2b",
    primary = "#171717",
    black = "#000000",
  },
  blue = {
    lavender = "#c4d5ff",
    wash = "#dee8fd",
    blue = "#1a3bb3",
    morpho = "#092382",
  },
  accent = {
    red = "#8d2816",
    orange = "#a66119",
    yellow = "#9c721c",
    green = "#0d542b",
    cyan = "#092382",
    magenta = "#4f4d49",
  },
  tint = {
    red = "#fbe1db",
    orange = "#fae3d1",
    yellow = "#f4e6ce",
    green = "#daeede",
    cyan = "#dee8fd",
    magenta = "#e7e4e0",
  },
  dim = {
    red = "#b9827b",
    orange = "#b28966",
    yellow = "#a98e60",
    green = "#739f7c",
    cyan = "#7e92be",
    blue = "#7e92be",
    magenta = "#a19e9a",
  },
}

M.shell = {
  critical = "#ff453a",
  success = "#03d161",
}

return M
