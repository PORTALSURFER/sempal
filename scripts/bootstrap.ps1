Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

<#
.SYNOPSIS
Bootstrap a known-good local dev environment (humans + agents).

.DESCRIPTION
- Installs the pinned Rust toolchain from rust-toolchain.toml
- Ensures rustfmt/clippy/cargo-nextest are available
- Detects optional local `sccache` support for explicit opt-in Cargo caching
- Checks git-lfs and Python
- Prints next-step commands for smoke, quick, and full validation tiers
#>

$verifyOnly = $false
if ($args -contains "--verify-only") {
  $verifyOnly = $true
  $args = @($args | Where-Object { $_ -ne "--verify-only" })
}
if ($args -contains "-h" -or $args -contains "--help") {
  Write-Host "Usage: scripts/bootstrap.ps1 [--verify-only]"
  Write-Host ""
  Write-Host "Default: installs/ensures a known-good local environment (pinned toolchain + rustfmt/clippy/cargo-nextest)."
  Write-Host "--verify-only: performs checks only (no installs); exits non-zero if missing."
  exit 0
}

function Install-AgentPreflightHooks {
  if ($env:WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK_INSTALL -eq "1") {
    Write-Host "[bootstrap] WAVECRATE_SKIP_AGENT_PREFLIGHT_HOOK_INSTALL=1: skipping hook install."
    return $true
  }

  $hookInstaller = Join-Path $rootDir "scripts/agent.sh"
  if (-not (Test-Path -LiteralPath $hookInstaller)) {
    Write-Host "[bootstrap] ERROR: missing hook installer at $hookInstaller" -ForegroundColor Red
    return $false
  }

  $bash = Get-Command bash -ErrorAction SilentlyContinue
  if ($null -eq $bash) {
    Write-Host "[bootstrap] ERROR: bash not found; required to install agent preflight hooks." -ForegroundColor Red
    Write-Host "[bootstrap] Install a bash-enabled shell (Git Bash/WSL)."
    return $false
  }

  & $bash.Path $hookInstaller install-hooks --force
  if ($LASTEXITCODE -ne 0) {
    Write-Host "[bootstrap] ERROR: hook installer failed with code $LASTEXITCODE." -ForegroundColor Red
    return $false
  }
  Write-Host "[bootstrap] agent preflight hooks: installed"
  return $true
}

$failures = 0

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $rootDir
try {
  Write-Host "[bootstrap] repo: $rootDir"

  $radiantScript = Join-Path $rootDir 'scripts/radiant.ps1'
  try {
    if ($verifyOnly) {
      & $radiantScript locate
      Write-Host "[bootstrap] Radiant sibling: available"
    } else {
      Write-Host "[bootstrap] Provisioning/checking the canonical Radiant sibling..."
      & $radiantScript provision
      if ($LASTEXITCODE -ne 0) { throw "Radiant sibling provisioning failed" }
    }
  } catch {
    if ($verifyOnly) {
      Write-Warning "[bootstrap] Radiant sibling: missing or invalid (run without -VerifyOnly to provision)"
      $failures++
    } else {
      throw
    }
  }

  if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    throw "[bootstrap] ERROR: git not found on PATH"
  }

  $rg = Get-Command rg -ErrorAction SilentlyContinue
  if ($null -ne $rg) {
    Write-Host "[bootstrap] rg: OK"
  } else {
    Write-Host "[bootstrap] rg: MISSING (recommended; several repo checks use it)"
    Write-Host "[bootstrap]   Install ripgrep (rg). Examples:"
    Write-Host "[bootstrap]     Windows: winget install BurntSushi.ripgrep.MSVC"
    Write-Host "[bootstrap]     Windows (alt): choco install ripgrep"
    if ($verifyOnly) { $failures++ }
  }

  $hasGitLfs = $false
  try {
    git lfs version | Out-Null
    $hasGitLfs = $true
  } catch {
    $hasGitLfs = $false
  }

  if ($hasGitLfs) {
    Write-Host "[bootstrap] git-lfs: OK"
    try { git lfs install --local | Out-Null } catch { }
  } else {
    Write-Host "[bootstrap] git-lfs: MISSING (recommended)"
    Write-Host "[bootstrap]   Install git-lfs and run: git lfs install --local"
  }

  # Prefer python3, then py -3, then python.
  $pythonCmd = $null
  foreach ($candidate in @("python3", "py", "python")) {
    $cmd = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($null -ne $cmd) { $pythonCmd = $candidate; break }
  }

  if ($null -ne $pythonCmd) {
    try {
      if ($pythonCmd -eq "py") {
        $ver = (& py -3 -c "import sys; print('.'.join(map(str, sys.version_info[:3])))" 2>$null)
        Write-Host "[bootstrap] python: OK (py -3 $ver)"
      } else {
        $ver = (& $pythonCmd -c "import sys; print('.'.join(map(str, sys.version_info[:3])))" 2>$null)
        Write-Host "[bootstrap] python: OK ($pythonCmd $ver)"
      }
    } catch {
      Write-Host "[bootstrap] python: present (version unknown)"
    }
  } else {
    Write-Host "[bootstrap] python: MISSING (recommended; used by some tooling/scripts)"
  }

  if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    throw "[bootstrap] ERROR: rustup not found on PATH. Install from https://rustup.rs/ and re-run."
  }

  $toolchainFile = Join-Path $rootDir "rust-toolchain.toml"
  if (-not (Test-Path -LiteralPath $toolchainFile)) {
    throw "[bootstrap] ERROR: rust-toolchain.toml not found at repo root"
  }

  # Parse pinned channel with a simple regex (works without tomllib).
  $channel = "stable"
  foreach ($line in Get-Content -LiteralPath $toolchainFile) {
    if ($line -match '^\s*channel\s*=\s*"([^"]+)"\s*$') {
      $channel = $Matches[1]
      break
    }
  }

  Write-Host "[bootstrap] rust toolchain (pinned): $channel"

  $toolchainInstalled = $false
  try {
    rustup run $channel rustc -V | Out-Null
    $toolchainInstalled = $true
    Write-Host "[bootstrap] pinned toolchain installed: yes"
  } catch {
    Write-Host "[bootstrap] pinned toolchain installed: no"
    if ($verifyOnly) {
      $failures++
    } else {
      Write-Host "[bootstrap] rustup toolchain install $channel"
      rustup toolchain install $channel --profile minimal
    }
  }

  $installed = @()
  try {
    $installed = rustup component list --toolchain $channel --installed
  } catch {
    $installed = @()
  }

  $hasFmt = $false
  $hasClippy = $false
  foreach ($l in $installed) {
    if ($l -match '^(rustfmt)') { $hasFmt = $true }
    if ($l -match '^(clippy)') { $hasClippy = $true }
  }

  if ($hasFmt) {
    Write-Host "[bootstrap] rustfmt: installed"
  } else {
    Write-Host "[bootstrap] rustfmt: missing"
    if ($verifyOnly) {
      $failures++
    } else {
      Write-Host "[bootstrap] rustup component add rustfmt --toolchain $channel"
      rustup component add rustfmt --toolchain $channel
    }
  }

  if ($hasClippy) {
    Write-Host "[bootstrap] clippy: installed"
  } else {
    Write-Host "[bootstrap] clippy: missing"
    if ($verifyOnly) {
      $failures++
    } else {
      Write-Host "[bootstrap] rustup component add clippy --toolchain $channel"
      rustup component add clippy --toolchain $channel
    }
  }

  $hasNextest = $false
  try {
    cargo nextest --version | Out-Null
    $hasNextest = $true
  } catch {
    $hasNextest = $false
  }

  if ($hasNextest) {
    Write-Host "[bootstrap] cargo-nextest: installed"
  } else {
    Write-Host "[bootstrap] cargo-nextest: missing"
    if ($verifyOnly) {
      $failures++
    } else {
      Write-Host "[bootstrap] cargo install cargo-nextest --locked"
      cargo install cargo-nextest --locked
    }
  }

  $sccache = Get-Command sccache -ErrorAction SilentlyContinue
  if ($null -ne $sccache) {
    Write-Host "[bootstrap] sccache: installed (set WAVECRATE_ENABLE_SCCACHE=1 to use it)"
  } else {
    Write-Host "[bootstrap] sccache: optional but missing"
    Write-Host "[bootstrap]   Install for faster repeated builds. Examples:"
    Write-Host "[bootstrap]     cargo install sccache --locked"
    Write-Host "[bootstrap]     winget install Mozilla.sccache"
  }

  Write-Host ""
  Write-Host "[bootstrap] Next steps:"
  Write-Host "  - Environment sanity:   powershell -ExecutionPolicy Bypass -File scripts/doctor.ps1"
  Write-Host "  - App-only check:       powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 smoke -AppOnly"
  Write-Host "  - Workspace smoke:      powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 smoke -Workspace"
  Write-Host "  - Smoke devcheck:       powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 smoke"
  Write-Host "  - Fast test checks:     powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 quick"
  Write-Host "  - CI parity checks:     powershell -ExecutionPolicy Bypass -File scripts/ci.ps1 local"
  Write-Host "  - Safe local run:       powershell -ExecutionPolicy Bypass -File scripts/run.ps1 sandbox --"

  if ($verifyOnly) {
    if ($failures -gt 0) {
      Write-Error ("[bootstrap] Result: FAIL ({0} missing requirements). Hint: run without --verify-only to install." -f $failures)
      exit 1
    }
    Write-Host "[bootstrap] Result: OK"
  } else {
    if (-not (Install-AgentPreflightHooks)) {
      Write-Host "[bootstrap] Agent workspace setup is incomplete without the preflight hooks."
      Write-Host "[bootstrap] Run manually: bash scripts/agent.sh install-hooks --force"
      Write-Error "[bootstrap] Result: FAIL"
      exit 1
    }
    Write-Host "[bootstrap] Result: OK"
  }
} finally {
  Pop-Location
}
