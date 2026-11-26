# ztrx launcher: runs the game for normal play (no scripted presses, no capture).
param(
    [Parameter(Mandatory = $true)][string]$Game,
    [string]$Cache = "$env:LOCALAPPDATA\ztrx\cache",
    [string]$Data = "$env:LOCALAPPDATA\ztrx\data",
    [string]$Exe = "$PSScriptRoot\..\target\release\victory.exe",
    [string]$TargetDir = ""
)

$RepoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$ExePath = [System.IO.Path]::GetFullPath($Exe)
$GamePath = [System.IO.Path]::GetFullPath($Game)

if (!(Test-Path -LiteralPath $ExePath -PathType Leaf)) {
    throw "Could not find VIctory executable: $ExePath. Build it first with: cargo build --release -p victory"
}
if (!(Test-Path -LiteralPath $GamePath -PathType Leaf)) {
    throw "Could not find the game executable: $GamePath"
}

# Read the pinned SDK version from xtask so this check cannot silently drift
# when the package version is updated.
$AgilitySource = Get-Content -LiteralPath (Join-Path $RepoRoot "xtask\src\agility.rs") -Raw
if ($AgilitySource -notmatch 'const VERSION: &str = "([^"]+)";') {
    throw "Could not read the pinned Agility SDK version from xtask\src\agility.rs"
}
$ExpectedSdkVersion = $Matches[1]

$ProfileDirectory = Split-Path -Parent $ExePath
$ProfileName = Split-Path -Leaf $ProfileDirectory
$RuntimeDirectory = Join-Path $ProfileDirectory "D3D12"
$CoreDll = Join-Path $RuntimeDirectory "D3D12Core.dll"
$LayersDll = Join-Path $RuntimeDirectory "d3d12SDKLayers.dll"
$VersionMarker = Join-Path $RuntimeDirectory "VICTORY_AGILITY_SDK_VERSION"

$RuntimeReady = $false
if ((Test-Path -LiteralPath $CoreDll -PathType Leaf) -and
    (Test-Path -LiteralPath $LayersDll -PathType Leaf) -and
    (Test-Path -LiteralPath $VersionMarker -PathType Leaf)) {
    $InstalledSdkVersion = (Get-Content -LiteralPath $VersionMarker -Raw).Trim()
    $RuntimeReady = $InstalledSdkVersion -eq $ExpectedSdkVersion
}

if (!$RuntimeReady) {
    if ($ProfileName -notin @("debug", "release", "diag")) {
        throw "Cannot infer a Cargo profile from '$ProfileDirectory'. Build into target\debug, target\release, or target\diag, or stage the Agility SDK manually beside victory.exe."
    }

    if (!(Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "The Agility SDK runtime is missing. Install Rust/Cargo, then run: cargo xtask agility"
    }

    if ([string]::IsNullOrWhiteSpace($TargetDir)) {
        $TargetRoot = Split-Path -Parent $ProfileDirectory
    } else {
        $TargetRoot = [System.IO.Path]::GetFullPath($TargetDir)
    }

    Write-Host "Preparing the pinned D3D12 Agility SDK $ExpectedSdkVersion beside victory.exe..." -ForegroundColor Cyan
    Push-Location $RepoRoot
    try {
        & cargo xtask agility --target-dir $TargetRoot
        $AgilityExitCode = $LASTEXITCODE
        if ($AgilityExitCode -ne 0) {
            throw "cargo xtask agility failed with exit code $AgilityExitCode. The game was not started."
        }
    } finally {
        Pop-Location
    }
}

if (!(Test-Path -LiteralPath $CoreDll -PathType Leaf) -or
    !(Test-Path -LiteralPath $LayersDll -PathType Leaf) -or
    !(Test-Path -LiteralPath $VersionMarker -PathType Leaf) -or
    (Get-Content -LiteralPath $VersionMarker -Raw).Trim() -ne $ExpectedSdkVersion) {
    throw "The matching D3D12 Agility SDK runtime is still missing after staging. The game was not started."
}

foreach ($v in "VICTORY_KEYS", "VICTORY_CAPTURE", "VICTORY_CAPTURE_FRAMES", "VICTORY_DUMP_TARGETS") {
    Remove-Item "Env:$v" -ErrorAction SilentlyContinue
}

# The experimental mesh fallback is memory- and CPU-intensive. Use a
# conservative VRAM shadow budget, parallelize shadow work, and overlap GPU
# submissions with copy-back. Explicit user values always take precedence.
if ($env:VICTORY_MESH_FALLBACK -eq "1") {
    $MeshFallbackDefaults = [ordered]@{
        VICTORY_DISPLAY_MODE = "1280x720"
        VICTORY_CARD_BUFFERS_MB = "256"
        VICTORY_CARD_REHASH_RUNS = "1"
        VICTORY_WIDE_BACK = "1"
        VICTORY_WIDE_MERGE = "1"
        VICTORY_RELAY = "1"
        VICTORY_LATE_WAIT_LISTS = "16"
    }
    foreach ($Name in $MeshFallbackDefaults.Keys) {
        if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($Name, "Process"))) {
            [Environment]::SetEnvironmentVariable($Name, $MeshFallbackDefaults[$Name], "Process")
        }
    }
    Write-Host "Mesh fallback profile: title mode $env:VICTORY_DISPLAY_MODE; parallel CPU copy/merge; async GPU submission; card-side buffer budget $env:VICTORY_CARD_BUFFERS_MB MB." -ForegroundColor Cyan
}
New-Item -ItemType Directory -Force $Cache, $Data | Out-Null
$env:VICTORY_CACHE_DIR = $Cache
$env:VICTORY_DATA_DIR = $Data

& $ExePath run $GamePath --execute 2>&1 | Out-File "$env:TEMP\ztrx.log" -Encoding utf8
$ExitCode = $LASTEXITCODE
Write-Host "Exited with $ExitCode; log: $env:TEMP\ztrx.log"
