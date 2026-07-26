<#
.SYNOPSIS
Reports large script and Radiant GUI files without failing validation.

.DESCRIPTION
This report-only guardrail surfaces maintainability hotspots outside the
blocking Rust file-size budget. It scans script internals and high-risk Radiant
GUI/API modules, then prints the largest files and files over the configured
line limit.
#>

param(
  [int]$Limit = 400,
  [int]$TopFiles = 20,
  [Alias("h")]
  [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Usage {
  Write-Host "Usage: scripts/internal/check/report_size_hotspots.ps1 [-Limit <n>] [-TopFiles <n>]"
  Write-Host ""
  Write-Host "Prints a report-only line-count snapshot for scripts/internal and high-risk Radiant GUI modules."
}

if ($Help) {
  Write-Usage
  exit 0
}

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path

function Convert-ToRepoPath {
  param([string]$Path)
  return $Path.Trim().Replace("\", "/")
}

function Get-RootTrackedFiles {
  param([string[]]$Paths)
  $files = New-Object "System.Collections.Generic.HashSet[string]"
  foreach ($file in @(git ls-files -- $Paths)) {
    if ($LASTEXITCODE -ne 0) {
      throw "[size_hotspots] failed to enumerate tracked files"
    }
    $path = Convert-ToRepoPath $file
    if ([string]::IsNullOrWhiteSpace($path)) { continue }
    [void]$files.Add($path)
  }
  return @($files)
}

function Get-Scope {
  param([string]$File)
  if ($File.StartsWith("scripts/internal/")) { return "scripts/internal" }
  return "other"
}

Push-Location $rootDir
try {
  $scriptExtensions = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
  foreach ($extension in @(".ps1", ".sh", ".py", ".json", ".cmd")) {
    [void]$scriptExtensions.Add($extension)
  }

  $candidateFiles = New-Object "System.Collections.Generic.HashSet[string]"
  foreach ($file in Get-RootTrackedFiles -Paths @("scripts/internal")) {
    $extension = [System.IO.Path]::GetExtension($file)
    if ($scriptExtensions.Contains($extension)) {
      [void]$candidateFiles.Add($file)
    }
  }

  $entries = foreach ($file in $candidateFiles) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { continue }
    [pscustomobject]@{
      Lines = ([System.IO.File]::ReadAllLines((Resolve-Path -LiteralPath $file))).Count
      Scope = Get-Scope -File $file
      File = $file
    }
  }

  $sorted = @($entries | Sort-Object Lines, File -Descending)
  $over = @($sorted | Where-Object { $_.Lines -gt $Limit })
  $timestampUtc = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")

  Write-Host "# Size Hotspot Report"
  Write-Host ""
  Write-Host ('- Timestamp (UTC): `{0}`' -f $timestampUtc)
  Write-Host ('- Limit: `{0}` lines' -f $Limit)
  Write-Host '- Scopes: `scripts/internal`'
  Write-Host ("- Entries: total={0} over={1}" -f $sorted.Count, $over.Count)
  Write-Host ""

  if ($over.Count -gt 0) {
    Write-Host "## Over Budget"
    Write-Host ""
    Write-Host "| Lines | Scope | File |"
    Write-Host "| ---: | --- | --- |"
    foreach ($entry in ($over | Select-Object -First $TopFiles)) {
      Write-Host ('| {0} | {1} | `{2}` |' -f $entry.Lines, $entry.Scope, $entry.File)
    }
    Write-Host ""
  } else {
    Write-Host "## Over Budget"
    Write-Host ""
    Write-Host "None."
    Write-Host ""
  }

  Write-Host "## Largest Files"
  Write-Host ""
  Write-Host "| Lines | Scope | File |"
  Write-Host "| ---: | --- | --- |"
  foreach ($entry in ($sorted | Select-Object -First $TopFiles)) {
    Write-Host ('| {0} | {1} | `{2}` |' -f $entry.Lines, $entry.Scope, $entry.File)
  }

  exit 0
} finally {
  Pop-Location
}
