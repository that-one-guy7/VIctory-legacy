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

# The runtime can select compute/vertex mesh fallback automatically when the
# adapter lacks native mesh shaders. The launcher cannot know that decision
# before D3D12 device creation, so the diagnostic profile also gets the
# conservative low-VRAM defaults used by this GTX 1050 Ti crash-repro setup.
# Explicit user values always take precedence; release launches are unchanged
# unless VICTORY_MESH_FALLBACK=1 is explicitly requested.
$UseLowVramProfile = ($env:VICTORY_MESH_FALLBACK -eq "1") -or ($ProfileName -eq "diag")
if ($UseLowVramProfile) {
    $MeshFallbackDefaults = [ordered]@{
        VICTORY_DISPLAY_MODE = "1280x720"
        VICTORY_CARD_BUFFERS = "0"
        VICTORY_CARD_REHASH_RUNS = "1"
        VICTORY_WIDE_BACK = "1"
        VICTORY_WIDE_MERGE = "1"
        VICTORY_WIDE_REFRESH = "1"
        # This profile is intentionally aggressive for 4 GB-class adapters.
        # These only affect eligible host-side resources; the title's own
        # resource descriptions and required mesh/indirect allocations remain intact.
        VICTORY_RENDER_SCALE = "1.0"
        VICTORY_RENDER_SCALE_MIN_SIZE = "512"
        VICTORY_TEXTURE_MIP_DROP = "2"
        VICTORY_TEXTURE_MIP_DROP_MIN_SIZE = "1024"
        # The regular window grid is 4 MiB; use 1 MiB for length-bounded host copies.
        VICTORY_WINDOW_KB = "1024"
        VICTORY_MESH_LOCAL_BUDGET_MB = "128"
        # Keep only a small idle shared-scratch cache on systems with limited RAM.
        VICTORY_MESH_SHARED_SPARE_MB = "32"
        # Leave CPU headroom for the title instead of saturating all logical processors.
        VICTORY_DSTORAGE_WORKERS = "3"
        VICTORY_PIPELINE_WORKERS = "2"
        # Let the CPU prepare the next frame while the GPU finishes the previous present.
        # Stage 3 skips the per-present black-frame diagnostic readback; explicit captures still synchronize.
        VICTORY_LATE_WAIT = "3"
        # The GTX 1050 Ti run reached 138 lists / 15 submissions ahead and exceeded its local-memory budget.
        # Limit the queued backlog without disabling asynchronous CPU/GPU overlap.
        VICTORY_LATE_WAIT_LISTS = "64"
        # Copy shader-written title buffers back on a worker while submissions continue.
        VICTORY_RELAY = "2"
        VICTORY_LOCAL_UAV_BUDGET_PERCENT = "70"
    }
    foreach ($Name in $MeshFallbackDefaults.Keys) {
        if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($Name, "Process"))) {
            [Environment]::SetEnvironmentVariable($Name, $MeshFallbackDefaults[$Name], "Process")
        }
    }
    Write-Host "Low-VRAM launch profile: title mode $env:VICTORY_DISPLAY_MODE; render scale=$env:VICTORY_RENDER_SCALE; texture mip drop=$env:VICTORY_TEXTURE_MIP_DROP; mesh-local scratch cap=$env:VICTORY_MESH_LOCAL_BUDGET_MB MiB; shared scratch cache=$env:VICTORY_MESH_SHARED_SPARE_MB MiB; DirectStorage workers=$env:VICTORY_DSTORAGE_WORKERS; pipeline workers=$env:VICTORY_PIPELINE_WORKERS; optional UAV threshold=$env:VICTORY_LOCAL_UAV_BUDGET_PERCENT%; GPU-local title-buffer copies=$env:VICTORY_CARD_BUFFERS." -ForegroundColor Cyan
    if ($env:VICTORY_MESH_SHARED_SPARE_MB -ne "32") {
        Write-Warning "VICTORY_MESH_SHARED_SPARE_MB=$env:VICTORY_MESH_SHARED_SPARE_MB is an explicit override; the low-VRAM preset is 32 MiB. Remove the process override to use the preset."
    }
    Write-Host "GPU overlap: VICTORY_LATE_WAIT=$env:VICTORY_LATE_WAIT; copy-back relay=$env:VICTORY_RELAY (stage 3 skips the per-present black-frame diagnostic readback)." -ForegroundColor Cyan
    Write-Host "Length-bounded title-memory window grid: $env:VICTORY_WINDOW_KB KiB; unsized root views remain 4 MiB." -ForegroundColor Cyan
}

# A failed command-list Close may hide invalid D3D12 commands. Enable the debug
# layer for every diagnostics executable, including when mesh fallback was
# selected automatically. GPU-based validation remains opt-in because it has
# much higher memory and runtime overhead.
if ($ProfileName -eq "diag" -and
    [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable("VICTORY_D3D12_DEBUG", "Process"))) {
    $env:VICTORY_D3D12_DEBUG = "1"
}

# DRED is enabled automatically for diagnostics executables so a device
# removal includes command breadcrumbs and page-fault details. This is
# diagnostics-only; release launches keep the default off.
if ($ProfileName -eq "diag" -and
    [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable("VICTORY_D3D12_DRED", "Process"))) {
    $env:VICTORY_D3D12_DRED = "1"
}
if ($ProfileName -eq "diag") {
    Write-Host "D3D12 diagnostics: DRED=$env:VICTORY_D3D12_DRED; debug layer=$env:VICTORY_D3D12_DEBUG." -ForegroundColor Cyan
}
New-Item -ItemType Directory -Force $Cache, $Data | Out-Null
$env:VICTORY_CACHE_DIR = $Cache
$env:VICTORY_DATA_DIR = $Data

# Write a compact, reproducible diagnostic header. Keep this to VICTORY_*
# settings rather than dumping the entire process environment (which may
# contain credentials or unrelated personal data).
$RuntimeLog = Join-Path $env:TEMP "ztrx.log"
$Report = [System.Collections.Generic.List[string]]::new()
$Report.Add("VIctory launcher diagnostic report")
$Report.Add("timestamp_local=$([DateTimeOffset]::Now.ToString('o'))")
$Report.Add("executable=$ExePath")
$Report.Add("game=$GamePath")
$Report.Add("profile=$ProfileName")
$Report.Add("low_vram_profile=$UseLowVramProfile")
$Report.Add("agility_sdk_expected=$ExpectedSdkVersion")
$Report.Add("agility_sdk_installed=$((Get-Content -LiteralPath $VersionMarker -Raw).Trim())")
$Report.Add("powershell=$($PSVersionTable.PSVersion)")
$Report.Add("os=$([Environment]::OSVersion.VersionString)")

# Include every effective wrapper setting, including newly added switches.
# This is especially useful when a previous PowerShell session left overrides
# active; the launcher deliberately does not erase explicit user choices.
$Report.Add("")
$Report.Add("[effective VICTORY_* environment]")
[Environment]::GetEnvironmentVariables("Process").GetEnumerator() |
    Where-Object { $_.Key -like "VICTORY_*" } |
    Sort-Object Key |
    ForEach-Object { $Report.Add("$($_.Key)=$($_.Value)") }

# Hardware data helps interpret memory failures and driver-specific behavior.
# CIM is best-effort: a restricted Windows installation must still launch.
try {
    $OsInfo = Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop
    $Report.Add("")
    $Report.Add("[system memory]")
    $Report.Add("physical_memory_total_mb=$([math]::Round($OsInfo.TotalVisibleMemorySize / 1024))")
    $Report.Add("physical_memory_free_mb=$([math]::Round($OsInfo.FreePhysicalMemory / 1024))")
} catch {
    $Report.Add("system_memory_query=unavailable ($($_.Exception.Message))")
}
try {
    $CpuInfo = Get-CimInstance -ClassName Win32_Processor -ErrorAction Stop |
        Select-Object -First 1
    $Report.Add("")
    $Report.Add("[processor]")
    $Report.Add("cpu=$($CpuInfo.Name)")
    $Report.Add("cpu_logical_processors=$($CpuInfo.NumberOfLogicalProcessors)")
} catch {
    $Report.Add("processor_query=unavailable ($($_.Exception.Message))")
}
try {
    $GpuInfo = @(Get-CimInstance -ClassName Win32_VideoController -ErrorAction Stop)
    $Report.Add("")
    $Report.Add("[display adapters]")
    foreach ($Gpu in $GpuInfo) {
        $Report.Add("gpu=$($Gpu.Name); driver=$($Gpu.DriverVersion); adapter_memory_bytes=$($Gpu.AdapterRAM)")
    }
} catch {
    $Report.Add("display_adapter_query=unavailable ($($_.Exception.Message))")
}

[System.IO.File]::WriteAllLines(
    $RuntimeLog,
    $Report,
    [System.Text.UTF8Encoding]::new($false)
)

& $ExePath run $GamePath --execute 2>&1 | Out-File $RuntimeLog -Append -Encoding utf8
$ExitCode = $LASTEXITCODE
Add-Content -LiteralPath $RuntimeLog -Value @("", "[launcher exit]", "exit_code=$ExitCode", "finished_local=$([DateTimeOffset]::Now.ToString('o'))") -Encoding utf8
Write-Host "Exited with $ExitCode; log: $RuntimeLog"
