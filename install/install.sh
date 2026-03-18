#!/bin/bash
set -e

REPO="ortus-boxlang/matchbox"
INSTALL_DIR="/usr/local/bin"

echo "Welcome to the MatchBox Installer!"
echo "----------------------------------"
echo "Which version would you like to install?"
echo "1) Latest Release (Stable)"
echo "2) Latest Snapshot (Bleeding Edge)"
read -p "Selection [1-2]: " VERSION_CHOICE

if [ "$VERSION_CHOICE" == "2" ]; then
    TAG="snapshot"
else
    TAG=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
fi

# Detect OS
OS_NAME=$(uname -s | tr '[:upper:]' '[:lower:]')
case "$OS_NAME" in
    linux*)  OS="linux" ;;
    darwin*) OS="macos" ;;
    *) echo "Unsupported OS: $OS_NAME"; exit 1 ;;
esac

# Detect Architecture
ARCH_NAME=$(uname -m)
case "$ARCH_NAME" in
    x86_64)  ARCH="x64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    i386|i686) ARCH="x86" ;;
    armv7*) ARCH="armv7" ;;
    *) echo "Unsupported architecture: $ARCH_NAME"; exit 1 ;;
esac

BINARY_NAME="matchbox-$OS-$ARCH"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$TAG/$BINARY_NAME"

echo "Downloading MatchBox ($TAG) for $OS-$ARCH..."
curl -L -o matchbox "$DOWNLOAD_URL"
chmod +x matchbox

echo "Installing to $INSTALL_DIR (requires sudo)..."
sudo mv matchbox "$INSTALL_DIR/matchbox"

echo "----------------------------------"
echo "Success! MatchBox has been installed."
matchbox --version
