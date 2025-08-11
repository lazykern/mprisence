#!/bin/bash
set -e

# Ensure the script is run from the project root
if [ ! -f "Cargo.toml" ]; then
    echo "Error: This script must be run from the root of the project" >&2
    exit 1
fi

# Check for dependencies
command -v cargo >/dev/null 2>&1 || { echo "Error: cargo is required but not installed." >&2; exit 1; }
command -v cargo-deb >/dev/null 2>&1 || { echo "Error: cargo-deb is required but not installed. Please run 'cargo install cargo-deb'" >&2; exit 1; }

VERSION=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)
DIST_DIR="dist"

echo "--- Packaging mprisence version ${VERSION} ---"

# Create dist directory
mkdir -p "${DIST_DIR}"

# 1. Build the release binary
echo
echo "--- Building release binary... ---"
cargo build --release

# 2. Create the .tar.gz archive
echo
echo "--- Creating binary archive... ---"
ARCHIVE_NAME="mprisence-v${VERSION}-x86_64-unknown-linux-gnu.tar.gz"
tar -C target/release -czvf "${ARCHIVE_NAME}" mprisence
mv "${ARCHIVE_NAME}" "${DIST_DIR}/"
echo "Archive created at: $(pwd)/${DIST_DIR}/${ARCHIVE_NAME}"

# 3. Build the .deb package
echo
echo "--- Building Debian package... ---"
cargo deb
DEB_PATH=$(find target/debian -name "*.deb")
mv "${DEB_PATH}" "${DIST_DIR}/"
echo "Debian package created at: $(pwd)/${DIST_DIR}/$(basename "${DEB_PATH}")"


echo
echo "--- Packaging complete! ---"
echo "Find your packages in the '${DIST_DIR}' directory."
