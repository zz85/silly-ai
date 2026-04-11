#!/bin/bash
set -euo pipefail

# Silly AI installer
# Usage: curl -fsSL https://raw.githubusercontent.com/zz85/silly-ai/main/install.sh | bash

REPO="zz85/silly-ai"
BINARY="silly"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# --------------------------------------------------------------------------
# Platform detection
# --------------------------------------------------------------------------

detect_platform() {
    local os arch

    os="$(uname -s)"
    case "$os" in
        Darwin*)  os="darwin" ;;
        Linux*)   os="linux" ;;
        *)
            echo "Error: Unsupported operating system: $os"
            echo "Silly currently supports macOS and Linux."
            exit 1
            ;;
    esac

    arch="$(uname -m)"
    case "$arch" in
        x86_64)          arch="x86_64" ;;
        aarch64|arm64)   arch="aarch64" ;;
        *)
            echo "Error: Unsupported architecture: $arch"
            echo "Silly currently supports x86_64 and aarch64 (arm64)."
            exit 1
            ;;
    esac

    # linux-aarch64 is not yet available
    if [ "$os" = "linux" ] && [ "$arch" = "aarch64" ]; then
        echo "Error: Linux ARM64 builds are not yet available."
        echo "Please build from source: cargo install --git https://github.com/$REPO"
        exit 1
    fi

    # macOS x86_64 (Intel) prebuilt binaries are not available
    if [ "$os" = "darwin" ] && [ "$arch" = "x86_64" ]; then
        echo "Error: Intel Mac prebuilt binaries are not available."
        echo "Apple Silicon (M1+) is required for prebuilt binaries."
        echo "Please build from source: cargo install --git https://github.com/$REPO"
        exit 1
    fi

    PLATFORM="${os}-${arch}"
}

# --------------------------------------------------------------------------
# Version discovery
# --------------------------------------------------------------------------

get_latest_version() {
    local url="https://api.github.com/repos/$REPO/releases/latest"

    if command -v curl >/dev/null 2>&1; then
        VERSION=$(curl -fsSL "$url" | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')
    elif command -v wget >/dev/null 2>&1; then
        VERSION=$(wget -qO- "$url" | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')
    else
        echo "Error: curl or wget is required."
        exit 1
    fi

    if [ -z "$VERSION" ]; then
        echo "Error: Could not determine the latest version."
        echo "Check https://github.com/$REPO/releases for available versions."
        exit 1
    fi
}

# --------------------------------------------------------------------------
# Download and install
# --------------------------------------------------------------------------

download_and_install() {
    local tarball="silly-${PLATFORM}.tar.gz"
    local url="https://github.com/$REPO/releases/download/${VERSION}/${tarball}"
    local tmpdir

    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    echo "Downloading $BINARY $VERSION for $PLATFORM..."

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$tmpdir/$tarball"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$url" -O "$tmpdir/$tarball"
    fi

    echo "Extracting..."
    tar -xzf "$tmpdir/$tarball" -C "$tmpdir"

    echo "Installing to $INSTALL_DIR..."
    mkdir -p "$INSTALL_DIR"
    mv "$tmpdir/$BINARY" "$INSTALL_DIR/$BINARY"
    chmod +x "$INSTALL_DIR/$BINARY"

    echo ""
}

# --------------------------------------------------------------------------
# Post-install checks
# --------------------------------------------------------------------------

verify_install() {
    if [ ! -x "$INSTALL_DIR/$BINARY" ]; then
        echo "Error: Installation failed. Binary not found at $INSTALL_DIR/$BINARY"
        exit 1
    fi

    echo "Silly $VERSION installed successfully!"
    echo ""

    # Check if install dir is in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*)
            ;;
        *)
            echo "WARNING: $INSTALL_DIR is not in your PATH."
            echo ""
            echo "Add it by appending one of the following to your shell config:"
            echo ""
            echo "  # For bash (~/.bashrc or ~/.bash_profile):"
            echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
            echo ""
            echo "  # For zsh (~/.zshrc):"
            echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
            echo ""
            echo "  # For fish (~/.config/fish/config.fish):"
            echo "  set -gx PATH $INSTALL_DIR \$PATH"
            echo ""
            echo "Then restart your shell or run: source ~/.zshrc (or equivalent)"
            echo ""
            ;;
    esac

    echo "NOTE: On first run, silly will download ~500MB of AI models to:"
    echo "  ~/.local/share/silly/models/"
    echo ""
    echo "This includes speech-to-text, voice activity detection, and"
    echo "text-to-speech models. Ensure you have an internet connection."
    echo ""
    echo "Get started:"
    echo "  $BINARY --help"
}

# --------------------------------------------------------------------------
# Main
# --------------------------------------------------------------------------

main() {
    echo ""
    echo "  Silly AI Installer"
    echo "  ==================="
    echo ""

    detect_platform
    get_latest_version

    echo "  Version:  $VERSION"
    echo "  Platform: $PLATFORM"
    echo "  Install:  $INSTALL_DIR"
    echo ""

    download_and_install
    verify_install
}

main
