[CmdletBinding()]
param(
  [ValidateSet('locate', 'provision')]
  [string] $Action = 'locate',
  [string] $Root,
  [string] $Path,
  [switch] $Clean
)

$ErrorActionPreference = 'Stop'
function Fail([string] $Message) { throw "[radiant] ERROR: $Message" }
function MetadataValue([string] $Key, [string] $File) {
  $pattern = '^\s*' + [regex]::Escape($Key) + '\s*=\s*"'
  $line = Get-Content -LiteralPath $File | Where-Object { $_ -match $pattern } | Select-Object -First 1
  if (-not $line) { return '' }
  return (($line -replace '^.*?=\s*"', '') -replace '"\s*$', '')
}
if (-not $Root) {
  $Root = (git rev-parse --show-toplevel 2>$null)
}
if (-not $Root -or -not (Test-Path (Join-Path $Root 'Cargo.toml'))) { Fail 'could not identify a Wavecrate checkout; pass -Root' }
$Root = [IO.Path]::GetFullPath($Root)
$Metadata = Join-Path $Root 'radiant-dependency.toml'
if (-not (Test-Path $Metadata)) { Fail "missing $Metadata" }
$Repository = MetadataValue 'repository' $Metadata
$Revision = MetadataValue 'revision' $Metadata
$MetadataPath = MetadataValue 'path' $Metadata
if ($Revision -notmatch '^[0-9a-f]{40}$') { Fail 'metadata revision is not a full SHA' }
if (-not $Repository -or -not $MetadataPath) { Fail 'metadata repository/path is incomplete' }
if ($env:WAVECRATE_RADIANT_DIR) {
  Fail 'WAVECRATE_RADIANT_DIR is unsupported because Cargo is pinned to the paired ../radiant sibling; unset it and use the paired path'
}
function Resolve-SiblingPath([string] $Value) {
  if (-not [IO.Path]::IsPathRooted($Value)) { $Value = Join-Path $Root $Value }
  $parent = Split-Path -Parent $Value
  if (-not (Test-Path $parent -PathType Container)) { Fail "cannot resolve sibling parent directory: $parent" }
  return [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetFullPath($parent)) (Split-Path -Leaf $Value)))
}
$MetadataTarget = Resolve-SiblingPath $MetadataPath
$Target = if ($Path) { Resolve-SiblingPath $Path } else { $MetadataTarget }
if ($Target -ne $MetadataTarget) {
  Fail "Radiant path '$Target' does not match Cargo's configured sibling '$MetadataTarget'; use the paired ../radiant path"
}

function Validate-Checkout {
  if (-not (Test-Path $Target -PathType Container)) { Fail "Radiant sibling is missing: $Target (run scripts/radiant.ps1 provision)" }
  if (-not (Test-Path (Join-Path $Target 'Cargo.toml'))) { Fail "Radiant sibling has no Cargo.toml: $Target" }
  $manifest = Get-Content (Join-Path $Target 'Cargo.toml') -Raw
  if ($manifest -notmatch '(?m)^name\s*=\s*["'']radiant["'']') { Fail "Radiant sibling manifest is not package radiant: $Target" }
  git -C $Target rev-parse --git-dir *> $null
  if ($LASTEXITCODE -ne 0) { Fail "Radiant sibling is not a Git checkout: $Target" }
  $remote = (git -C $Target remote get-url origin 2>$null)
  if (-not $remote) { Fail "Radiant sibling has no origin remote: $Target" }
  $normalized = $remote -replace '\.git$', ''
  $expectedRepository = $Repository -replace '\.git$', ''
  if ($normalized -notin @($expectedRepository, 'https://github.com/PORTALSURFER/radiant', 'git@github.com:PORTALSURFER/radiant')) {
    Fail "Radiant sibling origin '$remote' does not match $Repository"
  }
}
function Print-State {
  $head = (git -C $Target rev-parse HEAD).Trim()
  $branch = (git -C $Target symbolic-ref --short -q HEAD 2>$null)
  if (-not $branch) { $branch = 'detached' }
  $dirty = if ((git -C $Target status --porcelain --untracked-files=normal)) { 'dirty' } else { 'clean' }
  $match = if ($head -eq $Revision) { 'yes' } else { 'no' }
  "RADIANT_DIR=$Target"
  "RADIANT_HEAD=$head"
  "RADIANT_BRANCH=$branch"
  "RADIANT_STATE=$dirty"
  "RADIANT_REVISION_MATCH=$match"
}
function Provision {
  if (Test-Path $Target) {
    Validate-Checkout
    if (-not $Clean) { Write-Host "[radiant] existing sibling preserved (no reset/clean/pull): $Target"; Print-State; return }
    if (git -C $Target status --porcelain --untracked-files=normal) { Fail "refusing to mutate dirty Radiant sibling for clean provisioning: $Target" }
    git -C $Target fetch --no-tags origin $Revision
    git -C $Target checkout --detach $Revision
  } else {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Target) | Out-Null
    $key = if ($env:RADIANT_REPOSITORY_DEPLOY_KEY) { $env:RADIANT_REPOSITORY_DEPLOY_KEY } else { $env:RADIANT_SUBMODULE_DEPLOY_KEY }
    $oldSsh = $env:GIT_SSH_COMMAND
    $keyFile = $null
    $cloneUrl = $Repository
    try {
      if ($key) {
        $keyFile = Join-Path ([IO.Path]::GetTempPath()) ("radiant-key-{0}.pem" -f ([guid]::NewGuid()))
        Set-Content -LiteralPath $keyFile -Value $key -NoNewline
        $env:GIT_SSH_COMMAND = "ssh -i `"$keyFile`" -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new"
        $cloneUrl = 'git@github.com:PORTALSURFER/radiant.git'
      }
      git clone --no-checkout $cloneUrl $Target
      git -C $Target fetch --no-tags origin $Revision
      git -C $Target checkout --detach $Revision
    } finally {
      $env:GIT_SSH_COMMAND = $oldSsh
      if ($keyFile) { Remove-Item -LiteralPath $keyFile -Force -ErrorAction SilentlyContinue }
    }
  }
  Validate-Checkout
  if ((git -C $Target rev-parse HEAD).Trim() -ne $Revision) { Fail 'Radiant HEAD mismatch after provisioning' }
  Print-State
}
if ($Action -eq 'provision') { Provision } else { Validate-Checkout; Print-State }
