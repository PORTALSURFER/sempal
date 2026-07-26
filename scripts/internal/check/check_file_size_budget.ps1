
<#
.SYNOPSIS
Enforces a per-file line budget for Rust sources.

.DESCRIPTION
Checks Rust files under `src/` and `tests/` and fails if
any non-allowlisted file exceeds the line budget.

By default, checks files added/modified in the supplied git diff range (if any),
plus staged/unstaged working tree edits. Known legacy exceptions live in
`scripts/internal/check/allowlists/file_size_budget_allowlist.txt`.
#>

param(
  [string]$Base,
  [string]$Head = "HEAD",
  [int]$Limit = 400,
  [switch]$All
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"


$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Push-Location $rootDir
try {
  $allowlistPath = Join-Path $rootDir "scripts/internal/check/allowlists/file_size_budget_allowlist.txt"
  $allowlist = New-Object "System.Collections.Generic.HashSet[string]"
  if (Test-Path -LiteralPath $allowlistPath) {
    foreach ($line in Get-Content -LiteralPath $allowlistPath) {
      if ([string]::IsNullOrWhiteSpace($line)) { continue }
      if ($line.TrimStart().StartsWith("#")) { continue }
      [void]$allowlist.Add($line.Trim())
    }
  }

  $projectScopePaths = @("src", "tests")
  $files = New-Object "System.Collections.Generic.HashSet[string]"

  function Test-GitCommit([string]$Ref) {
    if ([string]::IsNullOrWhiteSpace($Ref)) { return $false }
    git rev-parse --verify --quiet "$Ref^{commit}" | Out-Null
    return ($LASTEXITCODE -eq 0)
  }


  function Add-GitFileList([string[]]$Lines, [string]$Prefix = "") {
    foreach ($line in $Lines) {
      $path = $line.Trim()
      if ([string]::IsNullOrWhiteSpace($path)) { continue }
      $path = $path.Replace("\", "/")
      if (-not [string]::IsNullOrWhiteSpace($Prefix)) {
        $path = ($Prefix.TrimEnd("/", "\") + "/" + $path.TrimStart("/", "\")).Replace("\", "/")
      }
      if (-not $path.EndsWith(".rs")) { continue }
      [void]$files.Add($path)
    }
  }

  function Get-PhysicalLineCount([string]$RelativePath) {
    $resolvedPath = (Resolve-Path -LiteralPath $RelativePath).Path
    return ([System.IO.File]::ReadAllLines($resolvedPath)).Count
  }

  if ($All) {
    Add-GitFileList (git ls-files -- $projectScopePaths)
  } else {
    if ([string]::IsNullOrWhiteSpace($Base)) {
      if (Test-GitCommit "origin/main") { $Base = "origin/main" }
      elseif (Test-GitCommit "main") { $Base = "main" }
    }

    if (-not [string]::IsNullOrWhiteSpace($Base) -and (Test-GitCommit $Base) -and (Test-GitCommit $Head)) {
      Add-GitFileList (git diff --name-only --diff-filter=AM "$Base...$Head" -- $projectScopePaths)
    } elseif (Test-GitCommit $Head) {
      Add-GitFileList (git show --name-only --pretty=format: $Head -- $projectScopePaths)
    }

    Add-GitFileList (git diff --name-only --diff-filter=AM --cached -- $projectScopePaths)
    Add-GitFileList (git diff --name-only --diff-filter=AM -- $projectScopePaths)

  }

  if ($files.Count -eq 0) {
    Write-Host "[file_budget] No changed Rust files detected."
    exit 0
  }

  $violations = @()
  $checked = 0
  foreach ($file in $files) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { continue }
    $checked++

    if ($allowlist.Contains($file)) { continue }

    $lineCount = Get-PhysicalLineCount -RelativePath $file
    if ($lineCount -gt $Limit) {
      $violations += ("{0}: {1}" -f $file, $lineCount)
    }
  }

  if ($checked -eq 0) {
    Write-Host "[file_budget] No matching Rust files found to check."
    exit 0
  }

  if ($violations.Count -gt 0) {
    Write-Host ("[file_budget] File size budget violations (limit: {0} lines):" -f $Limit)
    foreach ($v in ($violations | Sort-Object)) {
      Write-Host (" - {0}" -f $v)
    }
    Write-Host ("[file_budget] Fix by splitting files into focused modules, or (temporarily) add to allowlist: {0}" -f $allowlistPath)
    exit 1
  }

  Write-Host ("[file_budget] OK ({0} files checked)" -f $checked)
  exit 0
} finally {
  Pop-Location
}
