
<#
.SYNOPSIS
Enforces lightweight Rust “taste invariants” (diff-aware).

.DESCRIPTION
Fails when added lines introduce forbidden patterns in non-test Rust sources:
- `dbg!(...)`
- `println!(...)`
- `.unwrap()`
- `.expect(...)`

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
  $allowlistPath = Join-Path $rootDir "scripts/internal/check/allowlists/rust_taste_invariants_allowlist.txt"
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
    return (($Path -like "src/*.rs") -or ($Path -like "src/*/*.rs") -or ($Path -like "src/*/*/*.rs") -or ($Path -like "src/*/*/*.rs") -or ($Path -like "src/*/*/*/*.rs") -or ($Path -like "src/*/*/*/*/*.rs") -or ($Path -like "src/*/*/*/*/*/*.rs") -or ($Path -like "src/*/*/*/*/*/*/*.rs") -or ($Path -like "src/*/*/*/*/*/*/*/*.rs"))
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

  function Scan-DiffLines([string]$Label, [string[]]$Lines) {
    $current = ""
    $violations = @()
    foreach ($line in $Lines) {
      if ($line.StartsWith("+++ b/")) {
        $current = $line.Substring(6)
        continue
      }
      if (-not $line.StartsWith("+")) { continue }
      if ($line.StartsWith("+++")) { continue }
      if ([string]::IsNullOrWhiteSpace($current)) { continue }
      if (-not (Should-CheckFile $current)) { continue }
      if ($allowlist.Contains($current)) { continue }
      if (Is-TestishPath $current) { continue }
      if ($line -match '^\+\s*//') { continue }

      $text = $line.Substring(1)
      if ($text -match '\bdbg!\s*\(') {
        $violations += ("{0}: dbg!: {1}" -f $current, $text.Trim())
      }
      if ($text -match '\bprintln!\s*\(') {
        $violations += ("{0}: println!: {1}" -f $current, $text.Trim())
      }
      if ($text -match '\.unwrap\(\)') {
        $violations += ("{0}: unwrap(): {1}" -f $current, $text.Trim())
      }
      if ($text -match '\.expect\s*\(') {
        $violations += ("{0}: expect(): {1}" -f $current, $text.Trim())
      }
    }

    if ($violations.Count -gt 0) {
      Write-Error ("[taste] Violations detected ({0}):" -f $Label)
      Write-Host "[taste] Use `tracing` instead of `dbg!`/`println!` in non-test code."
      Write-Host "[taste] Avoid `.unwrap()`/`.expect(...)` in non-test code; propagate errors."
      Write-Host ("[taste] Allowlist (last resort): {0}" -f $allowlistPath)
      foreach ($v in ($violations | Sort-Object)) {
        Write-Host (" - {0}" -f $v)
      }
      return $false
    }
    return $true
  }

  function Scan-GitDiff([string]$Label, [string[]]$Args) {
    $lines = @()
    try {
      $lines = git diff --unified=0 --diff-filter=AMR @Args -- src
    } catch {
      $lines = @()
    }
    return (Scan-DiffLines -Label $Label -Lines $lines)
  }

  $ok = $true
  if ((-not [string]::IsNullOrWhiteSpace($Base)) -and (Test-GitCommit $Base) -and (Test-GitCommit $Head)) {
    $ok = $ok -and (Scan-GitDiff -Label ("range " + $Base + "..." + $Head) -Args @("$Base...$Head"))
  }
  $ok = $ok -and (Scan-GitDiff -Label "staged" -Args @("--cached"))
  $ok = $ok -and (Scan-GitDiff -Label "unstaged" -Args @())

  if (-not $ok) { exit 1 }
  Write-Host "[taste] OK"
  exit 0
} finally {
  Pop-Location
}
