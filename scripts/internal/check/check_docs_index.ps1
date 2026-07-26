Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

<#
.SYNOPSIS
Ensures `docs/README.md` remains a reliable system-of-record landing page.

.DESCRIPTION
Checks:
- Required docs are referenced by path in `docs/README.md`
- Any `docs/*.md` path referenced in `docs/README.md` exists on disk
- `docs/README.md` indicates that planning/backlog live in Linear
#>

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Push-Location $rootDir
try {
  $docsReadme = "docs/README.md"
  if (-not (Test-Path -LiteralPath $docsReadme -PathType Leaf)) {
    throw "[docs_index] Missing $docsReadme"
  }

  $required = @(
    "docs/ENV_VARS.md"
    "docs/TEST.md"
    "docs/TARGET.md"
    "docs/TROUBLESHOOTING.md"
  )
  $requiredPhrases = @(
    'Linear project `Wavecrate` under team `PORTALSURFER`'
  )

  $text = Get-Content -LiteralPath $docsReadme -Raw

  $missingRefs = @()
  foreach ($path in $required) {
    if (-not $text.Contains($path)) {
      $missingRefs += $path
    }
  }
  foreach ($path in $required) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
      throw ("[docs_index] Required file missing on disk: {0}" -f $path)
    }
  }
  foreach ($phrase in $requiredPhrases) {
    if (-not $text.Contains($phrase)) {
      $missingRefs += $phrase
    }
  }
  if ($missingRefs.Count -gt 0) {
    Write-Error "[docs_index] docs/README.md is missing required references:"
    foreach ($m in $missingRefs) {
      Write-Host (" - {0}" -f $m)
    }
    exit 1
  }

  $matches = [regex]::Matches($text, '\bdocs/[A-Za-z0-9._/-]+\.md\b')
  $paths = New-Object "System.Collections.Generic.HashSet[string]"
  foreach ($m in $matches) {
    [void]$paths.Add($m.Value)
  }

  $missing = @()
  foreach ($p in $paths) {
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) {
      $missing += $p
    }
  }
  if ($missing.Count -gt 0) {
    Write-Error "[docs_index] docs/README.md references missing files:"
    foreach ($m in ($missing | Sort-Object)) {
      Write-Host (" - {0}" -f $m)
    }
    exit 1
  }

  $legacyPlanRefs = @(
    "docs/plans/index.md"
    "docs/plans/TEMPLATE_execution_plan.md"
    "docs/plans/TEMPLATE_investigation.md"
    "docs/plans/active/todo.md"
    "tmp/improvement_audit_plan.md"
  )
  $legacyHits = @($legacyPlanRefs | Where-Object { $text -like "*$_*" })
  if ($legacyHits.Count -gt 0) {
    Write-Error "[docs_index] docs/README.md should not present Markdown plan files as live workflow entrypoints:"
    foreach ($hit in $legacyHits) {
      Write-Host (" - {0}" -f $hit)
    }
    exit 1
  }

  Write-Host ("[docs_index] OK ({0} referenced docs files)" -f $paths.Count)
} finally {
  Pop-Location
}
