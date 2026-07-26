Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

<#
.SYNOPSIS
Runs PowerShell-side script guardrail checks for agent tooling.

.DESCRIPTION
Validates parseability and basic behavior contracts for preflight-facing
PowerShell scripts, including lightweight fixture checks.
#>

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
$scriptsDir = Join-Path $rootDir "scripts"
$validationContractPath = Join-Path $scriptsDir "internal/data/validation_contract.json"
$failures = 0

$psRunner = Get-Command pwsh -ErrorAction SilentlyContinue
if ($null -eq $psRunner) {
  $psRunner = Get-Command powershell -ErrorAction SilentlyContinue
}
if ($null -eq $psRunner) {
  throw "[guardrails] Neither pwsh nor powershell is available."
}
$psExe = $psRunner.Path

function Add-Failure {
  param([string]$Message)
  Write-Host "[guardrails] FAIL: $Message"
  $script:failures++
}

function Write-Pass {
  param([string]$Label)
  Write-Host "[guardrails] PASS: $Label"
}

function Assert-ScriptParses {
  param([string]$Path)
  $tokens = $null
  $errors = $null
  [System.Management.Automation.Language.Parser]::ParseFile($Path, [ref]$tokens, [ref]$errors) | Out-Null
  if ($errors -and $errors.Count -gt 0) {
    Add-Failure "parse check failed for $Path"
    foreach ($error in $errors) {
      Write-Host ("  - {0}" -f $error.Message)
    }
    return
  }
  Write-Pass "parse check for $Path"
}

function Invoke-ExpectExitCode {
  param(
    [string]$Label,
    [int]$ExpectedCode,
    [string]$WorkDir,
    [string]$ScriptPath,
    [string[]]$Arguments = @(),
    [hashtable]$EnvVars = @{}
  )

  $previous = @{}
  foreach ($key in $EnvVars.Keys) {
    $previous[$key] = [Environment]::GetEnvironmentVariable($key)
    [Environment]::SetEnvironmentVariable($key, [string]$EnvVars[$key])
  }

  try {
    Push-Location $WorkDir
    try {
      $prevEap = $ErrorActionPreference
      $ErrorActionPreference = "Continue"
      try {
        $output = & $psExe -NoProfile -File $ScriptPath @Arguments 2>&1
      } finally {
        $ErrorActionPreference = $prevEap
      }
      $exitCode = $LASTEXITCODE
    } finally {
      Pop-Location
    }
  } finally {
    foreach ($key in $EnvVars.Keys) {
      [Environment]::SetEnvironmentVariable($key, $previous[$key])
    }
  }

  if ($exitCode -eq $ExpectedCode) {
    Write-Pass $Label
    return
  }

  Add-Failure "$Label (expected $ExpectedCode, got $exitCode)"
  if ($null -ne $output) {
    foreach ($line in $output) {
      Write-Host ("  {0}" -f $line.ToString())
    }
  }
}

function Invoke-ExpectOutput {
  param(
    [string]$Label,
    [int]$ExpectedCode = 0,
    [string]$WorkDir,
    [string]$ScriptPath,
    [string[]]$Arguments = @(),
    [string[]]$ExpectedSubstrings = @(),
    [hashtable]$EnvVars = @{},
    [string]$PowerShellPath = $psExe
  )

  $previous = @{}
  foreach ($key in $EnvVars.Keys) {
    $previous[$key] = [Environment]::GetEnvironmentVariable($key)
    [Environment]::SetEnvironmentVariable($key, [string]$EnvVars[$key])
  }

  try {
    Push-Location $WorkDir
    try {
      $prevEap = $ErrorActionPreference
      $ErrorActionPreference = "Continue"
      try {
        $output = & $PowerShellPath -NoProfile -File $ScriptPath @Arguments 2>&1
      } finally {
        $ErrorActionPreference = $prevEap
      }
      $exitCode = $LASTEXITCODE
    } finally {
      Pop-Location
    }
  } finally {
    foreach ($key in $EnvVars.Keys) {
      [Environment]::SetEnvironmentVariable($key, $previous[$key])
    }
  }

  $text = if ($null -eq $output) { "" } else { ($output | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine }
  $missing = @($ExpectedSubstrings | Where-Object { -not $text.Contains($_) })
  if ($exitCode -eq $ExpectedCode -and $missing.Count -eq 0) {
    Write-Pass $Label
    return
  }

  if ($exitCode -ne $ExpectedCode) {
    Add-Failure "$Label (expected exit code $ExpectedCode, got $exitCode)"
  } else {
    Add-Failure "$Label (missing expected output fragments)"
  }
  if ($text.Length -gt 0) {
    foreach ($line in ($text -split [Environment]::NewLine)) {
      Write-Host ("  {0}" -f $line)
    }
  }
  foreach ($fragment in $missing) {
    Write-Host ("  missing: {0}" -f $fragment)
  }
}

function New-TempDir {
  $tempPath = Join-Path ([System.IO.Path]::GetTempPath()) ("wavecrate-guardrails-" + [System.Guid]::NewGuid().ToString("N"))
  New-Item -ItemType Directory -Path $tempPath | Out-Null
  return $tempPath
}

function Assert-TextContains {
  param(
    [string]$Label,
    [string]$Text,
    [string]$Fragment
  )

  if ($Text -like "*$Fragment*") {
    Write-Pass $Label
    return
  }

  Add-Failure "$Label (missing fragment: $Fragment)"
}

function Assert-TextNotContains {
  param(
    [string]$Label,
    [string]$Text,
    [string]$Fragment
  )

  if ($Text -notlike "*$Fragment*") {
    Write-Pass $Label
    return
  }

  Add-Failure "$Label (unexpected fragment: $Fragment)"
}

function Assert-AgentCiCheckDirectory {
  param([string]$Path)

  $text = Get-Content -Path $Path -Raw
  Assert-TextContains -Label "agent ci PowerShell uses canonical internal check directory" -Text $text -Fragment '"scripts/internal/check"'
  Assert-TextNotContains -Label "agent ci PowerShell does not use stale scripts/check directory" -Text $text -Fragment '"scripts/check"'
}

function Assert-NoAmbiguousWavecrateCargoPackageShorthand {
  $violations = New-Object System.Collections.Generic.List[string]
  $pattern = '(?m)^\s*Invoke-WavecrateCargo\b[^\r\n]*\s-p(\s|$)'
  foreach ($scriptPath in Get-ChildItem -Path $scriptsDir -Recurse -Filter "*.ps1") {
    $matches = Select-String -Path $scriptPath.FullName -Pattern $pattern
    foreach ($match in $matches) {
      $relativePath = Resolve-Path -Path $match.Path -Relative
      $violations.Add(("{0}:{1}: use --package instead of -p when calling Invoke-WavecrateCargo" -f $relativePath, $match.LineNumber))
    }
  }

  if ($violations.Count -eq 0) {
    Write-Pass "Invoke-WavecrateCargo calls avoid ambiguous -p package shorthand"
    return
  }

  Add-Failure "Invoke-WavecrateCargo package shorthand can bind as a PowerShell parameter"
  foreach ($violation in $violations) {
    Write-Host ("  - {0}" -f $violation)
  }
}

function Get-Inventory {
  $inventoryPath = Join-Path $scriptsDir "command-inventory.json"
  if (-not (Test-Path -LiteralPath $inventoryPath)) {
    Add-Failure "script command inventory is missing"
    return $null
  }

  try {
    return (Get-Content -Path $inventoryPath -Raw | ConvertFrom-Json)
  } catch {
    Add-Failure "script command inventory is not valid JSON: $($_.Exception.Message)"
    return $null
  }
}

function Get-ValidationContract {
  if (-not (Test-Path -LiteralPath $validationContractPath)) {
    Add-Failure "shared validation contract is missing"
    return $null
  }

  try {
    return (Get-Content -Path $validationContractPath -Raw | ConvertFrom-Json)
  } catch {
    Add-Failure "shared validation contract is not valid JSON: $($_.Exception.Message)"
    return $null
  }
}

function Add-InventoryPath {
  param(
    [System.Collections.Generic.HashSet[string]]$Set,
    [string]$Path
  )

  if ([string]::IsNullOrWhiteSpace($Path)) {
    Add-Failure "script inventory contains an empty path"
    return
  }
  [void]$Set.Add($Path.Replace("\", "/"))
}

function Assert-ScriptInventoryClassifiesTopLevel {
  param([object]$Inventory)

  if ($null -eq $Inventory) { return }

  $classified = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
  foreach ($entry in @($Inventory.public_entrypoints)) {
    Add-InventoryPath -Set $classified -Path $entry.path
  }
  foreach ($entry in @($Inventory.compatibility_wrappers)) {
    Add-InventoryPath -Set $classified -Path $entry.path
  }
  foreach ($path in @($Inventory.top_level_non_commands)) {
    Add-InventoryPath -Set $classified -Path $path
  }

  $topLevelFiles = Get-ChildItem -LiteralPath $scriptsDir -File |
    Where-Object { $_.Name -ne "README.md" } |
    ForEach-Object { ("scripts/{0}" -f $_.Name) } |
    Sort-Object

  $missing = @($topLevelFiles | Where-Object { -not $classified.Contains($_) })
  $unexpected = @($classified | Where-Object {
      $_ -like "scripts/*" -and
      $_.Substring("scripts/".Length).Contains("/") -eq $false -and
      -not (Test-Path -LiteralPath (Join-Path $rootDir $_))
    })

  if ($missing.Count -eq 0 -and $unexpected.Count -eq 0) {
    Write-Pass "script inventory classifies every top-level scripts/ file"
  } else {
    foreach ($path in $missing) {
      Add-Failure "script inventory does not classify $path"
    }
    foreach ($path in $unexpected) {
      Add-Failure "script inventory references missing top-level file $path"
    }
  }
}

function Assert-CompatibilityWrappers {
  param([object]$Inventory)

  if ($null -eq $Inventory) { return }

  foreach ($wrapper in @($Inventory.compatibility_wrappers)) {
    $path = Join-Path $rootDir $wrapper.path
    if (-not (Test-Path -LiteralPath $path)) {
      Add-Failure "compatibility wrapper is missing: $($wrapper.path)"
      continue
    }

    $canonicalParts = @([string]$wrapper.canonical -split "\s+")
    if ($canonicalParts.Count -lt 2) {
      Add-Failure "compatibility wrapper $($wrapper.path) has invalid canonical command: $($wrapper.canonical)"
      continue
    }

    $text = Get-Content -Path $path -Raw
    $dispatcher = Split-Path -Leaf $canonicalParts[0]
    $command = $canonicalParts[1]
    Assert-TextContains -Label "compatibility wrapper $($wrapper.path) points at $($wrapper.canonical)" -Text $text -Fragment $dispatcher
    Assert-TextContains -Label "compatibility wrapper $($wrapper.path) passes canonical subcommand $command" -Text $text -Fragment $command
  }
}

function Assert-DispatcherInventory {
  param([object]$Inventory)

  if ($null -eq $Inventory) { return }

  foreach ($dispatcher in @($Inventory.dispatchers)) {
    if ($dispatcher.kind -ne "powershell") {
      continue
    }

    $path = Join-Path $rootDir $dispatcher.path
    if (-not (Test-Path -LiteralPath $path)) {
      Add-Failure "dispatcher is missing: $($dispatcher.path)"
      continue
    }

    $text = Get-Content -Path $path -Raw
    foreach ($command in @($dispatcher.commands)) {
      $fragment = ('"{0}" = "{1}"' -f $command.name, $command.target)
      Assert-TextContains -Label "dispatcher map $($dispatcher.path) includes $($command.name)" -Text $text -Fragment $fragment
    }
  }
}

function Assert-PowerShellDispatcherWindowsTargets {
  param([object]$Inventory)

  if ($null -eq $Inventory) { return }

  foreach ($dispatcher in @($Inventory.dispatchers)) {
    if ($dispatcher.kind -ne "powershell") {
      continue
    }

    foreach ($command in @($dispatcher.commands)) {
      $target = [string]$command.target
      if (-not $target.EndsWith(".sh")) {
        continue
      }

      $windowsSupported = $true
      if ($command.PSObject.Properties.Name -contains "windows_supported") {
        $windowsSupported = [bool]$command.windows_supported
      }
      $reason = ""
      if ($command.PSObject.Properties.Name -contains "reason") {
        $reason = [string]$command.reason
      }

      if (-not $windowsSupported -and -not [string]::IsNullOrWhiteSpace($reason)) {
        Write-Pass "dispatcher map $($dispatcher.path) documents Bash-only command $($command.name)"
        continue
      }

      Add-Failure "PowerShell dispatcher $($dispatcher.path) command $($command.name) is backed only by Bash target $target without explicit windows_supported=false metadata"
    }
  }
}

Push-Location $rootDir
try {
  $validationContract = Get-ValidationContract
  $scriptsToParse = @($validationContract.script_guardrails.parse_powershell | ForEach-Object {
      Join-Path $rootDir ([string]$_)
    })
  foreach ($scriptPath in $scriptsToParse) {
    Assert-ScriptParses -Path $scriptPath
  }

  Assert-AgentCiCheckDirectory -Path (Join-Path $scriptsDir "internal/agent/run_agent_ci_checks.ps1")
  Assert-NoAmbiguousWavecrateCargoPackageShorthand
  $inventory = Get-Inventory
  Assert-ScriptInventoryClassifiesTopLevel -Inventory $inventory
  Assert-CompatibilityWrappers -Inventory $inventory
  Assert-DispatcherInventory -Inventory $inventory
  Assert-PowerShellDispatcherWindowsTargets -Inventory $inventory

  foreach ($requiredFragment in @($validationContract.script_guardrails.required_fragments)) {
    $path = Join-Path $rootDir ([string]$requiredFragment.path)
    $text = Get-Content -Path $path -Raw
    Assert-TextContains -Label ([string]$requiredFragment.label) -Text $text -Fragment ([string]$requiredFragment.fragment)
  }

  Invoke-ExpectExitCode -Label "agent request --Help" -ExpectedCode 0 -WorkDir $rootDir -ScriptPath (Join-Path $scriptsDir "agent.ps1") -Arguments @("request", "-Help")
  Invoke-ExpectExitCode -Label "run_agent_preflight --Help" -ExpectedCode 0 -WorkDir $rootDir -ScriptPath (Join-Path $scriptsDir "internal/agent/run_agent_preflight.ps1") -Arguments @("-Help")
  Invoke-ExpectExitCode -Label "ci smoke --Help" -ExpectedCode 0 -WorkDir $rootDir -ScriptPath (Join-Path $scriptsDir "ci.ps1") -Arguments @("smoke", "-Help")
  Invoke-ExpectExitCode -Label "ci agent --Help" -ExpectedCode 0 -WorkDir $rootDir -ScriptPath (Join-Path $scriptsDir "ci.ps1") -Arguments @("agent", "-Help")
  Invoke-ExpectExitCode -Label "ci quick --Help" -ExpectedCode 0 -WorkDir $rootDir -ScriptPath (Join-Path $scriptsDir "ci.ps1") -Arguments @("quick", "-Help")
  Invoke-ExpectOutput -Label "size hotspot report surfaces scripts" -ExpectedCode 0 -WorkDir $rootDir -ScriptPath (Join-Path $scriptsDir "internal/check/report_size_hotspots.ps1") -Arguments @("-Limit", "1", "-TopFiles", "5") -ExpectedSubstrings @("# Size Hotspot Report", "scripts/internal", "Over Budget")

  $fixtureDir = New-TempDir
  try {
    $repoDir = Join-Path $fixtureDir "repo"
    New-Item -ItemType Directory -Path $repoDir | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "src") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "scripts") | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "scripts/internal/check") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "docs") | Out-Null

    Copy-Item (Join-Path $scriptsDir "internal/check/check_file_size_budget.ps1") (Join-Path $repoDir "scripts/internal/check/check_file_size_budget.ps1")
    Set-Content -Path (Join-Path $repoDir "src/too_many_lines.rs") -Value @(
      "fn ok() {",
      "    println!(`"budget`");",
      "}"
    )
    Set-Content -Path (Join-Path $repoDir "src/blank_lines_count.rs") -Value @(
      "fn keep_blank_lines() {",
      "",
      "    println!(`"count me`");",
      "",
      "}"
    )

    git -C $repoDir init -q
    git -C $repoDir config user.name "wavecrate-ci"
    git -C $repoDir config user.email "ci@wavecrate.test"
    git -C $repoDir add src/too_many_lines.rs src/blank_lines_count.rs
    git -C $repoDir commit -qm "seed"

    Add-Content -Path (Join-Path $repoDir "src/too_many_lines.rs") -Value @("    println!(`"budget`");", "    println!(`"budget`");", "    println!(`"budget`");")
    git -C $repoDir add src/too_many_lines.rs
    git -C $repoDir commit -qm "add budget lines"

    Invoke-ExpectExitCode -Label "file size budget catches over-limit Wavecrate file" -ExpectedCode 1 -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/internal/check/check_file_size_budget.ps1") -Arguments @("-All", "-Limit", "3")
    Invoke-ExpectExitCode -Label "file size budget passes under limit" -ExpectedCode 0 -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/internal/check/check_file_size_budget.ps1") -Arguments @("-All", "-Limit", "10")
    Invoke-ExpectExitCode -Label "file size budget counts blank lines in project files" -ExpectedCode 1 -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/internal/check/check_file_size_budget.ps1") -Arguments @("-All", "-Limit", "4")
  } finally {
    Remove-Item -Recurse -Force $fixtureDir -ErrorAction SilentlyContinue
  }

  $cleanupAuditFixtureDir = New-TempDir
  try {
    $repoDir = Join-Path $cleanupAuditFixtureDir "repo"
    New-Item -ItemType Directory -Path $repoDir | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "src/analysis") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "src/selection") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "scripts/internal/check") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "tmp") | Out-Null

    Copy-Item (Join-Path $scriptsDir "internal/check/audit_cleanup_hotspots.ps1") (Join-Path $repoDir "scripts/internal/check/audit_cleanup_hotspots.ps1")

    Set-Content -Path (Join-Path $repoDir "src/analysis/ann_index_tests.rs") -Value @(
      "fn alpha() {}",
      "fn beta() {}",
      "fn gamma() {}"
    )
    Set-Content -Path (Join-Path $repoDir "src/selection/mod.rs") -Value @(
      "#[cfg(test)]",
      "mod tests;"
    )
    Set-Content -Path (Join-Path $repoDir "src/selection/tests.rs") -Value @(
      "#[test]",
      "fn selection_is_covered() {}"
    )
    Set-Content -Path (Join-Path $repoDir "src/selection/range.rs") -Value @(
      "pub fn start() {}",
      "pub fn end() {}",
      "pub fn clamp() {}"
    )
    Set-Content -Path (Join-Path $repoDir "src/real_gap.rs") -Value @(
      "pub fn one() {}",
      "pub fn two() {}",
      "pub fn three() {}"
    )
    git -C $repoDir init -q
    git -C $repoDir config user.name "wavecrate-ci"
    git -C $repoDir config user.email "ci@wavecrate.test"
    git -C $repoDir add src scripts
    git -C $repoDir commit -qm "seed"

    $outputPath = Join-Path $repoDir "tmp/cleanup.md"
    Invoke-ExpectExitCode -Label "cleanup audit fixture succeeds" -ExpectedCode 0 -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/internal/check/audit_cleanup_hotspots.ps1") -Arguments @("-Output", $outputPath, "-TestGapMinLines", "3", "-TopFiles", "10")
    $reportText = Get-Content -Path $outputPath -Raw
    $sectionStart = $reportText.IndexOf("## Wavecrate-root likely test-gap hotspots (heuristic)")
    $sectionEnd = $reportText.IndexOf("## Radiant sibling likely test-gap hotspots (standalone gate)")
    $testGapSection = if ($sectionStart -ge 0 -and $sectionEnd -gt $sectionStart) {
      $reportText.Substring($sectionStart, $sectionEnd - $sectionStart)
    } else {
      $reportText
    }
    Assert-TextContains -Label "cleanup audit fixture reports one Wavecrate heuristic gap" -Text $reportText -Fragment "Likely large-file test-gap hotspots (heuristic): 1"
    Assert-TextContains -Label "cleanup audit fixture emits root section" -Text $reportText -Fragment "## Wavecrate-root largest Rust files"
    Assert-TextContains -Label "cleanup audit fixture emits standalone section" -Text $reportText -Fragment "## Radiant sibling largest Rust files"
    Assert-TextContains -Label "cleanup audit fixture keeps the real gap" -Text $testGapSection -Fragment 'src/real_gap.rs'
    Assert-TextNotContains -Label "cleanup audit fixture skips *_tests.rs files" -Text $testGapSection -Fragment 'src/analysis/ann_index_tests.rs'
    Assert-TextNotContains -Label "cleanup audit fixture skips sibling module tests" -Text $testGapSection -Fragment 'src/selection/range.rs'
  } finally {
    Remove-Item -Recurse -Force $cleanupAuditFixtureDir -ErrorAction SilentlyContinue
  }

  $docsIndexFixtureDir = New-TempDir
  try {
    $repoDir = Join-Path $docsIndexFixtureDir "repo"
    New-Item -ItemType Directory -Path $repoDir | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "scripts/internal/check") -Force | Out-Null

    Copy-Item (Join-Path $scriptsDir "internal/check/check_docs_index.ps1") (Join-Path $repoDir "scripts/internal/check/check_docs_index.ps1")

    foreach ($path in @(
        "docs/ENV_VARS.md",
        "docs/TEST.md",
        "docs/TARGET.md",
        "docs/TROUBLESHOOTING.md"
      )) {
      New-Item -ItemType File -Path (Join-Path $repoDir $path) -Force | Out-Null
    }

    Set-Content -Path (Join-Path $repoDir "docs/README.md") -Value @(
      "# Developer documentation",
      "",
      '- `docs/ENV_VARS.md`',
      '- `docs/TEST.md`',
      '- `docs/TARGET.md`',
      '- `docs/TROUBLESHOOTING.md`',
      '- `AGENTS.md`',
      '- Planning and backlog',
      '  - live in Linear project `Wavecrate` under team `PORTALSURFER`'
    )

    Invoke-ExpectExitCode -Label "docs index fixture accepts Linear planning pointer" -ExpectedCode 0 -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/internal/check/check_docs_index.ps1")

    Set-Content -Path (Join-Path $repoDir "docs/README.md") -Value @(
      "# Developer documentation",
      "",
      '- `docs/ENV_VARS.md`',
      '- `docs/TEST.md`',
      '- `docs/TARGET.md`',
      '- `docs/TROUBLESHOOTING.md`',
      '- `AGENTS.md`',
      '- `docs/plans/index.md`'
    )

    Invoke-ExpectExitCode -Label "docs index fixture rejects legacy markdown planning entrypoints" -ExpectedCode 1 -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/internal/check/check_docs_index.ps1")
  } finally {
    Remove-Item -Recurse -Force $docsIndexFixtureDir -ErrorAction SilentlyContinue
  }

  $runHelperFixtureDir = New-TempDir
  try {
    $repoDir = Join-Path $runHelperFixtureDir "repo"
    New-Item -ItemType Directory -Path $repoDir | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "scripts/internal/run") -Force | Out-Null
    $fakeBinDir = Join-Path $repoDir "fake-bin"
    New-Item -ItemType Directory -Path $fakeBinDir -Force | Out-Null

    Copy-Item (Join-Path $scriptsDir "run.ps1") (Join-Path $repoDir "scripts/run.ps1")
    Copy-Item (Join-Path $rootDir "run.ps1") (Join-Path $repoDir "run.ps1")
    Copy-Item (Join-Path $scriptsDir "internal/run/latest_log.ps1") (Join-Path $repoDir "scripts/internal/run/latest_log.ps1")
    Set-Content -Path (Join-Path $fakeBinDir "cargo") -Value @(
      '#!/usr/bin/env sh',
      'printf "[cargo-fixture] cwd=%s args=%s\n" "$PWD" "$*"'
    )
    $isWindowsHost = ($PSVersionTable.PSEdition -eq "Desktop") -or ($env:OS -eq "Windows_NT")
    if ($isWindowsHost) {
      Set-Content -Path (Join-Path $fakeBinDir "cargo.cmd") -Value @(
        '@echo off',
        'echo [cargo-fixture] cwd=%CD% args=%*'
      )
    } else {
      chmod +x (Join-Path $fakeBinDir "cargo")
    }
    $fakeCargoEnv = @{
      PATH = $fakeBinDir + [System.IO.Path]::PathSeparator + [Environment]::GetEnvironmentVariable("PATH")
    }

    $configBase = Join-Path $repoDir "fixture-config"
    $liveLogsDir = Join-Path $configBase ".wavecrate/logs"
    New-Item -ItemType Directory -Path $liveLogsDir -Force | Out-Null
    Set-Content -Path (Join-Path $liveLogsDir "older.log") -Value "older live log"
    Start-Sleep -Milliseconds 20
    $newestLiveLog = Join-Path $liveLogsDir "newer.log"
    Set-Content -Path $newestLiveLog -Value "newer live log"

    Invoke-ExpectOutput -Label "latest log helper resolves live profile log file" -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/run.ps1") -Arguments @("logs") -EnvVars @{
      APPDATA = $configBase
      WAVECRATE_CONFIG_HOME = ""
      WAVECRATE_CONFIG_PROFILE = ""
    } -ExpectedSubstrings @(
      "[latest_log] persistence_profile=live",
      "[latest_log] logs_dir=$liveLogsDir",
      "[latest_log] newest_log=$newestLiveLog",
      "newer live log"
    )

    $windowsPowerShell = Get-Command powershell -ErrorAction SilentlyContinue
    if ($null -ne $windowsPowerShell) {
      Invoke-ExpectOutput -Label "latest log helper resolves live profile log file in Windows PowerShell strict mode" -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/run.ps1") -Arguments @("logs") -PowerShellPath $windowsPowerShell.Path -EnvVars @{
        APPDATA = $configBase
        WAVECRATE_CONFIG_HOME = ""
        WAVECRATE_CONFIG_PROFILE = ""
      } -ExpectedSubstrings @(
        "[latest_log] persistence_profile=live",
        "[latest_log] logs_dir=$liveLogsDir",
        "[latest_log] newest_log=$newestLiveLog",
        "newer live log"
      )
    }

    Invoke-ExpectOutput -Label "run helper launches cargo debug overlays alias" -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/run.ps1") -Arguments @("logs", "debug-overlays") -EnvVars $fakeCargoEnv -ExpectedSubstrings @(
      "[run] launching cargo run with logs and debug layout overlays: cargo run -- --log --debug-overlays",
      "[cargo-fixture] cwd=$repoDir args=run -- --log --debug-overlays"
    )

    Invoke-ExpectOutput -Label "run helper launches cargo debug layout alias" -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/run.ps1") -Arguments @("logs", "debug-layout") -EnvVars $fakeCargoEnv -ExpectedSubstrings @(
      "[run] launching cargo run with logs and debug layout overlays: cargo run -- --log --debug-layout",
      "[cargo-fixture] cwd=$repoDir args=run -- --log --debug-layout"
    )

    Invoke-ExpectOutput -Label "root run helper delegates debug overlays alias" -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "run.ps1") -Arguments @("logs", "debug-overlays") -EnvVars $fakeCargoEnv -ExpectedSubstrings @(
      "[run] launching cargo run with logs and debug layout overlays: cargo run -- --log --debug-overlays",
      "[cargo-fixture] cwd=$repoDir args=run -- --log --debug-overlays"
    )

    Invoke-ExpectOutput -Label "root run helper delegates debug layout alias" -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "run.ps1") -Arguments @("logs", "debug-layout") -EnvVars $fakeCargoEnv -ExpectedSubstrings @(
      "[run] launching cargo run with logs and debug layout overlays: cargo run -- --log --debug-layout",
      "[cargo-fixture] cwd=$repoDir args=run -- --log --debug-layout"
    )

    $sandboxBase = Join-Path $repoDir ".sandbox/wavecrate"
    $sandboxDefaultRoot = Join-Path $sandboxBase ".wavecrate/profiles/sandbox"
    New-Item -ItemType Directory -Path $sandboxDefaultRoot -Force | Out-Null
    $overrideRoot = Join-Path $repoDir "sandbox-override"
    $overrideLogsDir = Join-Path $overrideRoot "logs"
    New-Item -ItemType Directory -Path $overrideLogsDir -Force | Out-Null
    Set-Content -Path (Join-Path $sandboxDefaultRoot "config.toml") -Value @(
      ('app_data_dir = "{0}"' -f $overrideRoot.Replace('\', '/'))
    )
    $overrideRootConfigStyle = $overrideRoot.Replace('\', '/')
    Set-Content -Path (Join-Path $overrideLogsDir "old.log") -Value "older sandbox log"
    Start-Sleep -Milliseconds 20
    $newestSandboxLog = Join-Path $overrideLogsDir "new.log"
    Set-Content -Path $newestSandboxLog -Value "newest sandbox log"

    Invoke-ExpectOutput -Label "latest log helper resolves sandbox profile and app_data_dir override" -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/run.ps1") -Arguments @("logs", "-Sandbox") -EnvVars @{
      APPDATA = $configBase
      WAVECRATE_CONFIG_HOME = ""
      WAVECRATE_CONFIG_PROFILE = ""
    } -ExpectedSubstrings @(
      "[latest_log] persistence_profile=sandbox",
      "[latest_log] app_root=$overrideRootConfigStyle",
      "[latest_log] logs_dir=$overrideLogsDir",
      "[latest_log] newest_log=$newestSandboxLog",
      "newest sandbox log"
    )
  } finally {
    Remove-Item -Recurse -Force $runHelperFixtureDir -ErrorAction SilentlyContinue
  }

  $migrationFixtureDir = New-TempDir
  try {
    $repoDir = Join-Path $migrationFixtureDir "repo"
    New-Item -ItemType Directory -Path $repoDir | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "src/app_core/tests") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "src/app_core/controller") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repoDir "scripts/internal/check") -Force | Out-Null

    Copy-Item (Join-Path $scriptsDir "internal/check/check_migration_boundary.ps1") (Join-Path $repoDir "scripts/internal/check/check_migration_boundary.ps1")

    Set-Content -Path (Join-Path $repoDir "src/app_core/app_api.rs") -Value @(
      "pub(crate) use crate::app::state::*;"
    )
    Set-Content -Path (Join-Path $repoDir "src/app_core/controller.rs") -Value @(
      "pub(crate) use crate::app_core::app_api::controller::AppController;"
    )
    Set-Content -Path (Join-Path $repoDir "src/app_core/controller/waveform_actions.rs") -Value @(
      "use crate::app_core::state::DestructiveSelectionEdit;"
    )
    Set-Content -Path (Join-Path $repoDir "src/app_core/tests/sample.rs") -Value @(
      "use crate::app::state::FocusContext;"
    )

    Invoke-ExpectExitCode -Label "migration boundary skips allowed and test paths" -ExpectedCode 0 -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/internal/check/check_migration_boundary.ps1")

    Set-Content -Path (Join-Path $repoDir "src/app_core/violation.rs") -Value @(
      "use crate::app::controller::StatusTone;"
    )
    Invoke-ExpectExitCode -Label "migration boundary fails on direct non-test path" -ExpectedCode 1 -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/internal/check/check_migration_boundary.ps1")
    Invoke-ExpectOutput -Label "migration boundary prints actionable violation lines" -ExpectedCode 1 -WorkDir $repoDir -ScriptPath (Join-Path $repoDir "scripts/internal/check/check_migration_boundary.ps1") -ExpectedSubstrings @(
      "Migration boundary check failed",
      "violation.rs:1:use crate::app::controller::StatusTone;",
      "Allowed app_core migration boundary location:"
    )
  } finally {
    Remove-Item -Recurse -Force $migrationFixtureDir -ErrorAction SilentlyContinue
  }

} finally {
  Pop-Location
}

Invoke-ExpectExitCode -Label "sandbox fixture dispatcher" -ExpectedCode 0 -WorkDir $rootDir -ScriptPath (Join-Path $scriptsDir "internal/run/test_sandbox_fixture_dispatch.ps1")

if ($failures -gt 0) {
  Write-Host "[guardrails] FAILED: $failures checks failed."
  exit 1
}

Write-Host "[guardrails] OK"
exit 0
