<#
.SYNOPSIS
Fast-forwards both development repos and runs Wavecrate in release sandbox mode.

.DESCRIPTION
Verifies that the main repo is on local `main` tracking `origin/main`,
requires the Wavecrate worktree to be clean, then delegates to
`scripts/run.ps1 sandbox`.

This script accepts the same sandbox options as `scripts/run.ps1 sandbox`. Any
trailing app arguments are forwarded to the release run after a `--` separator.
#>

param(
  [string]$Dir,
  [string]$Name,
  [switch]$Temp,
  [switch]$Clean,
  [switch]$WriteDb,
  [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($Help) {
  Write-Host "Usage: scripts/internal/release/pull_and_run_release.ps1 [-Dir <path> | -Name <name> | -Temp] [-Clean] [-WriteDb] [-- <app args...>]"
  Write-Host "Fast-forward Wavecrate, then run scripts/run.ps1 sandbox."
  exit 0
}

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
$runSandboxScript = Join-Path $rootDir "scripts/run.ps1"

function Invoke-GitCommand {
  param(
    [Parameter(Mandatory = $true)]
    [string]$RepoPath,
    [Parameter(Mandatory = $true)]
    [string[]]$GitArgs
  )

  $result = & git -C $RepoPath @GitArgs
  if ($LASTEXITCODE -ne 0) {
    $joinedArgs = $GitArgs -join " "
    throw "[pull_and_run_release] git $joinedArgs failed for $RepoPath with exit code $LASTEXITCODE."
  }
  return $result
}

function Assert-RepoExists {
  param(
    [Parameter(Mandatory = $true)]
    [string]$RepoPath,
    [Parameter(Mandatory = $true)]
    [string]$Label
  )

  if (-not (Test-Path -LiteralPath $RepoPath)) {
    throw "[pull_and_run_release] Missing $Label repo at $RepoPath."
  }
}

function Assert-CleanRepo {
  param(
    [Parameter(Mandatory = $true)]
    [string]$RepoPath,
    [Parameter(Mandatory = $true)]
    [string]$Label
  )

  $statusLines = Invoke-GitCommand -RepoPath $RepoPath -GitArgs @("status", "--short")
  if ($statusLines.Count -gt 0) {
    $details = ($statusLines | ForEach-Object { "  $_" }) -join [Environment]::NewLine
    throw "[pull_and_run_release] $Label repo is not clean:`n$details"
  }
}

function Assert-TrackedRepo {
  param(
    [Parameter(Mandatory = $true)]
    [string]$RepoPath,
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedBranch,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedUpstream
  )

  $branch = (Invoke-GitCommand -RepoPath $RepoPath -GitArgs @("rev-parse", "--abbrev-ref", "HEAD")).Trim()
  if ($branch -ne $ExpectedBranch) {
    throw "[pull_and_run_release] $Label repo must be on local $ExpectedBranch, found '$branch'."
  }

  $upstream = (Invoke-GitCommand -RepoPath $RepoPath -GitArgs @("rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}")).Trim()
  if ($upstream -ne $ExpectedUpstream) {
    throw "[pull_and_run_release] $Label repo must track $ExpectedUpstream, found '$upstream'."
  }
}

function Sync-Repo {
  param(
    [Parameter(Mandatory = $true)]
    [string]$RepoPath,
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedBranch,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedUpstream
  )

  Assert-RepoExists -RepoPath $RepoPath -Label $Label
  Assert-CleanRepo -RepoPath $RepoPath -Label $Label
  Assert-TrackedRepo -RepoPath $RepoPath -Label $Label -ExpectedBranch $ExpectedBranch -ExpectedUpstream $ExpectedUpstream

  Write-Host "[pull_and_run_release] Pulling $Label repo from $ExpectedUpstream"
  Invoke-GitCommand -RepoPath $RepoPath -GitArgs @("pull", "--ff-only", "origin", $ExpectedBranch) | Out-Null
}

$runSandboxArgs = @()
if (-not [string]::IsNullOrWhiteSpace($Dir)) {
  $runSandboxArgs += @("-Dir", $Dir)
}
if (-not [string]::IsNullOrWhiteSpace($Name)) {
  $runSandboxArgs += @("-Name", $Name)
}
if ($Temp) {
  $runSandboxArgs += "-Temp"
}
if ($Clean) {
  $runSandboxArgs += "-Clean"
}
if ($WriteDb) {
  $runSandboxArgs += "-WriteDb"
}

Sync-Repo -RepoPath $rootDir -Label "main" -ExpectedBranch "main" -ExpectedUpstream "origin/main"
Write-Host "[pull_and_run_release] Starting release sandbox run"
if ($args.Count -gt 0) {
  & $runSandboxScript sandbox @runSandboxArgs -- @args
} else {
  & $runSandboxScript sandbox @runSandboxArgs
}

exit $LASTEXITCODE
