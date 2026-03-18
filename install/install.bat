@echo off
set REPO=ortus-boxlang/matchbox
set INSTALL_DIR=%USERPROFILE%\.matchbox\bin

echo Welcome to the MatchBox Installer!
echo ----------------------------------
echo Which version would you like to install?
echo 1) Latest Release (Stable)
echo 2) Latest Snapshot (Bleeding Edge)
set /p choice="Selection [1-2]: "

if "%choice%"=="2" (
    set TAG=snapshot
) else (
    :: Use powershell to get the latest tag name
    for /f "delims=" %%i in ('powershell -command "(Invoke-RestMethod -Uri 'https://api.github.com/repos/%REPO%/releases/latest').tag_name"') do set TAG=%%i
)

:: Detect Architecture
set ARCH=x64
if "%PROCESSOR_ARCHITECTURE%"=="x86" set ARCH=x86
if "%PROCESSOR_ARCHITECTURE%"=="ARM64" set ARCH=arm64

set BINARY_NAME=matchbox-windows-%ARCH%.exe
set DOWNLOAD_URL=https://github.com/%REPO%/releases/download/%TAG%/%BINARY_NAME%

if not exist "%INSTALL_DIR%" mkdir "%INSTALL_DIR%"

echo Downloading MatchBox (%TAG%) for Windows-%ARCH%...
powershell -command "Invoke-WebRequest -Uri '%DOWNLOAD_URL%' -OutFile '%INSTALL_DIR%\matchbox.exe'"

:: Add to PATH if not already there
echo %PATH% | findstr /i "%INSTALL_DIR%" > nul
if errorlevel 1 (
    setx PATH "%PATH%;%INSTALL_DIR%"
)

echo ----------------------------------
echo Success! MatchBox has been installed to %INSTALL_DIR%
echo Please restart your terminal to use 'matchbox' from anywhere.
"%INSTALL_DIR%\matchbox.exe" --version
