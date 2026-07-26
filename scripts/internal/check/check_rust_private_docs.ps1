
<#
.SYNOPSIS
Diff-aware guard for doc comments on newly added Rust items.

.DESCRIPTION
Fails when added Rust items introduce missing doc comments in `src/` and
the standalone Radiant sibling.
#>

param(
  [string]$Base = "",
  [string]$Head = "HEAD",
  [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"


if ($Help) {
  Write-Host "Usage: scripts/internal/check/check_rust_private_docs.ps1 [-Base <ref>] [-Head <ref>]"
  Write-Host ""
  Write-Host "Allowlist file: scripts/internal/check/allowlists/rust_private_docs_allowlist.txt"
  exit 0
}

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
$allowlistPath = Join-Path $rootDir "scripts/internal/check/allowlists/rust_private_docs_allowlist.txt"
$itemRegex = '^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+|unsafe\s+|extern\s+"[^"]+"\s+)*\b(fn|struct|enum|trait|type|const|static|mod)\b'
$hunkRegex = '\+(\d+)(?:,(\d+))?'

Push-Location $rootDir
try {
  $allowlist = New-Object "System.Collections.Generic.HashSet[string]"
  if (Test-Path -LiteralPath $allowlistPath) {
    foreach ($line in Get-Content -LiteralPath $allowlistPath) {
      $trimmed = $line.Trim()
      if ([string]::IsNullOrWhiteSpace($trimmed)) { continue }
      if ($trimmed.StartsWith("#")) { continue }
      [void]$allowlist.Add($trimmed)
    }
  }

  function Test-GitCommit([string]$Ref) {
    if ([string]::IsNullOrWhiteSpace($Ref)) { return $false }
    git rev-parse --verify --quiet "$Ref^{commit}" | Out-Null
    return ($LASTEXITCODE -eq 0)
  }

  function Should-CheckFile([string]$Path) {
    if (-not $Path.EndsWith(".rs")) { return $false }
    if ($Path.StartsWith("src/")) { return $true }
    return $false
  }

  function Is-CandidateItemLine([string]$LineText) {
    $trimmed = $LineText.Trim()
    if ([string]::IsNullOrWhiteSpace($trimmed)) { return $false }
    if ($trimmed.StartsWith("//")) { return $false }
    if ($trimmed.StartsWith("use ") -or $trimmed.StartsWith("pub use ")) { return $false }
    return ($LineText -match $itemRegex)
  }

  function Get-FileLinesForSource([string]$Source, [string]$Path, [string]$HeadRef) {
    if ($Source -eq "worktree") {
      if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return @() }
      return (Get-Content -LiteralPath $Path)
    }

    $spec = ""
    if ($Source -eq "index") {
      $spec = ":$Path"
    } elseif ($Source -eq "commit") {
      if ([string]::IsNullOrWhiteSpace($HeadRef)) { return @() }
      $spec = "${HeadRef}:$Path"
    } else {
      throw "Unknown source '$Source'."
    }

    $content = git show $spec 2>$null
    if ($LASTEXITCODE -ne 0) { return @() }
    return ((($content | Out-String) -replace "`r", "") -split "`n")
  }

  function Has-DocComment([string[]]$Lines, [int]$ItemLine1) {
    $itemIndex = $ItemLine1 - 1
    if ($itemIndex -le 0 -or $itemIndex -gt $Lines.Length) { return $false }

    $start = [Math]::Max(0, $itemIndex - 12)
    for ($i = $itemIndex - 1; $i -ge $start; $i--) {
      $trimmed = $Lines[$i].Trim()
      if ([string]::IsNullOrWhiteSpace($trimmed)) { continue }

      if ($trimmed.StartsWith("///")) { return $true }
      if ($trimmed.StartsWith("/**") -or $trimmed.StartsWith("/*!")) { return $true }
      if ($trimmed -match '^\s*#\!?\[doc\s*(=|\()') { return $true }
      if ($trimmed.StartsWith("#[") -or $trimmed.StartsWith("#![")) { continue }

      return $false
    }
    return $false
  }

  function Scan-Diff([string]$Label, [string[]]$DiffArgs, [string]$Source, [string]$HeadRef) {
    $diffLines = git diff --unified=0 --diff-filter=AMR @DiffArgs -- src
    if ($LASTEXITCODE -ne 0) {
      $diffLines = @()
    }

    $currentFile = ""
    $newLine = 0
    $violations = New-Object System.Collections.Generic.List[string]
    $fileCache = @{}

    foreach ($line in $diffLines) {
      if ($line.StartsWith("+++ b/")) {
        $currentFile = $line.Substring(6)
        $newLine = 0
        continue
      }

      if ($line.StartsWith("@@")) {
        if ($line -match $hunkRegex) {
          $newLine = [int]$Matches[1]
        } else {
          $newLine = 0
        }
        continue
      }

      if (-not $line.StartsWith("+") -or $line.StartsWith("+++")) { continue }
      if ([string]::IsNullOrWhiteSpace($currentFile)) { continue }
      if (-not (Should-CheckFile $currentFile)) { $newLine++; continue }
      if ($allowlist.Contains($currentFile)) { $newLine++; continue }

      $addedText = $line.Substring(1)
      if (-not (Is-CandidateItemLine $addedText)) { $newLine++; continue }

      if (-not $fileCache.ContainsKey($currentFile)) {
        $fileCache[$currentFile] = Get-FileLinesForSource -Source $Source -Path $currentFile -HeadRef $HeadRef
      }

      $fileLines = [string[]]$fileCache[$currentFile]
      if ($fileLines.Length -eq 0) {
        $violations.Add(("{0}:{1}: missing file content for doc check" -f $currentFile, $newLine))
        $newLine++
        continue
      }

      if (-not (Has-DocComment -Lines $fileLines -ItemLine1 $newLine)) {
        $violations.Add(("{0}:{1}: {2}" -f $currentFile, $newLine, $addedText.Trim()))
      }
      $newLine++
    }

    if ($violations.Count -eq 0) { return $true }

    Write-Host ("[private_docs] Violations detected ({0}):" -f $Label)
    Write-Host "[private_docs] Newly added Rust items must have doc comments (`///` or `#[doc = ...]`)."
    Write-Host ("[private_docs] Allowlist (last resort): {0}" -f $allowlistPath)
    foreach ($violation in ($violations | Sort-Object)) {
      Write-Host (" - {0}" -f $violation)
    }
    return $false
  }

  $allOk = $true
  if (-not [string]::IsNullOrWhiteSpace($Base) -and (Test-GitCommit $Base) -and (Test-GitCommit $Head)) {
    $allOk = $allOk -and (Scan-Diff -Label ("range {0}...{1}" -f $Base, $Head) -DiffArgs @("$Base...$Head") -Source "commit" -HeadRef $Head)
  }
  $allOk = $allOk -and (Scan-Diff -Label "staged" -DiffArgs @("--cached") -Source "index" -HeadRef "")
  $allOk = $allOk -and (Scan-Diff -Label "unstaged" -DiffArgs @() -Source "worktree" -HeadRef "")

  if (-not $allOk) { exit 1 }
  Write-Host "[private_docs] OK"
  exit 0
} finally {
  Pop-Location
}
