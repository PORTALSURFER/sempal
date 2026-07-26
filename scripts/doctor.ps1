Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

<#
.SYNOPSIS
Environment sanity checks for local development and agent runs.

.DESCRIPTION
Checks the common footguns called out in README:
- `CPAL_ASIO_DIR` (Windows ASIO builds)
- `WAVECRATE_NATIVE_FONT_PATH` (UI projection font override)
- presence of `git lfs`
Also checks toolchain sanity:
- pinned Rust toolchain vs rust-toolchain.toml
- rustfmt/clippy present for the pinned toolchain
- presence of `rg` (ripgrep)
Also prints developer diagnostic `.wavecrate/logs` locations for each OS.
Linux/WSL paths are contributor/agent tooling hints, not shipped app support.
#>

$failures = 0
$warnings = 0

function Write-Info([string]$Message) { Write-Host "[doctor] $Message" }
function Write-Warn([string]$Message) { Write-Warning "[doctor][warn] $Message"; $script:warnings++ }
function Write-Err([string]$Message) { Write-Error "[doctor][error] $Message"; $script:failures++ }

function Get-PlatformFlag([string]$Name, [bool]$Fallback) {
  $value = Get-Variable -Name $Name -Scope Global -ErrorAction SilentlyContinue
  if ($null -ne $value) {
    return [bool]$value.Value
  }
  return $Fallback
}

function Write-BootstrapHint {
  Write-Warn "Run bootstrap to install pinned toolchain + tools:"
  Write-Warn "  - bash scripts/bootstrap.sh"
  Write-Warn "  - powershell -ExecutionPolicy Bypass -File scripts/bootstrap.ps1"
}

$rootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Write-Info "Repo: $rootDir"

Write-Info "Checking canonical Radiant sibling..."
try {
  & (Join-Path $rootDir 'scripts/radiant.ps1') locate
  if ($LASTEXITCODE -ne 0) { throw "Radiant locator failed" }
  Write-Info "Radiant sibling: available"
} catch {
  Write-Err "Radiant sibling is missing or invalid; run scripts/bootstrap.ps1 to provision it"
}

$isWindowsHost = Get-PlatformFlag -Name "IsWindows" -Fallback ([System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT)
$isMacOsHost = Get-PlatformFlag -Name "IsMacOS" -Fallback $false
$isLinuxHost = Get-PlatformFlag -Name "IsLinux" -Fallback ((-not $isWindowsHost) -and (-not $isMacOsHost))

$os =
  if ($isWindowsHost) { "windows" }
  elseif ($isMacOsHost) { "macos" }
  elseif ($isLinuxHost) { "linux" }
  else { "unknown" }
Write-Info "OS: $os"

Write-Info "Checking ripgrep (rg)..."
$rg = Get-Command rg -ErrorAction SilentlyContinue
if ($null -ne $rg) {
  Write-Info "rg: present"
} else {
  Write-Warn "rg: missing (recommended; used by several repo checks)"
  Write-BootstrapHint
}

Write-Info "Checking Rust toolchain (pinned)..."
$toolchainToml = Join-Path $rootDir "rust-toolchain.toml"
if (-not (Test-Path -LiteralPath $toolchainToml)) {
  Write-Warn "rust-toolchain.toml not found at repo root (can't verify toolchain pin)"
} else {
  $rustup = Get-Command rustup -ErrorAction SilentlyContinue
  if ($null -eq $rustup) {
    Write-Err "rustup not found on PATH (can't verify/install pinned toolchain)"
    Write-BootstrapHint
  } else {
    $channel = "stable"
    foreach ($line in Get-Content -LiteralPath $toolchainToml) {
      if ($line -match '^\s*channel\s*=\s*"([^"]+)"\s*$') {
        $channel = $Matches[1]
        break
      }
    }
    Write-Info "Pinned toolchain channel: $channel"

    # Verify toolchain exists by trying to run rustc under it.
    try {
      rustup run $channel rustc -V | Out-Null
      Write-Info "Pinned toolchain installed: yes"
    } catch {
      Write-Err "Pinned toolchain is not installed: $channel"
      Write-BootstrapHint
    }

    # Warn if active toolchain doesn't match the pin.
    try {
      $active = (rustup show active-toolchain | Select-Object -First 1)
      $activeTok = ($active -split '\s+')[0]
      if (-not [string]::IsNullOrWhiteSpace($activeTok)) {
        if ($activeTok.StartsWith($channel)) {
          Write-Info "Active toolchain: $activeTok (matches pin)"
        } else {
          Write-Warn "Active toolchain: $activeTok (does not match pin: $channel)"
          Write-Warn "Consider: rustup default $channel"
        }
      } else {
        Write-Warn "Could not determine active toolchain via rustup"
      }
    } catch {
      Write-Warn "Could not determine active toolchain via rustup"
    }

    # Verify components exist for pinned toolchain.
    try {
      $installed = rustup component list --toolchain $channel --installed
      $hasFmt = $false
      $hasClippy = $false
      foreach ($l in $installed) {
        if ($l -match '^(rustfmt)') { $hasFmt = $true }
        if ($l -match '^(clippy)') { $hasClippy = $true }
      }
      if ($hasFmt) {
        Write-Info "rustfmt: installed (toolchain $channel)"
      } else {
        Write-Err "rustfmt is not installed for toolchain $channel"
        Write-BootstrapHint
      }
      if ($hasClippy) {
        Write-Info "clippy: installed (toolchain $channel)"
      } else {
        Write-Err "clippy is not installed for toolchain $channel"
        Write-BootstrapHint
      }
    } catch {
      Write-Warn "Could not query rustup components for toolchain $channel"
      Write-BootstrapHint
    }
  }
}

Write-Info "Checking Git LFS..."
$git = Get-Command git -ErrorAction SilentlyContinue
if ($null -eq $git) {
  Write-Warn "git not found on PATH"
} else {
  try {
    git lfs version | Out-Null
    Write-Info "Git LFS: present"
  } catch {
    Write-Warn "Git LFS: missing (install git-lfs if you see checkout/build issues with large assets)"
  }
}

Write-Info "Checking WAVECRATE_NATIVE_FONT_PATH..."
$fontPath = $env:WAVECRATE_NATIVE_FONT_PATH
if ([string]::IsNullOrWhiteSpace($fontPath)) {
  Write-Info "WAVECRATE_NATIVE_FONT_PATH: not set (OK)"
} elseif (Test-Path -LiteralPath $fontPath -PathType Leaf) {
  Write-Info "WAVECRATE_NATIVE_FONT_PATH: OK ($fontPath)"
} else {
  Write-Err "WAVECRATE_NATIVE_FONT_PATH is set but not a file: $fontPath"
}

Write-Info "Checking CPAL_ASIO_DIR (Windows ASIO builds)..."
$asioDir = $env:CPAL_ASIO_DIR
if ([string]::IsNullOrWhiteSpace($asioDir)) {
  Write-Info "CPAL_ASIO_DIR: not set (OK unless building Windows ASIO support)"
} elseif (-not (Test-Path -LiteralPath $asioDir -PathType Container)) {
  Write-Err "CPAL_ASIO_DIR is set but not a directory: $asioDir"
} else {
  $hostDir = Join-Path $asioDir "host"
  $commonDir = Join-Path $asioDir "common"
  if ((Test-Path -LiteralPath $hostDir -PathType Container) -and (Test-Path -LiteralPath $commonDir -PathType Container)) {
    Write-Info "CPAL_ASIO_DIR: looks like an ASIO SDK root ($asioDir)"
  } else {
    Write-Err "CPAL_ASIO_DIR exists but doesn't look like ASIO SDK root (expected host/ and common/): $asioDir"
  }
}

Write-Info "Expected developer diagnostic log locations:"
Write-Info "  Supported app installs:"
Write-Info "    macOS:   `$HOME/Library/Application Support/.wavecrate/logs"
if ($isWindowsHost -and -not [string]::IsNullOrWhiteSpace($env:APPDATA)) {
  $windowsLogDir = Join-Path (Join-Path $env:APPDATA ".wavecrate") "logs"
  Write-Info ("    Windows: {0}" -f $windowsLogDir)
} else {
  Write-Info "    Windows: %APPDATA%\.wavecrate\logs"
}
Write-Info "  Developer/agent tooling only; Linux is not a shipped app install target:"
Write-Info "    Linux:   `$HOME/.config/.wavecrate/logs"
if ($isLinuxHost -and (Test-Path -LiteralPath "/proc/version")) {
  try {
    $procVersion = Get-Content -LiteralPath "/proc/version" -Raw
    if ($procVersion -match "(?i)microsoft") {
      Write-Info "    WSL hint: /mnt/c/Users/<you>/AppData/Roaming/.wavecrate/logs"
    }
  } catch {
    # best-effort WSL detection; ignore errors
  }
}

if ($failures -gt 0) {
  Write-Info "Result: FAIL ($failures errors, $warnings warnings)"
  exit 1
}

Write-Info "Result: OK ($warnings warnings)"
exit 0
