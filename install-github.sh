#!/bin/sh
set -e

# janq installer config
INSTALL_DIR="${HOME}/.local/bin"
BINARY_NAME="janq"

# GitHub URLs (primary)
GH_REPO="nabaxo/janq"
GH_API_URL="https://api.github.com/repos/${GH_REPO}/releases/latest"
GH_FALLBACK_URL="https://raw.githubusercontent.com/${GH_REPO}/main/dist/${BINARY_NAME}"

# Forgejo URLs (fallback)
FORGEJO_URL="https://git.nabaxo.dev"
FORGEJO_API_URL="${FORGEJO_URL}/api/v1/repos/${GH_REPO}/releases/latest"
FORGEJO_FALLBACK_URL="${FORGEJO_URL}/${GH_REPO}/raw/branch/main/dist/${BINARY_NAME}"

fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$1" -O "$2"
    else
        echo "Error: curl or wget required"
        exit 1
    fi
}

fetch_json() {
    if command -v curl >/dev/null 2>&1; then
        curl -s "$1" 2>/dev/null
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "$1" 2>/dev/null
    else
        echo ""
    fi
}

find_release_url() {
    _api_url="$1"
    _json=$(fetch_json "$_api_url")
    if [ -n "$_json" ]; then
        echo "$_json" | tr ',' '\n' | grep "browser_download_url" | grep "/${BINARY_NAME}\"" | head -n 1 | cut -d '"' -f 4
    fi
}

DOWNLOAD_URL=""

# 1. Try GitHub release
echo "Checking GitHub for latest release..."
DOWNLOAD_URL=$(find_release_url "$GH_API_URL")

# 2. Fall back to GitHub raw dist
if [ -z "$DOWNLOAD_URL" ]; then
    echo "No GitHub release asset. Trying GitHub raw dist..."
    if fetch_json "$GH_FALLBACK_URL" >/dev/null 2>&1; then
        DOWNLOAD_URL="$GH_FALLBACK_URL"
    fi
fi

# 3. Fall back to Forgejo release
if [ -z "$DOWNLOAD_URL" ]; then
    echo "GitHub unavailable. Trying Forgejo release..."
    DOWNLOAD_URL=$(find_release_url "$FORGEJO_API_URL")
fi

# 4. Fall back to Forgejo raw dist
if [ -z "$DOWNLOAD_URL" ]; then
    echo "No Forgejo release asset. Falling back to Forgejo raw dist..."
    DOWNLOAD_URL="$FORGEJO_FALLBACK_URL"
fi

echo "Downloading from: ${DOWNLOAD_URL}"
fetch "$DOWNLOAD_URL" "/tmp/${BINARY_NAME}"

echo "Installing to ${INSTALL_DIR}..."
mkdir -p "${INSTALL_DIR}"
mv "/tmp/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

echo "janq installed successfully!"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "---"
        echo "WARNING: ${INSTALL_DIR} is not in your PATH."
        echo "You may need to add 'export PATH=\$PATH:${INSTALL_DIR}' to your .bashrc or .zshrc"
        echo "---"
        ;;
esac

# Argument pass-through Logic
if [ "$#" -gt 0 ]; then
    echo "Forwarding arguments to ${BINARY_NAME}: $@"
    "${INSTALL_DIR}/${BINARY_NAME}" "$@"
else
    echo "Run '${BINARY_NAME} --help' to get started"
fi
