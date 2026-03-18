$REPO = "ortus-boxlang/matchbox"
$INSTALL_DIR = "$HOME\.matchbox\bin"

Write-Host "Welcome to the MatchBox Installer!" -ForegroundColor Cyan
Write-Host "----------------------------------"

$choice = Read-Host "Which version would you like to install?`n1) Latest Release (Stable)`n2) Latest Snapshot (Bleeding Edge)`nSelection [1-2]"

if ($choice -eq "2") {
    $TAG = "snapshot"
} else {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$REPO/releases/latest"
    $TAG = $release.tag_name
}

# Detect Architecture
$ARCH = "x64"
if ([IntPtr]::Size -eq 4) { $ARCH = "x86" }
if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { $ARCH = "arm64" }

$BINARY_NAME = "matchbox-windows-$ARCH.exe"
$DOWNLOAD_URL = "https://github.com/$REPO/releases/download/$TAG/$BINARY_NAME"

if (!(Test-Path $INSTALL_DIR)) {
    New-Item -Path $INSTALL_DIR -ItemType Directory | Out-Null
}

Write-Host "Downloading MatchBox ($TAG) for Windows-$ARCH..." -ForegroundColor Yellow
Invoke-WebRequest -Uri $DOWNLOAD_URL -OutFile "$INSTALL_DIR\matchbox.exe"

# Add to PATH for the current session if not present
if ($env:Path -notlike "*$INSTALL_DIR*") {
    $env:Path += ";$INSTALL_DIR"
    [Environment]::SetEnvironmentVariable("Path", [Environment]::GetEnvironmentVariable("Path", "User") + ";$INSTALL_DIR", "User")
}

Write-Host "----------------------------------" -ForegroundColor Cyan
Write-Host "Success! MatchBox has been installed to $INSTALL_DIR" -ForegroundColor Green
Write-Host "Please restart your terminal to use 'matchbox' from anywhere."
& matchbox.exe --version
