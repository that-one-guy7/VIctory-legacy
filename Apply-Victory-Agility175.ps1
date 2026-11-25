# Apply-Victory-Agility175.ps1
# Makes a separate source copy of ztrx-main, then checks and applies upstream
# VIctory PR #175. It never edits the original backup and never launches the game.

$ErrorActionPreference = "Stop"

$Source = Join-Path $env:USERPROFILE "Downloads\ztrx-main\ztrx-main"
$Destination = Join-Path $env:USERPROFILE "Downloads\ztrx-main-agility175"
$PatchFile = Join-Path $env:TEMP "victory-pr175.patch"
$PatchUrl = "https://github.com/Marvisak/VIctory/pull/175.patch"

function Stop-Safely([string]$Message) {
    Write-Host ""
    Write-Host "STOP: $Message" -ForegroundColor Red
    Write-Host "No changes were applied to the original backup." -ForegroundColor Yellow
    exit 1
}

if (!(Test-Path (Join-Path $Source "Cargo.toml"))) {
    Stop-Safely "Could not find the source folder: $Source"
}

$requiredFiles = @(
    "apps\victory\src\main.rs",
    "crates\victory-shader\src\emit\host.rs",
    "xtask\src\main.rs",
    ".cargo\config.toml"
)
foreach ($relative in $requiredFiles) {
    if (!(Test-Path (Join-Path $Source $relative))) {
        Stop-Safely "The backup is missing expected file: $relative"
    }
}

if (!(Get-Command git -ErrorAction SilentlyContinue)) {
    Stop-Safely "Git for Windows (git.exe) is required for the safe patch check/apply step."
}

if (Test-Path $Destination) {
    Stop-Safely "The destination already exists: $Destination`nRename or remove that folder yourself, then rerun this script."
}

Write-Host "Creating a separate working copy..." -ForegroundColor Cyan
New-Item -ItemType Directory -Path $Destination | Out-Null

# Exclude generated build output and any Git metadata; source/config files remain.
& robocopy $Source $Destination /E /R:1 /W:1 /XD (Join-Path $Source "target") (Join-Path $Source ".git") /NFL /NDL /NJH /NJS
$copyExit = $LASTEXITCODE
if ($copyExit -ge 8) {
    Stop-Safely "Robocopy failed with exit code $copyExit. Inspect the new copy before continuing."
}

Write-Host "Downloading the exact PR #175 patch..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $PatchUrl -OutFile $PatchFile -UseBasicParsing

$patchText = Get-Content -LiteralPath $PatchFile -Raw
if ($patchText -notmatch "apps/victory/build\.rs" -or
    $patchText -notmatch "xtask/src/agility\.rs") {
    Stop-Safely "The downloaded file does not look like the expected PR #175 patch."
}

Write-Host "Checking whether PR #175 matches this backup..." -ForegroundColor Cyan
& git -C $Destination apply --check $PatchFile
if ($LASTEXITCODE -ne 0) {
    Stop-Safely "git apply --check found differences. The patch was NOT applied. Send me the complete output above."
}

Write-Host "Compatibility check passed. Applying PR #175..." -ForegroundColor Green
& git -C $Destination apply $PatchFile
if ($LASTEXITCODE -ne 0) {
    Stop-Safely "The patch application failed. The original backup remains untouched; inspect the copied folder."
}

$expectedNewFiles = @(
    "apps\victory\build.rs",
    "apps\victory\src\agility.rs",
    "xtask\src\agility.rs"
)
foreach ($relative in $expectedNewFiles) {
    if (!(Test-Path (Join-Path $Destination $relative))) {
        Stop-Safely "Patch command returned success, but expected file is missing: $relative"
    }
}

Write-Host ""
Write-Host "SUCCESS: PR #175 has been applied to the separate copy:" -ForegroundColor Green
Write-Host $Destination -ForegroundColor Green
Write-Host "Original backup left untouched:" -ForegroundColor Green
Write-Host $Source -ForegroundColor Green
Write-Host ""
Write-Host "Next: open PowerShell in the new folder and run the validation/build commands from the instructions."
