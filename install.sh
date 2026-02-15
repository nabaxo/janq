#!/bin/sh
set -e

# janq installer config
INSTALL_DIR="${HOME}/.local/bin"
BINARY_NAME="janq"

# URLs
INSTANCE_URL="https://git.nabaxo.dev"
REPO_PATH="nabaxo/janq"
API_URL="${INSTANCE_URL}/api/v1/repos/${REPO_PATH}/releases/latest"
FALLBACK_URL="${INSTANCE_URL}/${REPO_PATH}/raw/branch/main/dist/${BINARY_NAME}"

echo "Checking for latest release..."
DOWNLOAD_URL=""

JSON_DATA=$(curl -s "${API_URL}")

if [ -n "$JSON_DATA" ]; then
    DOWNLOAD_URL=$(echo "$JSON_DATA" | tr ',' '\n' | grep "browser_download_url" | grep "/${BINARY_NAME}\"" | head -n 1 | cut -d '"' -f 4)
fi

if [ -z "$DOWNLOAD_URL" ]; then
    echo "No release asset found. Falling back to latest binary from dist..."
    DOWNLOAD_URL="${FALLBACK_URL}"
else
    echo "Found release asset: ${DOWNLOAD_URL}"
fi

echo "Downloading from: ${DOWNLOAD_URL}"
if command -v curl >/dev/null 2>&1; then
    curl -fsSL "${DOWNLOAD_URL}" -o "/tmp/${BINARY_NAME}"
elif command -v wget >/dev/null 2>&1; then
    wget -q "${DOWNLOAD_URL}" -O "/tmp/${BINARY_NAME}"
else
    echo "Error: curl or wget required"
    exit 1
fi

echo "Installing to ${INSTALL_DIR}..."
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
