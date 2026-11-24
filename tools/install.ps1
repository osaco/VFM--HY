# Install VFM for the current user (no admin required).
#
#   powershell -ExecutionPolicy Bypass -File tools/install.ps1
#   powershell -ExecutionPolicy Bypass -File tools/install.ps1 -NoDesktop
#
# Layout produced:
#   %LOCALAPPDATA%\Programs\VFM\VFM.exe
#   %LOCALAPPDATA%\Programs\VFM\vlc\            (libvlc.dll, libvlccore.dll, plugins\)
#   %LOCALAPPDATA%\Programs\VFM\vfm.ico
#   %LOCALAPPDATA%\Programs\VFM\docs\
#   Desktop\VFM.lnk and Start Menu\VFM.lnk
#
# The runtime sits in a "vlc" subfolder because that is the first place the loader looks
# (see crates/vlc-wrapper/src/loader.rs), and because the LGPL requires the DLLs to stay
# replaceable as loose files.

param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\VFM",
    [switch]$NoDesktop,
    [switch]$NoStartMenu
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
# Either build directory may hold the newest binary (target\release has a stuck lock file on
# this machine, so builds sometimes land in target2). Pick whichever exe is fresher.
$exe = @(
    (Join-Path $root "target\release\vfm-spike.exe"),
    (Join-Path $root "target2\release\vfm-spike.exe")
) | Where-Object { Test-Path $_ } |
    Sort-Object { (Get-Item $_).LastWriteTime } -Descending |
    Select-Object -First 1
$libvlc = Join-Path $root "vendor\libvlc"
$icon = Join-Path $root "assets\branding\vfm.ico"

if (-not $exe) { throw "missing target\release\vfm-spike.exe - run: cargo build --release -p vfm-app" }
if (-not (Test-Path (Join-Path $libvlc "libvlc.dll"))) { throw "missing libVLC runtime in vendor\libvlc" }
if (-not (Test-Path $icon)) { throw "missing $icon" }

Write-Host "Installing VFM to $InstallDir"

# Close a running copy so the executable can be replaced.
Get-Process -Name "VFM", "vfm-spike" -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "  stopping $($_.ProcessName) ($($_.Id))"
    $_ | Stop-Process -Force -ErrorAction SilentlyContinue
}
Start-Sleep -Milliseconds 300

$target = Join-Path $InstallDir "VFM.exe"
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
Copy-Item $exe $target -Force

# Runtime: DLLs plus the whole plugins tree.
$runtime = Join-Path $InstallDir "vlc"
New-Item -ItemType Directory -Path $runtime -Force | Out-Null
Copy-Item (Join-Path $libvlc "*.dll") $runtime -Force
$plugins = Join-Path $libvlc "plugins"
if (Test-Path $plugins) {
    $pluginsTarget = Join-Path $runtime "plugins"
    if (Test-Path $pluginsTarget) { Remove-Item -Recurse -Force $pluginsTarget }
    Copy-Item $plugins $pluginsTarget -Recurse -Force
}

# sqlite3.dll is a load-time import of the executable, not just a test dependency.
$sqlite = Join-Path $root "vendor\sqlite\lib\sqlite3.dll"
if (Test-Path $sqlite) {
    Copy-Item $sqlite (Join-Path $InstallDir "sqlite3.dll") -Force
} else {
    throw "missing $sqlite"
}

Copy-Item $icon (Join-Path $InstallDir "vfm.ico") -Force

$docs = Join-Path $InstallDir "docs"
New-Item -ItemType Directory -Path $docs -Force | Out-Null
foreach ($doc in @("README.md", "THIRD_PARTY_LICENSES.md", "PHASES.md")) {
    $path = Join-Path $root $doc
    if (Test-Path $path) { Copy-Item $path $InstallDir -Force }
}
$docSource = Join-Path $root "docs"
if (Test-Path $docSource) {
    Copy-Item (Join-Path $docSource "*.md") $docs -Force
}

# Uninstaller, so the install is removable without a package manager.
$uninstall = @"
# Remove VFM from this user account.
`$ErrorActionPreference = "Continue"
Get-Process -Name "VFM" -ErrorAction SilentlyContinue | Stop-Process -Force
foreach (`$lnk in @(
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'VFM.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'VFM.lnk')
)) { if (Test-Path `$lnk) { Remove-Item `$lnk -Force } }
`$target = "$InstallDir"
if (Test-Path `$target) { Remove-Item `$target -Recurse -Force }
Write-Host "VFM removed."
"@
$uninstall | Set-Content -Path (Join-Path $InstallDir "uninstall.ps1") -Encoding UTF8

# Shortcuts.
$shell = New-Object -ComObject WScript.Shell
function New-VfmShortcut([string]$linkPath, [string]$description) {
    $shortcut = $shell.CreateShortcut($linkPath)
    $shortcut.TargetPath = $target
    $shortcut.WorkingDirectory = $InstallDir
    $shortcut.IconLocation = "$InstallDir\vfm.ico,0"
    $shortcut.Description = $description
    $shortcut.WindowStyle = 1
    $shortcut.Save()
    Write-Host "  shortcut: $linkPath"
}

if (-not $NoDesktop) {
    $desktop = [Environment]::GetFolderPath('Desktop')
    New-VfmShortcut (Join-Path $desktop "VFM.lnk") "VFM - media player"
}
if (-not $NoStartMenu) {
    $programs = [Environment]::GetFolderPath('Programs')
    New-VfmShortcut (Join-Path $programs "VFM.lnk") "VFM - media player"
}

$size = [math]::Round(((Get-ChildItem $InstallDir -Recurse -File | Measure-Object Length -Sum).Sum / 1MB), 1)
Write-Host ""
Write-Host "VFM installed."
Write-Host "  exe:      $target"
Write-Host "  runtime:  $runtime"
Write-Host "  size:     $size MB"
Write-Host "  remove:   powershell -ExecutionPolicy Bypass -File `"$InstallDir\uninstall.ps1`""
