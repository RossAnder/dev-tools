-- mini.icons and mini.hipatterns.

return function(c)
  local i = c.icon
  return {
    MiniIconsRed = { fg = i.red },
    MiniIconsOrange = { fg = i.orange },
    MiniIconsYellow = { fg = i.yellow },
    MiniIconsGreen = { fg = i.green },
    MiniIconsCyan = { fg = i.cyan },
    MiniIconsBlue = { fg = i.blue },
    MiniIconsAzure = { fg = i.azure },
    MiniIconsPurple = { fg = i.purple },
    MiniIconsGrey = { fg = i.grey },

    MiniHipatternsFixme = { link = "@comment.error" },
    MiniHipatternsHack = { link = "@comment.warning" },
    MiniHipatternsTodo = { link = "@comment.todo" },
    MiniHipatternsNote = { link = "@comment.note" },
  }
end
