param(
    [string]$AppRoot,
    [string]$Onnx,
    [string]$OnnxUrl,
    [string]$RuntimeFile,
    [switch]$Force
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
$scriptPath = Join-Path $scriptDir "setup_panns.py"

$argsList = @($scriptPath)
if ($AppRoot) { $argsList += @("--app-root", $AppRoot) }
if ($Onnx) { $argsList += @("--onnx", $Onnx) }
if ($OnnxUrl) { $argsList += @("--onnx-url", $OnnxUrl) }
if ($RuntimeFile) { $argsList += @("--runtime-file", $RuntimeFile) }
if ($Force) { $argsList += "--force" }

& $python @argsList
