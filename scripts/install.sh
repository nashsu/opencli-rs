#!/bin/sh
# autocli installer — detects OS/Arch and downloads the right binary
# Usage: curl -fsSL https://raw.githubusercontent.com/nashsu/autocli/main/scripts/install.sh | sh

set -e

REPO="nashsu/autocli"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="autocli"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

info() { printf "${CYAN}$1${NC}\n"; }
success() { printf "${GREEN}$1${NC}\n"; }
error() { printf "${RED}Error: $1${NC}\n" >&2; exit 1; }

# Detect OS
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
case "$OS" in
    linux*)  OS="unknown-linux-musl" ;;
    darwin*) OS="apple-darwin" ;;
    mingw*|msys*|cygwin*) OS="pc-windows-msvc" ;;
    *) error "Unsupported OS: $OS" ;;
esac

# Detect Arch
ARCH=$(uname -m)
case "$ARCH" in
    x86_64|amd64)  ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) error "Unsupported architecture: $ARCH" ;;
esac

TARGET="${ARCH}-${OS}"

# Get latest version (via redirect, avoids API rate limit)
info "Detecting latest version..."
VERSION=$(curl -fsSI "https://github.com/${REPO}/releases/latest" | grep -i "location:" | sed -E 's/.*\/tag\/(.*)/\1/' | tr -d '\r\n')
if [ -z "$VERSION" ]; then
    error "Could not detect latest version. Check https://github.com/${REPO}/releases"
fi
info "Latest version: ${VERSION}"

# Determine archive format
if echo "$OS" | grep -q "windows"; then
    EXT="zip"
    ARCHIVE="${BINARY_NAME}-${TARGET}.zip"
else
    EXT="tar.gz"
    ARCHIVE="${BINARY_NAME}-${TARGET}.tar.gz"
fi

URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"

# Download
info "Downloading ${ARCHIVE}..."
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

if ! curl -fsSL "$URL" -o "${TMPDIR}/${ARCHIVE}"; then
    error "Download failed. Binary may not exist for ${TARGET}.\nCheck: https://github.com/${REPO}/releases/tag/${VERSION}"
fi

# Extract
info "Extracting..."
cd "$TMPDIR"
if [ "$EXT" = "zip" ]; then
    unzip -q "$ARCHIVE"
else
    tar xzf "$ARCHIVE"
fi

# Install
if [ -w "$INSTALL_DIR" ]; then
    mv "$BINARY_NAME" "$INSTALL_DIR/"
else
    info "Installing to ${INSTALL_DIR} (requires sudo)..."
    sudo mv "$BINARY_NAME" "$INSTALL_DIR/"
fi

chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

# Migrate from .opencli-rs to .autocli
OLD_CONFIG="$HOME/.opencli-rs"
NEW_CONFIG="$HOME/.autocli"
if [ -d "$OLD_CONFIG" ]; then
    if [ -d "$NEW_CONFIG" ]; then
        info "Both ~/.opencli-rs and ~/.autocli exist, merging..."
        cp -rn "$OLD_CONFIG/"* "$NEW_CONFIG/" 2>/dev/null || true
    else
        info "Migrating ~/.opencli-rs to ~/.autocli..."
        cp -r "$OLD_CONFIG" "$NEW_CONFIG"
    fi
    rm -rf "$OLD_CONFIG"
    success "✓ Migrated config from ~/.opencli-rs to ~/.autocli"
fi

# Remove old binary if exists
if command -v "opencli-rs" >/dev/null 2>&1; then
    OLD_BIN=$(command -v "opencli-rs")
    info "Removing old binary: ${OLD_BIN}"
    rm -f "$OLD_BIN" 2>/dev/null || sudo rm -f "$OLD_BIN" 2>/dev/null || true
fi

# Kill old daemon and start new one
DAEMON_PORT=19925
if lsof -ti tcp:${DAEMON_PORT} >/dev/null 2>&1; then
    info "Stopping old daemon on port ${DAEMON_PORT}..."
    lsof -ti tcp:${DAEMON_PORT} | xargs kill -9 2>/dev/null || true
    sleep 1
fi
info "Starting new daemon..."
"${INSTALL_DIR}/${BINARY_NAME}" --version >/dev/null 2>&1 || true

# Verify
if command -v "$BINARY_NAME" >/dev/null 2>&1; then
    INSTALLED_VERSION=$("$BINARY_NAME" --version 2>/dev/null || echo "unknown")
    success "✓ ${BINARY_NAME} installed successfully! (${INSTALLED_VERSION})"
    echo ""
    echo "  Get started:"
    echo "    ${BINARY_NAME} --help"
    echo "    ${BINARY_NAME} hackernews top --limit 5"
else
    success "✓ Installed to ${INSTALL_DIR}/${BINARY_NAME}"
    echo "  Make sure ${INSTALL_DIR} is in your PATH."
fi
