-- The same strip as BufferLine* groups, for a bufferline set up without
-- eggshell's `highlights` option. bufferline defines its groups with
-- `default`, so these win.

return function(c)
  local hl = {}
  for key, spec in pairs(require("eggshell.bufferline").theme(c)) do
    local name = key:gsub("_(.)", string.upper):gsub("^%l", string.upper)
    hl["BufferLine" .. name] = spec
  end
  return hl
end
