-- ~/.config/nvim/snippets/qml.lua
-- Quickshell / QML snippet pack for the eggshell project.

local ls   = require("luasnip")
local s    = ls.snippet
local i    = ls.insert_node
local t    = ls.text_node
local fmt  = require("luasnip.extras.fmt").fmt

return {
  -- panel: PanelWindow with anchors + exclusive zone
  s({ trig = "panel", dscr = "Quickshell PanelWindow with anchors + exclusive zone" },
    fmt([[
import QtQuick
import Quickshell
import Quickshell.Wayland

PanelWindow {{
    WlrLayershell.namespace: "{ns}"
    WlrLayershell.layer: WlrLayer.{layer}
    WlrLayershell.exclusionMode: ExclusionMode.{excl}
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.{kbd}

    anchors {{
        top: {top}
        left: {left}
        right: {right}
        bottom: {bottom}
    }}

    implicitHeight: {height}
    color: "transparent"

    {body}
}}
]], {
      ns     = i(1, "eggshell:bar"),
      layer  = i(2, "Top"),
      excl   = i(3, "Auto"),
      kbd    = i(4, "None"),
      top    = i(5, "true"),
      left   = i(6, "true"),
      right  = i(7, "true"),
      bottom = i(8, "false"),
      height = i(9, "32"),
      body   = i(10, "// content"),
    })
  ),

  -- singleton: pragma Singleton
  s({ trig = "singleton", dscr = "pragma Singleton wrapper" },
    fmt([[
pragma Singleton

import QtQuick
import Quickshell

Singleton {{
    id: root

    {body}
}}
]], {
      body = i(1, "// properties"),
    })
  ),

  -- ipc: IpcHandler with typed function
  s({ trig = "ipc", dscr = "IpcHandler with target + typed function" },
    fmt([[
import Quickshell.Io

IpcHandler {{
    target: "{target}"

    function {fn}({arg}: {argType}): {ret} {{
        {body}
    }}
}}
]], {
      target  = i(1, "name"),
      fn      = i(2, "doThing"),
      arg     = i(3, "arg"),
      argType = i(4, "string"),
      ret     = i(5, "string"),
      body    = i(6, 'return "ok"'),
    })
  ),

  -- variants: Variants over Quickshell.screens
  s({ trig = "variants", dscr = "Variants over Quickshell.screens with PanelWindow delegate" },
    fmt([[
import Quickshell

Variants {{
    model: Quickshell.screens
    delegate: Component {{
        PanelWindow {{
            required property var modelData
            screen: modelData

            {body}
        }}
    }}
}}
]], {
      body = i(1, "// per-screen content"),
    })
  ),

  -- notif: NotificationServer with onNotification
  s({ trig = "notif", dscr = "NotificationServer with onNotification handler" },
    fmt([[
import Quickshell.Services.Notifications

NotificationServer {{
    keepOnReload: true
    actionsSupported: true
    actionIconsSupported: true
    bodySupported: true
    bodyMarkupSupported: true
    imageSupported: true
    persistenceSupported: true

    onNotification: notif => {{
        notif.tracked = true;
        {body}
    }}
}}
]], {
      body = i(1, "// add to popups, schedule dismiss timer"),
    })
  ),

  -- proc: Process with command + onExited
  s({ trig = "proc", dscr = "Process with command + onExited handler" },
    fmt([[
import Quickshell.Io

Process {{
    command: [{cmd}]
    running: {running}

    onExited: (exitCode, exitStatus) => {{
        {body}
    }}
}}
]], {
      cmd     = i(1, '"echo", "hi"'),
      running = i(2, "false"),
      body    = i(3, "// handle"),
    })
  ),

  -- file: FileView + JsonAdapter with onAdapterUpdated wiring
  --   This snippet INCLUDES the writeAdapter() call so users don't fall into
  --   the no-auto-persist trap.
  s({ trig = "file", dscr = "FileView + JsonAdapter (with onAdapterUpdated: writeAdapter() wiring)" },
    fmt([[
import Quickshell.Io

FileView {{
    id: {id}
    path: {path}

    adapter: JsonAdapter {{
        {props}
    }}

    onLoaded: {{
        {onLoaded}
    }}

    // CRITICAL: JsonAdapter does NOT auto-persist. Wire writeAdapter() here.
    onAdapterUpdated: writeAdapter()
}}
]], {
      id       = i(1, "settingsFile"),
      path     = i(2, 'Quickshell.shellDir + "/.cache/settings.json"'),
      props    = i(3, 'property string theme: "dark"'),
      onLoaded = i(4, "// hydrated"),
    })
  ),
}
