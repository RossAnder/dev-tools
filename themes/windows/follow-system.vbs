' Points the apps that cannot follow Windows' light/dark app mode at the matching
' eggshell variant, by copying that variant over a file they read:
'   Claude Code  <claude dir>\themes\eggshell-system.json   (select "Eggshell (system)")
'   gitui        %APPDATA%\gitui\theme.ron, eggshell.tmTheme (read when gitui starts)
'   Alacritty    %APPDATA%\alacritty\eggshell-system.toml (imported; live-reloaded)
' It also keeps the two fixed Claude Code variants equal to this repo's, since file
' symlinks need Developer Mode.
' Run every minute and at logon by the "Eggshell Follow System" scheduled task that
' install-follow-system.ps1 registers. wscript is a GUI-subsystem host, so no console
' window flashes. A destination is rewritten only when its content differs, in place
' rather than by rename: Claude Code reloads on change, and a rename's unlink would
' briefly leave the selected theme missing.
Option Explicit

Dim sh, fso, themes, light, claudeDir
Set sh = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")
themes = fso.GetParentFolderName(fso.GetParentFolderName(WScript.ScriptFullName))

light = False
On Error Resume Next
light = (sh.RegRead("HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme") = 1)
On Error GoTo 0

claudeDir = sh.ExpandEnvironmentStrings("%CLAUDE_CONFIG_DIR%")
If claudeDir = "%CLAUDE_CONFIG_DIR%" Or claudeDir = "" Then
    claudeDir = sh.ExpandEnvironmentStrings("%USERPROFILE%") & "\.claude"
End If

Sync themes & "\claude\eggshell.json", themes & "\claude\eggshell-light.json", _
    claudeDir & "\themes\eggshell-system.json", "Eggshell (system)"
Sync themes & "\gitui\eggshell-dark.tmTheme", themes & "\gitui\eggshell-light.tmTheme", _
    sh.ExpandEnvironmentStrings("%APPDATA%") & "\gitui\eggshell.tmTheme", ""
Sync themes & "\gitui\theme-dark.ron", themes & "\gitui\theme-light.ron", _
    sh.ExpandEnvironmentStrings("%APPDATA%") & "\gitui\theme.ron", ""
Sync themes & "\alacritty\eggshell.toml", themes & "\alacritty\eggshell-light.toml", _
    sh.ExpandEnvironmentStrings("%APPDATA%") & "\alacritty\eggshell-system.toml", ""
Copy themes & "\claude\eggshell.json", claudeDir & "\themes\eggshell.json"
Copy themes & "\claude\eggshell-light.json", claudeDir & "\themes\eggshell-light.json"

Sub Copy(src, dst)
    Sync src, src, dst, ""
End Sub

' rename, when not empty, replaces the first "name" value so the picker shows one
' entry whatever the variant.
Sub Sync(darkSrc, lightSrc, dst, rename)
    Dim src, text, re
    If Not fso.FolderExists(fso.GetParentFolderName(dst)) Then Exit Sub
    If light Then src = lightSrc Else src = darkSrc
    If Not fso.FileExists(src) Then Exit Sub
    text = ReadAll(src)
    If rename <> "" Then
        Set re = New RegExp
        re.Pattern = """name"": *""[^""]*"""
        text = re.Replace(text, """name"": """ & rename & """")
    End If
    If fso.FileExists(dst) Then
        If ReadAll(dst) = text Then Exit Sub
    End If
    With fso.OpenTextFile(dst, 2, True)
        .Write text
        .Close
    End With
End Sub

Function ReadAll(path)
    ReadAll = ""
    If fso.GetFile(path).Size = 0 Then Exit Function
    With fso.OpenTextFile(path, 1)
        ReadAll = .ReadAll
        .Close
    End With
End Function
