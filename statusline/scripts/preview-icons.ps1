<#
.SYNOPSIS
    Browse candidate statusline glyphs in the terminal that has to render them.

.DESCRIPTION
    Two problems decide whether a glyph works in a terminal statusline, and
    neither is visible from a codepoint chart on the web:

      1. The Unicode *Emoji* property. A codepoint carrying it (U+2733, U+2734,
         U+2728, U+2747 ...) is pulled from the colour-emoji font rather than
         your monospace face, so it lands off-weight, off-baseline, and often
         double-width. Those are marked `!` here.
      2. Font coverage. A glyph your font lacks falls back to whatever the OS
         picks. Pass -Font to check coverage directly.

    The only real test is seeing it in place, so -InSitu renders each candidate
    through the actual statusline binary.

.PARAMETER Binary
    The statusline executable -InSitu renders through. Defaults to the
    cargo-installed one, $env:USERPROFILE\.cargo\bin\statusline.exe, and -InSitu
    throws when that path does not exist. Pass -Binary to preview a build you
    have not installed, or an install that is not on the default path:

      ./preview-icons.ps1 -InSitu -Binary ../target/release/statusline.exe

    Grid mode never runs the binary, so -Binary is irrelevant without -InSitu.

.EXAMPLE
    ./preview-icons.ps1
    Dingbats (U+2700-U+27BF) as a grid, emoji-property codepoints flagged.

.EXAMPLE
    ./preview-icons.ps1 -From 0x25A0 -To 0x25FF
    Geometric Shapes.

.EXAMPLE
    ./preview-icons.ps1 -InSitu
    Every shortlisted glyph rendered inside a real status line.

.EXAMPLE
    ./preview-icons.ps1 -InSitu -From 0x2726 -To 0x273D -Font `
      "$env:LOCALAPPDATA\Microsoft\Windows\Fonts\IosevkaTermNerdFont-Regular.ttf"

.NOTES
    Ranges worth browsing:
      0x2190-0x21FF  Arrows
      0x25A0-0x25FF  Geometric Shapes   (solid, reliably monospaced)
      0x2600-0x26FF  Misc Symbols       (heavily emoji-contaminated)
      0x2700-0x27BF  Dingbats           (the asterisks and stars)
      0x2B00-0x2BFF  Misc Symbols and Arrows
      0xE0A0-0xE0D4  Powerline          (Nerd Font private use)
      0xF000-0xF2FF  Font Awesome       (Nerd Font private use)
#>
[CmdletBinding()]
param(
    [int]    $From   = 0x2700,
    [int]    $To     = 0x27BF,
    [switch] $InSitu,
    [string] $Font,
    [string] $Binary = "$env:USERPROFILE\.cargo\bin\statusline.exe"
)

# Without this the glyphs leave the pipe as '?' and every candidate looks broken.
[Console]::OutputEncoding = [Text.Encoding]::UTF8

# Codepoints below U+3000 carrying the Unicode Emoji property, from
# emoji-data.txt. These are the ones a terminal renders from the colour-emoji
# font no matter what your monospace face contains.
$emojiRanges = @(
    0x203C, 0x203C, 0x2049, 0x2049, 0x2122, 0x2122, 0x2139, 0x2139,
    0x2194, 0x2199, 0x21A9, 0x21AA, 0x231A, 0x231B, 0x2328, 0x2328,
    0x23CF, 0x23CF, 0x23E9, 0x23F3, 0x23F8, 0x23FA, 0x24C2, 0x24C2,
    0x25AA, 0x25AB, 0x25B6, 0x25B6, 0x25C0, 0x25C0, 0x25FB, 0x25FE,
    0x2600, 0x2604, 0x260E, 0x260E, 0x2611, 0x2611, 0x2614, 0x2615,
    0x2618, 0x2618, 0x261D, 0x261D, 0x2620, 0x2620, 0x2622, 0x2623,
    0x2626, 0x2626, 0x262A, 0x262A, 0x262E, 0x262F, 0x2638, 0x263A,
    0x2640, 0x2640, 0x2642, 0x2642, 0x2648, 0x2653, 0x265F, 0x2660,
    0x2663, 0x2663, 0x2665, 0x2666, 0x2668, 0x2668, 0x267B, 0x267B,
    0x267E, 0x267F, 0x2692, 0x2697, 0x2699, 0x2699, 0x269B, 0x269C,
    0x26A0, 0x26A1, 0x26A7, 0x26A7, 0x26AA, 0x26AB, 0x26B0, 0x26B1,
    0x26BD, 0x26BE, 0x26C4, 0x26C5, 0x26C8, 0x26C8, 0x26CE, 0x26CF,
    0x26D1, 0x26D1, 0x26D3, 0x26D4, 0x26E9, 0x26EA, 0x26F0, 0x26F5,
    0x26F7, 0x26FA, 0x26FD, 0x26FD, 0x2702, 0x2702, 0x2705, 0x2705,
    0x2708, 0x270D, 0x270F, 0x270F, 0x2712, 0x2712, 0x2714, 0x2714,
    0x2716, 0x2716, 0x271D, 0x271D, 0x2721, 0x2721, 0x2728, 0x2728,
    0x2733, 0x2734, 0x2744, 0x2744, 0x2747, 0x2747, 0x274C, 0x274C,
    0x274E, 0x274E, 0x2753, 0x2755, 0x2757, 0x2757, 0x2763, 0x2764,
    0x2795, 0x2797, 0x27A1, 0x27A1, 0x27B0, 0x27B0, 0x27BF, 0x27BF,
    0x2934, 0x2935, 0x2B05, 0x2B07, 0x2B1B, 0x2B1C, 0x2B50, 0x2B50,
    0x2B55, 0x2B55
)

function Test-Emoji([int] $cp) {
    for ($i = 0; $i -lt $emojiRanges.Count; $i += 2) {
        if ($cp -ge $emojiRanges[$i] -and $cp -le $emojiRanges[$i + 1]) { return $true }
    }
    return $false
}

# Optional font-coverage probe. PresentationCore ships with the Windows Desktop
# runtime; if it is not loadable we simply skip the check rather than fail.
$glyphMap = $null
if ($Font) {
    try {
        Add-Type -AssemblyName PresentationCore -ErrorAction Stop
        $glyphMap = [Windows.Media.GlyphTypeface]::new([uri]$Font).CharacterToGlyphMap
        Write-Host "font coverage: $(Split-Path $Font -Leaf)" -ForegroundColor DarkGray
    } catch {
        Write-Warning "could not read $Font ($($_.Exception.Message)); skipping coverage"
    }
}

function Format-Flags([int] $cp) {
    $f = ''
    if (Test-Emoji $cp) { $f += '!' }
    if ($glyphMap -and -not $glyphMap.ContainsKey($cp)) { $f += '?' }
    if ($f -eq '') { $f = ' ' }
    return $f
}

if ($InSitu) {
    if (-not (Test-Path $Binary)) { throw "statusline binary not found: $Binary" }

    $payload = @{
        workspace      = @{ repo = @{ name = 'dev-tools' } }
        context_window = @{ total_input_tokens = 31450; used_percentage = 15.7 }
        rate_limits    = @{
            five_hour = @{ used_percentage = 26.4 }
            seven_day = @{ used_percentage = 7.1 }
        }
    } | ConvertTo-Json -Compress -Depth 6

    Write-Host "`nEach line is the real renderer. Pick the one that sits right.`n"
    foreach ($cp in $From..$To) {
        $flags = Format-Flags $cp
        if ($flags -match '\?') { continue }   # not in the font: nothing to judge
        $ch = [char]::ConvertFromUtf32($cp)
        $line = $payload | & $Binary --style min --columns 120 --icon $ch
        '{0} U+{1:X4}  {2}' -f $flags, $cp, $line | Write-Host
    }
    Write-Host "`n  ! = has the Unicode Emoji property; your terminal will render it"
    Write-Host "      from the colour-emoji font, not your monospace face.`n"
    Write-Host "Adopt one with:  statusline --style min --icon <glyph>"
    return
}

# Grid mode: 16 per row, codepoint-labelled, with a flag column per glyph.
$rowStart = $From - ($From % 16)
Write-Host ''
Write-Host ('        ' + ((0..15 | ForEach-Object { '{0:X}  ' -f $_ }) -join '')) -ForegroundColor DarkGray
for ($base = $rowStart; $base -le $To; $base += 16) {
    $cells = foreach ($i in 0..15) {
        $cp = $base + $i
        if ($cp -lt $From -or $cp -gt $To) { '   ' ; continue }
        $ch = [char]::ConvertFromUtf32($cp)
        '{0}{1} ' -f $ch, (Format-Flags $cp).Trim().PadRight(1)
    }
    # X3, not X4: the label is the row's leading nibbles, so 0x2700 must read
    # `270_`, not `0270_` — which parses as U+0270x.
    '{0:X3}_    {1}' -f ($base -shr 4), ($cells -join '') | Write-Host
}
Write-Host ''
Write-Host '  ! = Unicode Emoji property -> rendered from the colour-emoji font' -ForegroundColor DarkGray
if ($glyphMap) {
    Write-Host '  ? = absent from the probed font -> OS fallback' -ForegroundColor DarkGray
}
Write-Host ''
Write-Host '  Preview one in place:  statusline --style min --icon <glyph>' -ForegroundColor DarkGray
Write-Host '  Preview a whole range: ./preview-icons.ps1 -InSitu -From 0x2726 -To 0x273D' -ForegroundColor DarkGray
Write-Host ''
