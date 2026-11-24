# Build a portable VFM bundle (and optionally the installer).
#
#   powershell -ExecutionPolicy Bypass -File tools/package.ps1
#   powershell -ExecutionPolicy Bypass -File tools/package.ps1 -SkipBuild
#
# The libVLC runtime is copied as loose files on purpose: the LGPL requires that a user can
# replace libvlc.dll / libvlccore.dll / plugins with their own build.

param(
    [switch]$SkipBuild,
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches.Groups[1].Value
if (-not $version) { $version = "0.0.0" }

$dist = Join-Path $root "dist\VFM-$version"
# Either build directory may hold the newest binary (see install.ps1).
$exe = @(
    (Join-Path $root "target\$Configuration\vfm-spike.exe"),
    (Join-Path $root "target2\$Configuration\vfm-spike.exe")
) | Where-Object { Test-Path $_ } |
    Sort-Object { (Get-Item $_).LastWriteTime } -Descending |
    Select-Object -First 1
$libvlc = Join-Path $root "vendor\libvlc"

if (-not $SkipBuild) {
    Write-Host "Building $Configuration…"
    & cargo build --release -p vfm-app
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

if (-not (Test-Path $exe)) { throw "missing $exe" }
if (-not (Test-Path $libvlc)) { throw "missing libVLC runtime in vendor\libvlc" }

if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Path $dist | Out-Null

Copy-Item $exe (Join-Path $dist "vfm-spike.exe")

# Runtime: DLLs plus the whole plugins tree, kept as separate files.
Copy-Item (Join-Path $libvlc "*.dll") $dist -ErrorAction SilentlyContinue

# The executable imports sqlite3.dll at load time; without it the bundle will not start.
$sqlite = Join-Path $root "vendor\sqlite\lib\sqlite3.dll"
if (Test-Path $sqlite) { Copy-Item $sqlite $dist } else { throw "missing $sqlite" }
$plugins = Join-Path $libvlc "plugins"
if (Test-Path $plugins) {
    Copy-Item $plugins (Join-Path $dist "plugins") -Recurse
}

# Documentation and notices.
foreach ($doc in @("README.md", "THIRD_PARTY_LICENSES.md", "PHASES.md")) {
    $path = Join-Path $root $doc
    if (Test-Path $path) { Copy-Item $path $dist }
}
$docs = Join-Path $root "docs"
if (Test-Path $docs) {
    New-Item -ItemType Directory -Path (Join-Path $dist "docs") | Out-Null
    Copy-Item (Join-Path $docs "*.md") (Join-Path $dist "docs")
}

$zip = Join-Path $root "dist\VFM-$version-portable.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $dist "*") -DestinationPath $zip
Write-Host "portable bundle: $zip"

# Installer (optional: only when Inno Setup is installed).
$iscc = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($iscc) {
    $iss = Join-Path $root "packaging\vfm.iss"
    Write-Host "Compiling installer with $iscc…"
    & $iscc "/DAppVersion=$version" "/DSourceDir=$dist" "/DOutputDir=$(Join-Path $root 'dist')" $iss
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }
} else {
    Write-Host "Inno Setup not found — skipping installer (portable zip is ready)."
}
