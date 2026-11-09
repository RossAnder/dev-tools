# Links the review-v2 commands, agents, skills and Workflows into the Claude config dir
# ($env:CLAUDE_CONFIG_DIR, else ~/.claude). Idempotent: an entry already linked to its
# source is skipped. Nothing under the config dir is ever deleted or overwritten: a real
# file or directory, or a link pointing elsewhere, in an entry's place is reported and
# left alone. Skills are junctions; every other entry is a file symbolic link, which
# Windows creates only from an elevated shell or with Developer Mode on. Without either,
# the junctions are made and each file link is listed as pending.
#
# Exit: 0 all linked; 2 only file links pending elevation; 1 any refusal or failure.

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$config = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } else { Join-Path $HOME '.claude' }

$entries = @(
    @{ Kind = 'commands'; Name = 'review-v2.md' }
    @{ Kind = 'commands'; Name = 'optimise-v2.md' }
    @{ Kind = 'commands'; Name = 'review-plan-v2.md' }
    @{ Kind = 'agents'; Name = 'review-surveyor.md' }
    @{ Kind = 'agents'; Name = 'review-sweeper.md' }
    @{ Kind = 'agents'; Name = 'review-checker.md' }
    @{ Kind = 'agents'; Name = 'review-finder.md' }
    @{ Kind = 'agents'; Name = 'review-finder-cf.md' }
    @{ Kind = 'agents'; Name = 'review-verifier.md' }
    @{ Kind = 'agents'; Name = 'plan-executor.md' }
    @{ Kind = 'agents'; Name = 'plan-review-lead.md' }
    @{ Kind = 'skills'; Name = 'review-v2-engine' }
    @{ Kind = 'skills'; Name = 'review-v2-method' }
    @{ Kind = 'skills'; Name = 'review-v2-lenses' }
    @{ Kind = 'workflows'; Name = 'review-v2.js' }
    @{ Kind = 'workflows'; Name = 'plan-v2-review.js' }
)

function Get-FullPath([string]$path) {
    $p = $path
    if ($p.StartsWith('\\?\')) { $p = $p.Substring(4) }
    [System.IO.Path]::GetFullPath($p).TrimEnd('\')
}

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
$elevated = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$devMode = $false
try {
    $devMode = (Get-ItemPropertyValue -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock' -Name 'AllowDevelopmentWithoutDevLicense') -eq 1
} catch { }
$canFileLink = $elevated -or $devMode

Write-Output "config dir: $config"
Write-Output "elevated: $elevated; developer mode: $devMode"

$made = 0; $skipped = 0; $pending = 0; $refused = 0

foreach ($e in $entries) {
    $isDir = $e.Kind -eq 'skills'
    $source = Join-Path (Join-Path (Join-Path $repo 'claude') $e.Kind) $e.Name
    $dest = Join-Path (Join-Path $config $e.Kind) $e.Name
    $label = "$($e.Kind)/$($e.Name)"

    $sourceType = if ($isDir) { 'Container' } else { 'Leaf' }
    if (-not (Test-Path -LiteralPath $source -PathType $sourceType)) {
        Write-Output "refused  $label  source missing: $source"
        $refused++
        continue
    }

    # -Force lists a dangling link too, so a broken entry is reported rather than retried.
    $existing = Get-Item -LiteralPath $dest -Force -ErrorAction SilentlyContinue
    if ($existing) {
        if ($existing.LinkType) {
            $target = @($existing.Target)[0]
            if ($target -and ((Get-FullPath $target) -ieq (Get-FullPath $source))) {
                Write-Output "skipped  $label  already $($existing.LinkType) -> $target"
                $skipped++
            } else {
                Write-Output "refused  $label  $($existing.LinkType) points elsewhere: $target"
                $refused++
            }
        } else {
            Write-Output "refused  $label  a real $(if ($existing.PSIsContainer) { 'directory' } else { 'file' }) is in its place"
            $refused++
        }
        continue
    }

    if (-not $isDir -and -not $canFileLink) {
        Write-Output "pending  $label  file symbolic link needs an elevated shell: $dest -> $source"
        $pending++
        continue
    }

    $parent = Split-Path -Parent $dest
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        New-Item -ItemType Directory -Path $parent | Out-Null
    }

    # New-Item without -Force fails rather than replace anything that appeared since the check.
    $type = if ($isDir) { 'Junction' } else { 'SymbolicLink' }
    try {
        New-Item -ItemType $type -Path $dest -Target $source | Out-Null
        Write-Output "made     $label  $type -> $source"
        $made++
    } catch {
        Write-Output "refused  $label  $type failed: $($_.Exception.Message)"
        $refused++
    }
}

Write-Output "made $made, skipped $skipped, pending $pending, refused $refused"

if ($refused -gt 0) { exit 1 }
if ($pending -gt 0) { exit 2 }
exit 0
