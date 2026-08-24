-- Convert the colour under the cursor between formats via LSP
-- textDocument/colorPresentation (cssls offers rgb/hex/hsl/hwb/lab/lch/
-- oklab/oklch). Public client APIs only — no vim.lsp._capability internals.
local M = {}

local api = vim.api

---@alias util.colors.Format "oklch"|"hex"|"rgb"

-- Cycle successors: oklch -> rgb -> hex -> oklch, so a hex colour reaches
-- oklch in one press. Any unrecognised format converts to oklch first.
local ring = { "oklch", "rgb", "hex" }

local patterns = {
  oklch = "^oklch%(",
  hex = "^#",
  rgb = "^rgba?%(",
}

---@param label string
---@param format util.colors.Format
local function label_matches(label, format)
  return label:lower():find(patterns[format]) ~= nil
end

--- Find the LSP colour covering the cursor. Calls `cb(client, color_info)`
--- with the raw lsp.ColorInformation, or notifies if there is none.
---@param cb fun(client: vim.lsp.Client, info: lsp.ColorInformation)
local function color_under_cursor(cb)
  local bufnr = api.nvim_get_current_buf()
  local clients = vim.lsp.get_clients({ bufnr = bufnr, method = "textDocument/documentColor" })
  if #clients == 0 then
    return vim.notify("No LSP client with colour support attached.", vim.log.levels.WARN)
  end

  local row, col = unpack(api.nvim_win_get_cursor(0))
  local cursor_pos = vim.pos(bufnr, row - 1, col)
  local pending = #clients
  local found = false

  for _, client in ipairs(clients) do
    client:request("textDocument/documentColor", {
      textDocument = { uri = vim.uri_from_bufnr(bufnr) },
    }, function(err, result)
      pending = pending - 1
      if found or api.nvim_get_current_buf() ~= bufnr then
        return
      end
      if not err then
        for _, info in ipairs(result or {}) do
          local range = vim.range.lsp(bufnr, info.range, client.offset_encoding)
          if range:has(cursor_pos) then
            found = true
            return cb(client, info)
          end
        end
      end
      if pending == 0 then
        vim.notify("No colour under cursor.", vim.log.levels.WARN)
      end
    end, bufnr)
  end
end

--- Rewrite the colour under the cursor as `format`.
---@param format util.colors.Format
function M.convert(format)
  local bufnr = api.nvim_get_current_buf()
  color_under_cursor(function(client, info)
    client:request("textDocument/colorPresentation", {
      textDocument = { uri = vim.uri_from_bufnr(bufnr) },
      color = info.color,
      range = info.range,
    }, function(err, result)
      if err or api.nvim_get_current_buf() ~= bufnr then
        return
      end
      for _, pres in ipairs(result or {}) do
        if label_matches(pres.label, format) then
          local edits = { pres.textEdit or { range = info.range, newText = pres.label } }
          vim.list_extend(edits, pres.additionalTextEdits or {})
          return vim.lsp.util.apply_text_edits(edits, bufnr, client.offset_encoding)
        end
      end
      vim.notify(("No %s presentation offered here."):format(format), vim.log.levels.WARN)
    end, bufnr)
  end)
end

--- Cycle the colour under the cursor: oklch -> hex -> rgb(a) -> oklch.
--- Anything else (hsl, named colours, ...) converts to oklch first.
function M.cycle()
  local bufnr = api.nvim_get_current_buf()
  color_under_cursor(function(client, info)
    local range = vim.range.lsp(bufnr, info.range, client.offset_encoding)
    local text = api.nvim_buf_get_text(
      bufnr, range.start_row, range.start_col, range.end_row, range.end_col, {}
    )[1] or ""

    local next_format = "oklch"
    for i, format in ipairs(ring) do
      if text:lower():find(patterns[format]) then
        next_format = ring[i % #ring + 1]
        break
      end
    end
    M.convert(next_format)
  end)
end

return M
