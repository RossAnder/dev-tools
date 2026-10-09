#Requires -Version 7
<#
.SYNOPSIS
  Links the repo's claude/ agents, commands, skills and themes into ~/.claude/.

.DESCRIPTION
  Idempotent. agents/, commands/ and themes/ get file symlinks (these need
  Developer Mode or an elevated shell); skills/*/ get junctions. A real file or
  directory in a link's place is moved to ~/.claude/backups/deploy-claude-<ts>/
  first. Links into this repo's claude/ whose source is gone are removed; other
  entries, such as skills/synced/, are never touched.
  -Check changes nothing, lists every discrepancy, and exits 1 if there is any.
#>
param(
  [switch]$Check,
  [string]$ClaudeHome = (Join-Path $HOME '.claude')
)

$ErrorActionPreference = 'Stop'
$Source = (Resolve-Path (Join-Path $PSScriptRoot '..\claude')).Path

# area -> how its entries are picked and linked
$Areas = @(
  @{ Name = 'agents';   Filter = '*.md';   Dirs = $false },
  @{ Name = 'commands'; Filter = '*.md';   Dirs = $false },
  @{ Name = 'skills';   Filter = '*';      Dirs = $true  },
  @{ Name = 'themes';   Filter = '*.json'; Dirs = $false }
)

$Problems = [System.Collections.Generic.List[string]]::new()
$Actions  = [System.Collections.Generic.List[string]]::new()
$BackupDir = Join-Path $ClaudeHome ('backups\deploy-claude-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))

function Get-LinkTarget($item) {
  if (-not $item.LinkType) { return $null }
  $t = @($item.Target)[0]
  if (-not [System.IO.Path]::IsPathRooted($t)) { $t = Join-Path $item.Directory.FullName $t }
  return [System.IO.Path]::GetFullPath($t)
}

function Backup-Item($item, $area) {
  $dest = Join-Path $BackupDir $area
  New-Item -ItemType Directory -Force $dest | Out-Null
  Move-Item -LiteralPath $item.FullName -Destination (Join-Path $dest $item.Name)
  $Actions.Add("backed up $area/$($item.Name) -> $dest")
}

function Remove-Link($item) {
  # Deleting the link itself; for a junction this never recurses into the target.
  if ($item.PSIsContainer) { [System.IO.Directory]::Delete($item.FullName, $false) }
  else { [System.IO.File]::Delete($item.FullName) }
}

function Assert-CanSymlink {
  $probe = Join-Path $ClaudeHome ".deploy-claude-probe-$PID"
  try { New-Item -ItemType SymbolicLink -Path $probe -Target $PSCommandPath -ErrorAction Stop | Out-Null }
  catch {
    [Console]::Error.WriteLine("deploy-claude: cannot create file symlinks ($($_.Exception.Message))")
    [Console]::Error.WriteLine('Enable Developer Mode (Settings > System > For developers) or rerun from an elevated pwsh. Nothing was changed.')
    exit 2
  }
  [System.IO.File]::Delete($probe)
}

if (-not $Check) { Assert-CanSymlink }

foreach ($area in $Areas) {
  $srcDir = Join-Path $Source $area.Name
  $dstDir = Join-Path $ClaudeHome $area.Name
  if (-not (Test-Path $srcDir)) { continue }
  if (-not (Test-Path $dstDir)) {
    if ($Check) { $Problems.Add("missing dir  $($area.Name)/") }
    else { New-Item -ItemType Directory -Force $dstDir | Out-Null }
  }

  $entries = if ($area.Dirs) { Get-ChildItem $srcDir -Directory } else { Get-ChildItem $srcDir -File -Filter $area.Filter }

  foreach ($e in $entries) {
    $rel = "$($area.Name)/$($e.Name)"
    $dst = Join-Path $dstDir $e.Name
    $existing = Get-Item -LiteralPath $dst -Force -ErrorAction SilentlyContinue

    if ($existing) {
      $target = Get-LinkTarget $existing
      if ($target -and $target -eq $e.FullName) { continue }
      if ($target) {
        if ($Check) { $Problems.Add("wrong target $rel -> $target"); continue }
        Remove-Link $existing
        $Actions.Add("relinked     $rel (was -> $target)")
      } else {
        if ($Check) { $Problems.Add("copy         $rel (not a link)"); continue }
        Backup-Item $existing $area.Name
      }
    } elseif ($Check) {
      $Problems.Add("missing      $rel"); continue
    }

    if ($area.Dirs) { New-Item -ItemType Junction -Path $dst -Target $e.FullName | Out-Null }
    else { New-Item -ItemType SymbolicLink -Path $dst -Target $e.FullName | Out-Null }
    $Actions.Add("linked       $rel")
  }

  # Links into this repo whose source has gone.
  if (Test-Path $dstDir) {
    foreach ($item in Get-ChildItem $dstDir -Force) {
      $target = Get-LinkTarget $item
      if (-not $target -or -not $target.StartsWith($Source + '\', [StringComparison]::OrdinalIgnoreCase)) { continue }
      if (Test-Path -LiteralPath $target) { continue }
      $rel = "$($area.Name)/$($item.Name)"
      if ($Check) { $Problems.Add("dangling     $rel -> $target"); continue }
      Remove-Link $item
      $Actions.Add("removed      $rel (dangling -> $target)")
    }
  }
}

if ($Check) {
  if ($Problems.Count -eq 0) { Write-Output "deploy-claude: ~/.claude is in sync with $Source"; exit 0 }
  Write-Output "deploy-claude: $($Problems.Count) entr$(if ($Problems.Count -eq 1) {'y'} else {'ies'}) out of sync:"
  $Problems | ForEach-Object { Write-Output "  $_" }
  Write-Output "Fix with: pwsh -NoProfile -File scripts/deploy-claude.ps1"
  exit 1
}

if ($Actions.Count -eq 0) { Write-Output "deploy-claude: nothing to do" }
else { $Actions | ForEach-Object { Write-Output "  $_" } }
