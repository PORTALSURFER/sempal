Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

<#
.SYNOPSIS
One-change-per-worktree harness.

.DESCRIPTION
Creates a new git worktree, runs:
- scripts/bootstrap.ps1 --verify-only
- scripts/ci.ps1 local (unless -SkipCi)

Then (unless -NoRun) launches:
- scripts/run.ps1 sandbox (persistent by default)
  Use -RunTemp to run with -Temp (ephemeral; deleted on exit).

Defaults:
- base ref: HEAD
- path: <repo>\.worktrees\<id>\wavecrate
#>

param(
  [Parameter(Mandatory = $true)]
  [string]$Name,
  [string]$Base = "HEAD",
  [string]$Path,
  [switch]$SkipCi,
  [switch]$NoRun,
  [switch]$RunTemp
)

function Sanitize-BranchId([string]$Text) {
  $t = $Text.ToLowerInvariant().Replace(" ", "-")
  $t = [regex]::Replace($t, '[^a-z0-9._/-]+', '-')
  $t = [regex]::Replace($t, '-+', '-')
  $t = $t.Trim('-', '/', '\')
  return $t
}

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Push-Location $rootDir
try {
  if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    throw "[worktree_task] ERROR: git not found on PATH"
  }

  if ([string]::IsNullOrWhiteSpace($Path)) {
    $Path = Join-Path $rootDir (".worktrees\\" + $Name + "\\wavecrate")
  }
  $branchId = Sanitize-BranchId $Name
  if ([string]::IsNullOrWhiteSpace($branchId)) {
    throw "[worktree_task] ERROR: invalid -Name: $Name"
  }
  $branch = "task/$branchId"

  if (Test-Path -LiteralPath $Path) {
    throw "[worktree_task] ERROR: worktree path already exists: $Path"
  }
  Write-Host "[worktree_task] Creating worktree:"
  Write-Host ("[worktree_task]   branch={0}" -f $branch)
  Write-Host ("[worktree_task]   base={0}" -f $Base)
  Write-Host ("[worktree_task]   path={0}" -f $Path)
  git worktree add -b $branch $Path $Base
  Push-Location $Path
  try {
    Write-Host "[worktree_task] Running bootstrap verification..."
    & (Join-Path $Path "scripts/bootstrap.ps1") --verify-only

    if (-not $SkipCi) {
      Write-Host "[worktree_task] Running CI parity checks..."
      & (Join-Path $Path "scripts/ci.ps1") local
    } else {
      Write-Host "[worktree_task] Skipping CI parity checks (-SkipCi)."
    }

    Write-Host ("[worktree_task] Worktree ready: {0}" -f $Path)
    Write-Host ("[worktree_task] Tip: remove when done: git worktree remove ""{0}""" -f $Path)

    if ($NoRun) {
      Write-Host "[worktree_task] Not launching app (-NoRun)."
      exit 0
    }

    Write-Host "[worktree_task] Launching app in sandbox. Close the app to return."
    if ($RunTemp) {
      & (Join-Path $Path "scripts/run.ps1") sandbox -Temp @args
    } else {
      & (Join-Path $Path "scripts/run.ps1") sandbox @args
    }
  } finally {
    Pop-Location
  }
} finally {
  Pop-Location
}
