
<#
.SYNOPSIS
Diff-aware check for doc comments on newly added `pub` Rust items.

.DESCRIPTION
Fails when added lines introduce public Rust items (fn/struct/enum/trait/type/const/static)
without nearby doc comments (`///` or `#[doc = ...]`).

The check is diff-aware (only added lines), but doc-comment presence is validated
against the "b-side" content:
- range diffs use `git show <head>:<path>`
- staged diffs use `git show :<path>` (index)
- unstaged diffs read the working tree

Scope:
- Rust files under `src/`
- Skips test/bench paths and allowlisted files
#>

param(
  [string]$Base,
  [string]$Head = "HEAD"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"


$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Push-Location $rootDir
try {
  $allowlistPath = Join-Path $rootDir "scripts/internal/check/allowlists/rust_public_docs_allowlist.txt"
  $allowlist = New-Object "System.Collections.Generic.HashSet[string]"
  if (Test-Path -LiteralPath $allowlistPath) {
    foreach ($line in Get-Content -LiteralPath $allowlistPath) {
      $t = $line.Trim()
      if ([string]::IsNullOrWhiteSpace($t)) { continue }
      if ($t.StartsWith("#")) { continue }
      [void]$allowlist.Add($t)
    }
  }

  function Test-GitCommit([string]$Ref) {
    if ([string]::IsNullOrWhiteSpace($Ref)) { return $false }
    try {
      git rev-parse --verify --quiet "$Ref^{commit}" | Out-Null
      return $true
    } catch {
      return $false
    }
  }

  function Should-CheckFile([string]$Path) {
    return (($Path -like "src/*.rs") -or ($Path -like "src/*/*.rs") -or ($Path -like "src/*/*/*.rs") -or ($Path -like "src/*/*/*/*.rs") -or ($Path -like "src/*/*/*/*/*.rs") -or ($Path -like "src/*/*/*/*/*/*.rs") -or ($Path -like "src/*/*/*/*/*/*/*.rs") -or ($Path -like "src/*/*/*/*/*/*/*/*.rs"))
  }

  function Is-TestishPath([string]$Path) {
    if ($Path -like "tests/*") { return $true }
    if ($Path -like "*\\tests\\*") { return $true }
    if ($Path -like "*/tests/*") { return $true }
    if ($Path -like "benches/*") { return $true }
    if ($Path -like "*/benches/*") { return $true }
    if ($Path -like "*_test.rs") { return $true }
    if ($Path -like "*_tests.rs") { return $true }
    return $false
  }

  function Is-PublicItemLine([string]$Text) {
    # `pub` but not `pub(crate)` / `pub(super)` / `pub(in ...)`
    if ($Text -notmatch '^\s*pub\s+(?!\()') { return $false }
    if ($Text -match '^\s*pub\s+use\b') { return $false }
    return ($Text -match '^\s*pub\s+(?:async\s+|unsafe\s+|extern\s+"[^"]+"\s+)*\b(fn|struct|enum|trait|type|const|static)\b')
  }

  function Has-DocComment([string[]]$Lines, [int]$LineNumber1) {
    # 1-based line number of the item line.
    $idx = $LineNumber1 - 1
    $start = [Math]::Max(0, $idx - 12)
    for ($i = $idx - 1; $i -ge $start; $i--) {
      $s = $Lines[$i].Trim()
      if ([string]::IsNullOrWhiteSpace($s)) { continue }

      if ($s.StartsWith("///")) { return $true }
      if ($s.StartsWith("/**") -or $s.StartsWith("/*!")) { return $true }
      if ($s -match '^\s*#\!\?\[doc\s*(=|\()') { return $true }

      if ($s.StartsWith("#[") -or $s.StartsWith("#![")) { continue }

      # Any other comment / code line is a barrier (docs should be adjacent-ish).
      return $false
    }
    return $false
  }

  function Get-FileLinesForSource([string]$Source, [string]$Path, [string]$HeadRef) {
    if ($Source -eq "worktree") {
      return (Get-Content -LiteralPath $Path)
    }
    if ($Source -eq "index") {
      $spec = ":$Path"
      $content = (git show $spec | Out-String)
      return (($content -replace "`r", "") -split "`n")
    }
    if ($Source -eq "commit") {
      $spec = "${HeadRef}:$Path"
      $content = (git show $spec | Out-String)
      return (($content -replace "`r", "") -split "`n")
    }
    throw "Unknown source: $Source"
  }

  function Scan-Diff([string]$Label, [string[]]$GitArgs, [string]$Source, [string]$HeadRef) {
    $lines = @()
    try {
      $lines = git diff --unified=0 --diff-filter=AMR @GitArgs -- src
    } catch {
      $lines = @()
    }

    $current = ""
    $newLine = 0
    $violations = New-Object System.Collections.Generic.List[string]
    $fileCache = @{}

    foreach ($line in $lines) {
      if ($line.StartsWith("+++ b/")) {
        $current = $line.Substring(6)
        $newLine = 0
        continue
      }
      if ($line.StartsWith("@@")) {
        if ($line -match '\+(\d+)(?:,(\d+))?') {
          $newLine = [int]$Matches[1]
        } else {
          $newLine = 0
        }
        continue
      }
      if (-not $line.StartsWith("+")) { continue }
      if ($line.StartsWith("+++")) { continue }
      if ([string]::IsNullOrWhiteSpace($current)) { continue }
      if (-not (Should-CheckFile $current)) { continue }
      if ($allowlist.Contains($current)) { $newLine++; continue }
      if (Is-TestishPath $current) { $newLine++; continue }

      $text = $line.Substring(1)
      if (-not (Is-PublicItemLine $text)) { $newLine++; continue }

      if (-not $fileCache.ContainsKey($current)) {
        try {
          $fileCache[$current] = Get-FileLinesForSource -Source $Source -Path $current -HeadRef $HeadRef
        } catch {
          $fileCache[$current] = @()
        }
      }

      $fileLines = [string[]]$fileCache[$current]
      if ($fileLines.Length -eq 0) {
        $violations.Add(("{0}:{1}: missing file content for doc check" -f $current, $newLine))
        $newLine++
        continue
      }

      if (-not (Has-DocComment -Lines $fileLines -LineNumber1 $newLine)) {
        $violations.Add(("{0}:{1}: {2}" -f $current, $newLine, $text.Trim()))
      }
      $newLine++
    }

    if ($violations.Count -gt 0) {
      Write-Error ("[public_docs] Violations detected ({0}):" -f $Label)
      Write-Host "[public_docs] Newly added public items must have doc comments (`///` or `#[doc = ...]`)."
      Write-Host ("[public_docs] Allowlist (last resort): {0}" -f $allowlistPath)
      foreach ($v in ($violations | Sort-Object)) {
        Write-Host (" - {0}" -f $v)
      }
      return $false
    }

    return $true
  }

  $ok = $true
  if ((-not [string]::IsNullOrWhiteSpace($Base)) -and (Test-GitCommit $Base) -and (Test-GitCommit $Head)) {
    $ok = $ok -and (Scan-Diff -Label ("range " + $Base + "..." + $Head) -GitArgs @("$Base...$Head") -Source "commit" -HeadRef $Head)
  }
  $ok = $ok -and (Scan-Diff -Label "staged" -GitArgs @("--cached") -Source "index" -HeadRef "")
  $ok = $ok -and (Scan-Diff -Label "unstaged" -GitArgs @() -Source "worktree" -HeadRef "")

  if (-not $ok) { exit 1 }
  Write-Host "[public_docs] OK"
  exit 0
} finally {
  Pop-Location
}
