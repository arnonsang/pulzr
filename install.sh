#!/bin/bash

set -e

# Pulzr Installation Script

REPO="arnonsang/pulzr"
BINARY_NAME="pulzr"
INSTALL_DIR="/usr/local/bin"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

print_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case $ARCH in
    x86_64)
        ARCH="amd64"
        ;;
    aarch64|arm64)
        ARCH="arm64"
        ;;
    *)
        print_error "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

case $OS in
    linux)
        ASSET_NAME="${BINARY_NAME}-linux-${ARCH}"
        ;;
    darwin)
        ASSET_NAME="${BINARY_NAME}-macos-${ARCH}"
        ;;
    *)
        print_error "Unsupported OS: $OS"
        exit 1
        ;;
esac

print_info "Detected OS: $OS, Architecture: $ARCH"

# Check if curl is available
if ! command -v curl &> /dev/null; then
    print_error "curl is required but not installed."
    exit 1
fi

# Get latest release
print_info "Fetching latest release information..."
LATEST_RELEASE=$(curl -s "https://api.github.com/repos/$REPO/releases/latest")
VERSION=$(echo "$LATEST_RELEASE" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
DOWNLOAD_URL=$(echo "$LATEST_RELEASE" | grep "browser_download_url.*$ASSET_NAME" | cut -d '"' -f 4)

if [ -z "$DOWNLOAD_URL" ]; then
    print_error "Could not find download URL for $ASSET_NAME"
    exit 1
fi

print_info "Latest version: $VERSION"
print_info "Download URL: $DOWNLOAD_URL"

# Download binary
TEMP_DIR=$(mktemp -d)
TEMP_FILE="$TEMP_DIR/$BINARY_NAME"

print_info "Downloading $BINARY_NAME..."
curl -L -o "$TEMP_FILE" "$DOWNLOAD_URL"

# Make executable
chmod +x "$TEMP_FILE"

# Install binary
print_info "Installing $BINARY_NAME to $INSTALL_DIR..."
if [ -w "$INSTALL_DIR" ]; then
    mv "$TEMP_FILE" "$INSTALL_DIR/$BINARY_NAME"
else
    sudo mv "$TEMP_FILE" "$INSTALL_DIR/$BINARY_NAME"
fi

# Cleanup
rm -rf "$TEMP_DIR"

print_info "✅ Pulzr installed successfully!"
print_info "Run 'pulzr --help' to get started."

# Verify installation
if command -v pulzr &> /dev/null; then
    print_info "Installation verified: $(pulzr --version)"
else
    print_warning "Installation complete, but 'pulzr' not found in PATH."
    print_warning "You may need to add $INSTALL_DIR to your PATH."
fi