#Requires -Version 7
<#
.SYNOPSIS
  Links the repo's claude/ agents, commands, skills, themes and workflows into the
  Claude config dir (CLAUDE_CONFIG_DIR, default ~/.claude).

.DESCRIPTION
  Idempotent. agents/, commands/, themes/ and workflows/ get file symlinks (on Windows
  these need Developer Mode or an elevated shell); skills/*/ get junctions on Windows
  and directory symlinks elsewhere. A real file or directory in a link's place is
  moved to backups/deploy-claude-<ts>/ in the Claude config dir first. Links into this
  repo's claude/ whose source is gone are removed; other entries, such as
  skills/synced/, are never touched.
  -Check changes nothing, lists every discrepancy, and exits 1 if there is any.
#>
param(
  [switch]$Check,
  [string]$ClaudeHome = $(if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } else { Join-Path $HOME '.claude' })
)

$ErrorActionPreference = 'Stop'
$Source = (Resolve-Path (Join-Path $PSScriptRoot '..' 'claude')).Path
$DirLinkType = if ($IsWindows) { 'Junction' } else { 'SymbolicLink' }
$PathComparison = if ($IsWindows) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }

# area -> how its entries are picked and linked
$Areas = @(
  @{ Name = 'agents';   Filter = '*.md';   Dirs = $false },
  @{ Name = 'commands'; Filter = '*.md';   Dirs = $false },
  @{ Name = 'skills';   Filter = '*';      Dirs = $true  },
  @{ Name = 'themes';   Filter = '*.json'; Dirs = $false },
  @{ Name = 'workflows'; Filter = '*.js';  Dirs = $false }
)

$Problems = [System.Collections.Generic.List[string]]::new()
$Actions  = [System.Collections.Generic.List[string]]::new()
$BackupDir = Join-Path $ClaudeHome 'backups' ('deploy-claude-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))

function Get-LinkTarget($item) {
  if (-not $item.LinkType) { return $null }
  $t = @($item.Target)[0]
  # A hard link reports a LinkType but has no target: it is a real file, so it gets backed up, not relinked.
  if (-not $t) { return $null }
  if (-not [System.IO.Path]::IsPathRooted($t)) { $t = Join-Path ([System.IO.Path]::GetDirectoryName($item.FullName)) $t }
  return [System.IO.Path]::GetFullPath($t)
}

function Backup-Item($item, $area) {
  $dest = Join-Path $BackupDir $area
  New-Item -ItemType Directory -Force $dest | Out-Null
  Move-Item -LiteralPath $item.FullName -Destination (Join-Path $dest $item.Name)
  $Actions.Add("backed up $area/$($item.Name) -> $dest")
}

function Remove-Link($item) {
  # Deleting the link itself; for a junction or a directory symlink this never recurses into the target.
  if ($item.PSIsContainer) { [System.IO.Directory]::Delete($item.FullName, $false) }
  else { [System.IO.File]::Delete($item.FullName) }
}

function Assert-CanSymlink {
  if (-not (Test-Path -LiteralPath $ClaudeHome)) { New-Item -ItemType Directory -Force $ClaudeHome | Out-Null }
  $probe = Join-Path $ClaudeHome ".deploy-claude-probe-$PID"
  try { New-Item -ItemType SymbolicLink -Path $probe -Target $PSCommandPath -ErrorAction Stop | Out-Null }
  catch {
    [Console]::Error.WriteLine("deploy-claude: cannot create file symlinks ($($_.Exception.Message))")
    if ($IsWindows) { [Console]::Error.WriteLine('Enable Developer Mode (Settings > System > For developers) or rerun from an elevated pwsh.') }
    [Console]::Error.WriteLine('No links were changed.')
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
      if ($target -and [string]::Equals($target, $e.FullName, $PathComparison)) { continue }
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

    if ($area.Dirs) { New-Item -ItemType $DirLinkType -Path $dst -Target $e.FullName | Out-Null }
    else { New-Item -ItemType SymbolicLink -Path $dst -Target $e.FullName | Out-Null }
    $Actions.Add("linked       $rel")
  }

  # Links into this repo whose source has gone.
  if (Test-Path $dstDir) {
    foreach ($item in Get-ChildItem $dstDir -Force) {
      $target = Get-LinkTarget $item
      if (-not $target -or -not $target.StartsWith($Source + [System.IO.Path]::DirectorySeparatorChar, $PathComparison)) { continue }
      if (Test-Path -LiteralPath $target) { continue }
      $rel = "$($area.Name)/$($item.Name)"
      if ($Check) { $Problems.Add("dangling     $rel -> $target"); continue }
      Remove-Link $item
      $Actions.Add("removed      $rel (dangling -> $target)")
    }
  }
}

if ($Check) {
  if ($Problems.Count -eq 0) { Write-Output "deploy-claude: $ClaudeHome is in sync with $Source"; exit 0 }
  Write-Output "deploy-claude: $($Problems.Count) entr$(if ($Problems.Count -eq 1) {'y'} else {'ies'}) out of sync:"
  $Problems | ForEach-Object { Write-Output "  $_" }
  Write-Output "Fix with: pwsh -NoProfile -File scripts/deploy-claude.ps1"
  exit 1
}

if ($Actions.Count -eq 0) { Write-Output "deploy-claude: nothing to do" }
else { $Actions | ForEach-Object { Write-Output "  $_" } }
