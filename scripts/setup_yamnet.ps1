param(
    [string]$AppRoot,
    [switch]$NoInstall,
    [switch]$Force,
    [string]$RuntimeUrl
)

$ErrorActionPreference = "Stop"

function Find-Python {
    $pythonCmd = Get-Command python -ErrorAction SilentlyContinue
    if ($pythonCmd) { return $pythonCmd.Source }
    $python3Cmd = Get-Command python3 -ErrorAction SilentlyContinue
    if ($python3Cmd) { return $python3Cmd.Source }
    throw "Python not found on PATH. Install Python 3.10+ or add it to PATH."
}

$python = Find-Python
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$scriptPath = Join-Path $scriptDir "setup_yamnet.py"

$argsList = @($scriptPath)
if ($AppRoot) { $argsList += @("--app-root", $AppRoot) }
if ($NoInstall) { $argsList += "--no-install" }
if ($Force) { $argsList += "--force" }
if ($RuntimeUrl) { $argsList += @("--runtime-url", $RuntimeUrl) }

& $python @argsList
