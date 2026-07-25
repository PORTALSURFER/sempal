<#
.SYNOPSIS
Dispatches development and CI validation lanes.

.DESCRIPTION
Collapses the old `devcheck`/`ci_*` script family into one public entrypoint.
#>

param(
  [Parameter(Position = 0)]
  [string]$Command,
  [switch]$Help,
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$Arguments
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$psExe = (Get-Process -Id $PID).Path

$commands = @{
  "smoke" = "devcheck.ps1"
  "agent" = "ci_agent.ps1"
  "isolation-stress" = "ci_isolation_stress.ps1"
  "quick" = "ci_quick.ps1"
  "local" = "ci_local.ps1"
}

if ([string]::IsNullOrWhiteSpace($Command)) {
  Write-Host "Usage: scripts/ci.ps1 <smoke|agent|quick|local|isolation-stress> [args...]"
  exit 0
}

if ($Help) {
  $Arguments = @("-Help") + $Arguments
}

$scriptName = $commands[$Command]
if ($null -eq $scriptName) {
  throw "Unknown CI command: $Command"
}

& $psExe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "internal/ci/$scriptName") @Arguments
