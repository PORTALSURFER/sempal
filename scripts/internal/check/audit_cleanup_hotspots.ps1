param(
  [string]$Output = "tmp/cleanup_audit_hotspots.md",
  [int]$TopFiles = 20,
  [int]$TopSuppressions = 20,
  [int]$TopFunctionSpans = 20,
  [int]$TestGapMinLines = 200,
  [int]$FileSizeLimit = 400
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path

<#
.SYNOPSIS
Generates a deterministic cleanup-hotspot snapshot for planning.

.DESCRIPTION
PowerShell equivalent of `scripts/check.sh cleanup-hotspots`.

The report includes:
- largest Rust files (line count)
- largest function spans (heuristic)
- files still over the file-size budget limit
- dead-code and clippy::too_many_arguments suppression density
- likely test-gap hotspots (large files without local test modules)

Output defaults to `tmp/cleanup_audit_hotspots.md`.
#>

function Test-NonNegativeInteger {
  param([int]$Value)
  return $Value -ge 0
}

function Get-RustFiles {
  $files = New-Object "System.Collections.Generic.HashSet[string]"
  foreach ($file in @(git ls-files -- '*.rs')) {
    if ($LASTEXITCODE -ne 0) {
      throw "[cleanup_audit] failed to enumerate tracked Rust files"
    }
    $path = $file.Trim().Replace("\", "/")
    if ([string]::IsNullOrWhiteSpace($path) -or -not $path.EndsWith(".rs")) { continue }
    [void]$files.Add($path)
  }

  return @($files) | Sort-Object
}

function Get-FunctionSpans {
  param(
    [string]$FilePath,
    [string[]]$Lines
  )

  $pattern = '^\s*(pub(\([^)]*\))?\s+)?(async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)'
  $spans = New-Object System.Collections.Generic.List[object]
  $currentName = $null
  $currentStart = 0

  for ($index = 0; $index -lt $Lines.Count; $index++) {
    $lineNumber = $index + 1
    $line = $Lines[$index]
    if ($line -match $pattern) {
      if ($null -ne $currentName) {
        $span = [Math]::Max(1, $lineNumber - $currentStart)
        $spans.Add([pscustomobject]@{
            Span = $span
            Location = ("{0}:{1}" -f $FilePath, $currentStart)
            Name = $currentName
          })
      }
      $currentName = $Matches[4]
      $currentStart = $lineNumber
    }
  }

  if ($null -ne $currentName) {
    $span = [Math]::Max(1, ($Lines.Count + 1) - $currentStart)
    $spans.Add([pscustomobject]@{
        Span = $span
        Location = ("{0}:{1}" -f $FilePath, $currentStart)
        Name = $currentName
      })
  }

  return $spans
}

function Format-Code {
  param([string]$Text)
  return ('`{0}`' -f $Text)
}

function Test-IsVendorPath {
  param([string]$FilePath)
  return $false
}

function Get-ScopeLabel {
  param([string]$FilePath)
  if (Test-IsVendorPath -FilePath $FilePath) {
    return "Radiant sibling (standalone validation)"
  }
  return "Wavecrate root"
}

function Get-SuppressionCounts {
  param(
    [object[]]$FileEntries,
    [regex]$Pattern
  )

  $rows = foreach ($entry in $FileEntries) {
    $count = 0
    foreach ($line in $entry.Lines) {
      if ($Pattern.IsMatch($line)) {
        $count++
      }
    }
    if ($count -gt 0) {
      [pscustomobject]@{
        Count = $count
        File = $entry.File
        Scope = $entry.Scope
      }
    }
  }

  return $rows | Sort-Object Count, File -Descending
}

function Test-IsDedicatedTestPath {
  param([string]$FilePath)
  return $FilePath -like "tests/*" -or
    $FilePath -like "*/tests/*" -or
    $FilePath -like "*_test.rs" -or
    $FilePath -like "*_tests.rs" -or
    $FilePath -like "tests.rs" -or
    $FilePath -like "*/tests.rs"
}

function Test-HasLocalTestMarkers {
  param([string[]]$Lines)
  foreach ($line in $Lines) {
    if ($line -match '^\s*#\s*\[cfg\(test\)\]' -or $line -match '^\s*mod\s+tests\b') {
      return $true
    }
  }
  return $false
}

function Test-HasSiblingModuleTests {
  param([string]$FilePath)

  $moduleDir = Split-Path -Parent $FilePath
  if ([string]::IsNullOrWhiteSpace($moduleDir)) {
    return $false
  }

  $testsFile = Join-Path $moduleDir "tests.rs"
  $moduleFile = Join-Path $moduleDir "mod.rs"
  if (-not (Test-Path -LiteralPath $testsFile) -or -not (Test-Path -LiteralPath $moduleFile)) {
    return $false
  }

  $moduleLines = [System.IO.File]::ReadAllLines((Resolve-Path -LiteralPath $moduleFile))
  return Test-HasLocalTestMarkers -Lines $moduleLines
}

function Test-HasTestCoverageHint {
  param(
    [string]$FilePath,
    [string[]]$Lines
  )

  return (Test-HasLocalTestMarkers -Lines $Lines) -or
    (Test-HasSiblingModuleTests -FilePath $FilePath)
}

function Write-MarkdownTable {
  param(
    [System.IO.StreamWriter]$Writer,
    [string[]]$Headers,
    [object[]]$Rows
  )

  $Writer.WriteLine("| {0} |" -f ($Headers -join " | "))
  $Writer.WriteLine("| {0} |" -f (($Headers | ForEach-Object { "---" }) -join " | "))
  foreach ($row in $Rows) {
    $cells = if ($row.PSObject.Properties.Name -contains "Cells") {
      $row.Cells
    } else {
      @($row)
    }
    $Writer.WriteLine("| {0} |" -f ($cells -join " | "))
  }
  $Writer.WriteLine()
}

function Write-RankedSection {
  param(
    [System.IO.StreamWriter]$Writer,
    [string]$Title,
    [string[]]$Headers,
    [object[]]$Rows
  )

  if (-not [string]::IsNullOrWhiteSpace($Title)) {
    $Writer.WriteLine($Title)
    $Writer.WriteLine()
  }
  if (($Rows | Measure-Object).Count -eq 0) {
    $Writer.WriteLine("None.")
    $Writer.WriteLine()
    return
  }

  Write-MarkdownTable -Writer $Writer -Headers $Headers -Rows $Rows
}

foreach ($value in @(
    @{ Name = "TopFiles"; Value = $TopFiles }
    @{ Name = "TopSuppressions"; Value = $TopSuppressions }
    @{ Name = "TopFunctionSpans"; Value = $TopFunctionSpans }
    @{ Name = "TestGapMinLines"; Value = $TestGapMinLines }
    @{ Name = "FileSizeLimit"; Value = $FileSizeLimit }
  )) {
  if (-not (Test-NonNegativeInteger $value.Value)) {
    throw "[cleanup_audit] $($value.Name) must be a non-negative integer (got: $($value.Value))"
  }
}

if ([string]::IsNullOrWhiteSpace($Output)) {
  throw "[cleanup_audit] --output requires a non-empty path"
}

$outputDir = Split-Path -Parent $Output
if (-not [string]::IsNullOrWhiteSpace($outputDir)) {
  New-Item -ItemType Directory -Path $outputDir -Force | Out-Null
}

Push-Location $rootDir
try {
  $rustFiles = Get-RustFiles
  $fileEntries = New-Object System.Collections.Generic.List[object]
  $functionSpans = New-Object System.Collections.Generic.List[object]

  foreach ($file in $rustFiles) {
    if (-not (Test-Path -LiteralPath $file)) {
      continue
    }
    $lines = [System.IO.File]::ReadAllLines((Resolve-Path $file))
    $entry = [pscustomobject]@{
      File = $file
      LineCount = $lines.Count
      Lines = $lines
      Scope = Get-ScopeLabel -FilePath $file
    }
    $fileEntries.Add($entry)
    foreach ($span in (Get-FunctionSpans -FilePath $file -Lines $lines)) {
      $functionSpans.Add($span)
    }
  }

  $sortedFileEntries = $fileEntries | Sort-Object LineCount, File -Descending
  $overLimit = $sortedFileEntries | Where-Object { $_.LineCount -gt $FileSizeLimit }
  $deadCounts = Get-SuppressionCounts -FileEntries $fileEntries -Pattern ([regex]'^\s*#\s*\[allow\([^]]*dead_code[^]]*\)\]')
  $tmaCounts = Get-SuppressionCounts -FileEntries $fileEntries -Pattern ([regex]'^\s*#\s*\[allow\([^]]*clippy::too_many_arguments[^]]*\)\]')
  $testGaps = $sortedFileEntries | Where-Object {
    $_.LineCount -ge $TestGapMinLines -and
    -not (Test-IsDedicatedTestPath $_.File) -and
    -not (Test-HasTestCoverageHint -FilePath $_.File -Lines $_.Lines)
  }

  $timestampUtc = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
  $branch = (git rev-parse --abbrev-ref HEAD 2>$null)
  if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($branch)) {
    $branch = "unknown"
  }
  $commit = (git rev-parse --short HEAD 2>$null)
  if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($commit)) {
    $commit = "unknown"
  }
  $rootEntries = @($fileEntries | Where-Object { $_.Scope -eq "Wavecrate root" })
  $vendorEntries = @()
  $rootOverLimit = @($overLimit | Where-Object { $_.Scope -eq "Wavecrate root" })
  $vendorOverLimit = @()
  $rootFunctionSpans = @($functionSpans | Where-Object { -not (Test-IsVendorPath -FilePath $_.Location) })
  $vendorFunctionSpans = @($functionSpans | Where-Object { Test-IsVendorPath -FilePath $_.Location })
  $rootDeadCounts = @($deadCounts | Where-Object { $_.Scope -eq "Wavecrate root" })
  $vendorDeadCounts = @()
  $rootTmaCounts = @($tmaCounts | Where-Object { $_.Scope -eq "Wavecrate root" })
  $vendorTmaCounts = @()
  $rootTestGaps = @($testGaps | Where-Object { $_.Scope -eq "Wavecrate root" })
  $vendorTestGaps = @()

  $writer = [System.IO.StreamWriter]::new((Resolve-Path -LiteralPath (New-Item -ItemType File -Path $Output -Force)).Path, $false, [System.Text.UTF8Encoding]::new($false))
  try {
    $writer.WriteLine("# Cleanup Hotspot Audit Snapshot")
    $writer.WriteLine()
    $writer.WriteLine(("- Generated (UTC): {0}" -f (Format-Code $timestampUtc)))
    $writer.WriteLine(("- Branch: {0}" -f (Format-Code $branch)))
    $writer.WriteLine(("- Commit: {0}" -f (Format-Code $commit)))
    $writer.WriteLine(("- Rust files scanned: {0}" -f $fileEntries.Count))
    $writer.WriteLine(("- File-size budget limit: {0}" -f (Format-Code ([string]$FileSizeLimit))))
    $writer.WriteLine()

    $writer.WriteLine("## Summary")
    $writer.WriteLine()
    $writer.WriteLine(("- Over file-size budget: {0}" -f ($overLimit | Measure-Object | Select-Object -ExpandProperty Count)))
    $writer.WriteLine(("- Function spans captured: {0}" -f ($functionSpans | Measure-Object | Select-Object -ExpandProperty Count)))
    $writer.WriteLine(("- Files with {0} suppressions: {1}" -f (Format-Code "dead_code"), ($deadCounts | Measure-Object | Select-Object -ExpandProperty Count)))
    $writer.WriteLine(("- Files with {0} suppressions: {1}" -f (Format-Code "clippy::too_many_arguments"), ($tmaCounts | Measure-Object | Select-Object -ExpandProperty Count)))
    $writer.WriteLine(("- Likely large-file test-gap hotspots (heuristic): {0}" -f ($testGaps | Measure-Object | Select-Object -ExpandProperty Count)))
    $writer.WriteLine(("- Wavecrate-root Rust files scanned: {0}" -f ($rootEntries | Measure-Object | Select-Object -ExpandProperty Count)))
    $writer.WriteLine(("- Radiant sibling Rust files scanned separately: {0}" -f ($vendorEntries | Measure-Object | Select-Object -ExpandProperty Count)))
    $writer.WriteLine(("- Wavecrate-root files over budget: {0}" -f ($rootOverLimit | Measure-Object | Select-Object -ExpandProperty Count)))
    $writer.WriteLine(("- Radiant sibling files over budget (standalone gate): {0}" -f ($vendorOverLimit | Measure-Object | Select-Object -ExpandProperty Count)))
    $writer.WriteLine()

    Write-RankedSection -Writer $writer -Title "## Wavecrate-root largest Rust files" -Headers @("Lines", "File") -Rows (
      $rootEntries |
        Sort-Object LineCount, File -Descending |
        Select-Object -First $TopFiles |
        ForEach-Object { [pscustomobject]@{ Cells = @($_.LineCount, (Format-Code $_.File)) } }
    )

    Write-RankedSection -Writer $writer -Title "## Radiant sibling largest Rust files (standalone gate)" -Headers @("Lines", "File") -Rows (
      $vendorEntries |
        Sort-Object LineCount, File -Descending |
        Select-Object -First $TopFiles |
        ForEach-Object { [pscustomobject]@{ Cells = @($_.LineCount, (Format-Code $_.File)) } }
    )

    Write-RankedSection -Writer $writer -Title "## Wavecrate-root largest function spans (heuristic)" -Headers @("Span (lines)", "Function") -Rows (
      $rootFunctionSpans |
        Sort-Object Span, Location -Descending |
        Select-Object -First $TopFunctionSpans |
        ForEach-Object { [pscustomobject]@{ Cells = @($_.Span, ("{0} ({1})" -f (Format-Code $_.Name), (Format-Code $_.Location))) } }
    )

    Write-RankedSection -Writer $writer -Title "## Radiant sibling largest function spans (standalone gate)" -Headers @("Span (lines)", "Function") -Rows (
      $vendorFunctionSpans |
        Sort-Object Span, Location -Descending |
        Select-Object -First $TopFunctionSpans |
        ForEach-Object { [pscustomobject]@{ Cells = @($_.Span, ("{0} ({1})" -f (Format-Code $_.Name), (Format-Code $_.Location))) } }
    )

    Write-RankedSection -Writer $writer -Title "## Wavecrate-root files over budget" -Headers @("Lines", "File") -Rows (
      $rootOverLimit | ForEach-Object { [pscustomobject]@{ Cells = @($_.LineCount, (Format-Code $_.File)) } }
    )

    Write-RankedSection -Writer $writer -Title "## Radiant sibling files over budget (standalone gate)" -Headers @("Lines", "File") -Rows (
      $vendorOverLimit | ForEach-Object { [pscustomobject]@{ Cells = @($_.LineCount, (Format-Code $_.File)) } }
    )

    Write-RankedSection -Writer $writer -Title "## Wavecrate-root dead_code suppression density" -Headers @("Occurrences", "File") -Rows (
      $rootDeadCounts |
        Select-Object -First $TopSuppressions |
        ForEach-Object { [pscustomobject]@{ Cells = @($_.Count, (Format-Code $_.File)) } }
    )

    Write-RankedSection -Writer $writer -Title "## Radiant sibling dead_code suppression density (standalone gate)" -Headers @("Occurrences", "File") -Rows (
      $vendorDeadCounts |
        Select-Object -First $TopSuppressions |
        ForEach-Object { [pscustomobject]@{ Cells = @($_.Count, (Format-Code $_.File)) } }
    )

    Write-RankedSection -Writer $writer -Title "## Wavecrate-root too_many_arguments suppression density" -Headers @("Occurrences", "File") -Rows (
      $rootTmaCounts |
        Select-Object -First $TopSuppressions |
        ForEach-Object { [pscustomobject]@{ Cells = @($_.Count, (Format-Code $_.File)) } }
    )

    Write-RankedSection -Writer $writer -Title "## Radiant sibling too_many_arguments suppression density (standalone gate)" -Headers @("Occurrences", "File") -Rows (
      $vendorTmaCounts |
        Select-Object -First $TopSuppressions |
        ForEach-Object { [pscustomobject]@{ Cells = @($_.Count, (Format-Code $_.File)) } }
    )

    $writer.WriteLine("## Wavecrate-root likely test-gap hotspots (heuristic)")
    $writer.WriteLine()
    $writer.WriteLine(("Files with at least {0} lines and no local {1} or {2} marker." -f (Format-Code ([string]$TestGapMinLines)), (Format-Code "#[cfg(test)]"), (Format-Code "mod tests")))
    $writer.WriteLine(("Skips dedicated test modules/paths ({0}, {1}, {2}, {3}) and sibling module tests declared through {4} + {5}." -f (Format-Code "tests/**"), (Format-Code "tests.rs"), (Format-Code "*_test.rs"), (Format-Code "*_tests.rs"), (Format-Code "mod.rs"), (Format-Code "tests.rs")))
    $writer.WriteLine()
    Write-RankedSection -Writer $writer -Title "" -Headers @("Lines", "File") -Rows (
      $rootTestGaps |
        Select-Object -First $TopFiles |
        ForEach-Object { [pscustomobject]@{ Cells = @($_.LineCount, (Format-Code $_.File)) } }
    )

    $writer.WriteLine("## Radiant sibling likely test-gap hotspots (standalone gate)")
    $writer.WriteLine()
    $writer.WriteLine(("Files with at least {0} lines and no local {1} or {2} marker." -f (Format-Code ([string]$TestGapMinLines)), (Format-Code "#[cfg(test)]"), (Format-Code "mod tests")))
    $writer.WriteLine(("Skips dedicated test modules/paths ({0}, {1}, {2}, {3}) and sibling module tests declared through {4} + {5}." -f (Format-Code "tests/**"), (Format-Code "tests.rs"), (Format-Code "*_test.rs"), (Format-Code "*_tests.rs"), (Format-Code "mod.rs"), (Format-Code "tests.rs")))
    $writer.WriteLine()
    Write-RankedSection -Writer $writer -Title "" -Headers @("Lines", "File") -Rows (
      $vendorTestGaps |
        Select-Object -First $TopFiles |
        ForEach-Object { [pscustomobject]@{ Cells = @($_.LineCount, (Format-Code $_.File)) } }
    )

    $writer.WriteLine("## Suggested follow-up")
    $writer.WriteLine()
    $writer.WriteLine("1. Triage Wavecrate-root candidates separately from the standalone Radiant repository.")
    $writer.WriteLine("2. Remove or test-gate high-density suppressions after each refactor slice.")
    $writer.WriteLine("3. Add focused tests for top heuristic gaps where behavior is non-trivial.")
  } finally {
    $writer.Dispose()
  }
} finally {
  Pop-Location
}

Write-Host "[cleanup_audit] wrote $Output"
