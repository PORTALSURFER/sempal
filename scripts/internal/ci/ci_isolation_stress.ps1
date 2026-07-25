<#
.SYNOPSIS
Runs the opt-in parallel test-isolation stress lane.

.DESCRIPTION
Builds the Wavecrate library test harness once, verifies injected process-state
and mutable-global-control leak sentinels, then runs bounded repetitions in
fresh explicitly parallel test processes. Results are emitted as JSONL.
#>

param(
  [ValidateRange(1, 100)]
  [int]$Iterations = 5,
  [ValidateRange(2, 256)]
  [int]$TestThreads = [Math]::Min(8, [Math]::Max(2, [Environment]::ProcessorCount)),
  [ValidateRange(1, 3600)]
  [int]$TimeoutSeconds = 900,
  [string]$Output = "",
  [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path

if ($Help) {
  Write-Host "Usage: scripts/ci.ps1 isolation-stress [-Iterations N] [-TestThreads N] [-TimeoutSeconds N] [-Output PATH]"
  Write-Host ""
  Write-Host "Runs bounded fresh-process repetitions of the Wavecrate library suite."
  Write-Host "The lane verifies injected leak sentinels and emits one JSONL record per process."
  exit 0
}

$python = $null
$pythonPrefix = @()
foreach ($candidate in @("python3", "py", "python")) {
  $resolved = Get-Command $candidate -ErrorAction SilentlyContinue
  if ($null -ne $resolved) {
    $python = $resolved.Path
    if ($candidate -eq "py") {
      $pythonPrefix = @("-3")
    }
    break
  }
}
if ($null -eq $python) {
  throw "[parallel_isolation] Python 3 was not found."
}

$arguments = @(
  (Join-Path $PSScriptRoot "parallel_isolation_stress.py"),
  "--iterations",
  $Iterations.ToString(),
  "--test-threads",
  $TestThreads.ToString(),
  "--timeout-seconds",
  $TimeoutSeconds.ToString()
)
if (-not [string]::IsNullOrWhiteSpace($Output)) {
  $arguments += @("--output", $Output)
}

Push-Location $rootDir
try {
  & $python @pythonPrefix @arguments
  if ($LASTEXITCODE -ne 0) {
    throw "[parallel_isolation] Lane failed with exit code $LASTEXITCODE."
  }
} finally {
  Pop-Location
}
