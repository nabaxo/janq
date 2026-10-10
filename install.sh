#!/bin/sh
set -e

# janq installer config
INSTALL_DIR="${HOME}/.local/bin"
BINARY_NAME="janq"

# URLs
INSTANCE_URL="https://git.nabaxo.dev"
REPO_PATH="nabaxo/janq"
API_URL="${INSTANCE_URL}/api/v1/repos/${REPO_PATH}/releases/latest"

echo "Checking for latest release..."

# fetch URL [OUTPUT]: print URL to stdout, or save it to OUTPUT
if command -v curl >/dev/null 2>&1; then
    fetch() { if [ -n "$2" ]; then curl -fsSL "$1" -o "$2"; else curl -s "$1"; fi; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { if [ -n "$2" ]; then wget -q "$1" -O "$2"; else wget -qO- "$1"; fi; }
else
    echo "Error: curl or wget required"
    exit 1
fi

JSON_DATA=$(fetch "${API_URL}" || true)
DOWNLOAD_URL=""

if [ -n "$JSON_DATA" ]; then
    DOWNLOAD_URL=$(echo "$JSON_DATA" | tr ',' '\n' | grep "browser_download_url" | grep "/${BINARY_NAME}\"" | head -n 1 | cut -d '"' -f 4)
fi

if [ -z "$DOWNLOAD_URL" ]; then
    echo "Error: No release asset found at ${API_URL}"
    exit 1
fi

echo "Found release asset: ${DOWNLOAD_URL}"
echo "Downloading from: ${DOWNLOAD_URL}"
# Private, unpredictable temp file: a fixed /tmp path can be pre-created by another user.
TMP_FILE=$(mktemp)
trap 'rm -f "$TMP_FILE"' EXIT
fetch "${DOWNLOAD_URL}" "${TMP_FILE}"

echo "Installing to ${INSTALL_DIR}..."
mkdir -p "${INSTALL_DIR}"
mv "${TMP_FILE}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod 755 "${INSTALL_DIR}/${BINARY_NAME}"

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
