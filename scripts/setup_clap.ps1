param(
    [string]$AppRoot,
    [switch]$NoInstall,
    [switch]$Force,
    [string]$RuntimeUrl,
    [string]$RuntimeFile,
    [string]$OrtVersion,
    [string]$OrtFlavor,
    [string]$Checkpoint,
    [string]$CheckpointUrl,
    [switch]$NoCheckpointDownload,
    [int]$SampleRate,
    [double]$Seconds,
    [int]$Channels,
    [int]$Opset
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
$scriptPath = Join-Path $scriptDir "setup_clap.py"

$argsList = @($scriptPath)
if ($AppRoot) { $argsList += @("--app-root", $AppRoot) }
if ($NoInstall) { $argsList += "--no-install" }
if ($Force) { $argsList += "--force" }
if ($RuntimeUrl) { $argsList += @("--runtime-url", $RuntimeUrl) }
if ($RuntimeFile) { $argsList += @("--runtime-file", $RuntimeFile) }
if ($OrtVersion) { $argsList += @("--ort-version", $OrtVersion) }
if ($OrtFlavor) { $argsList += @("--ort-flavor", $OrtFlavor) }
if ($Checkpoint) { $argsList += @("--checkpoint", $Checkpoint) }
if ($CheckpointUrl) { $argsList += @("--checkpoint-url", $CheckpointUrl) }
if ($NoCheckpointDownload) { $argsList += "--no-checkpoint-download" }
if ($SampleRate) { $argsList += @("--sample-rate", $SampleRate) }
if ($Seconds) { $argsList += @("--seconds", $Seconds) }
if ($Channels) { $argsList += @("--channels", $Channels) }
if ($Opset) { $argsList += @("--opset", $Opset) }

& $python @argsList
