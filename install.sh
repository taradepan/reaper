#!/bin/sh
set -eu

REPO="taradepan/reaper"
BINARY="reaper"
INSTALL_DIR="/usr/local/bin"

info() {
    printf '\033[1;34m%s\033[0m\n' "$*"
}

error() {
    printf '\033[1;31merror: %s\033[0m\n' "$*" >&2
    exit 1
}

detect_target() {
    OS=$(uname -s)
    ARCH=$(uname -m)

    case "$OS" in
        Linux)  OS_PART="unknown-linux-gnu" ;;
        Darwin) OS_PART="apple-darwin" ;;
        *)      error "Unsupported OS: $OS. Use Windows PowerShell script (install.ps1) on Windows." ;;
    esac

    case "$ARCH" in
        x86_64|amd64)   ARCH_PART="x86_64" ;;
        aarch64|arm64)   ARCH_PART="aarch64" ;;
        *)               error "Unsupported architecture: $ARCH" ;;
    esac

    echo "${ARCH_PART}-${OS_PART}"
}

get_latest_version() {
    if command -v curl > /dev/null 2>&1; then
        curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep '"tag_name"' \
            | sed 's/.*"v\(.*\)".*/\1/'
    elif command -v wget > /dev/null 2>&1; then
        wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep '"tag_name"' \
            | sed 's/.*"v\(.*\)".*/\1/'
    else
        error "Neither curl nor wget found. Please install one of them."
    fi
}

download() {
    URL="$1"
    OUTPUT="$2"
    if command -v curl > /dev/null 2>&1; then
        curl -fsSL -o "$OUTPUT" "$URL"
    elif command -v wget > /dev/null 2>&1; then
        wget -qO "$OUTPUT" "$URL"
    else
        error "Neither curl nor wget found. Please install one of them."
    fi
}

main() {
    TARGET=$(detect_target)
    info "Detected target: ${TARGET}"

    info "Fetching latest version..."
    VERSION=$(get_latest_version)
    if [ -z "$VERSION" ]; then
        error "Could not determine the latest version. Check https://github.com/${REPO}/releases"
    fi
    info "Latest version: v${VERSION}"

    ARCHIVE="reaper-v${VERSION}-${TARGET}.tar.gz"
    URL="https://github.com/${REPO}/releases/download/v${VERSION}/${ARCHIVE}"

    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    info "Downloading ${URL}..."
    download "$URL" "${TMP_DIR}/${ARCHIVE}"

    info "Extracting..."
    tar xzf "${TMP_DIR}/${ARCHIVE}" -C "$TMP_DIR"
    chmod +x "${TMP_DIR}/${BINARY}"

    if [ -w "$INSTALL_DIR" ]; then
        mv "${TMP_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    else
        info "Writing to ${INSTALL_DIR} requires elevated permissions."
        sudo mv "${TMP_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    fi

    info "Installed ${BINARY} v${VERSION} to ${INSTALL_DIR}/${BINARY}"
    info "Run 'reaper --help' to get started."
}

main