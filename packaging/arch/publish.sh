#!/usr/bin/env bash

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Help message
show_help() {
    echo "Usage: $0 [OPTIONS]"
    echo "Options:"
    echo "  -h, --help     Show this help message"
    echo "  --release      Publish the release package (default)"
}

# Parse command line arguments
if [ $# -gt 0 ]; then
    for arg in "$@"; do
        case $arg in
            -h|--help)
                show_help
                exit 0
                ;;
            --release)
                ;;
            *)
                echo -e "${RED}Error: Unknown option: $arg${NC}"
                show_help
                exit 1
                ;;
        esac
    done
fi

# Check if running from the root of the project
if [ ! -f "Cargo.toml" ]; then
    echo -e "${RED}Error: This script must be run from the root of the project${NC}"
    exit 1
fi

# Check if makepkg is installed
if ! command -v makepkg &> /dev/null; then
    echo -e "${RED}Error: makepkg is required but not installed${NC}"
    exit 1
fi

# Function to print with color
print() {
    echo -e "${GREEN}==>${NC} $1"
}

# Function to print warning with color
warn() {
    echo -e "${YELLOW}Warning:${NC} $1"
}

# Function to generate .SRCINFO
generate_srcinfo() {
    local pkgbuild_dir="$1"
    (cd "$pkgbuild_dir" && makepkg --printsrcinfo > .SRCINFO)
}

# Get the current version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)

print "Publishing version ${VERSION}"

# Release package should only publish stable versions
if [[ "$VERSION" == *"-"* ]]; then
    echo -e "${RED}Error: Cannot publish pre-release version ${VERSION} to release package.${NC}"
    exit 1
fi

# Sync version across package files
print "Syncing version ${VERSION} to PKGBUILD..."
sed -i "s/^pkgver=.*/pkgver=${VERSION}/" packaging/arch/release/PKGBUILD

# Path to the AUR package repo
RELEASE_REPO="aur-mprisence"

print "Generating .SRCINFO for release package..."
generate_srcinfo "packaging/arch/release"

if [ ! -d "$RELEASE_REPO" ]; then
    print "Cloning release AUR repository..."
    git clone ssh://aur@aur.archlinux.org/mprisence.git "$RELEASE_REPO"
fi

print "Updating release package..."
cp packaging/arch/release/PKGBUILD "$RELEASE_REPO/PKGBUILD"
cp packaging/arch/release/.SRCINFO "$RELEASE_REPO/.SRCINFO"
cp packaging/arch/release/mprisence.install "$RELEASE_REPO/mprisence.install"
cp packaging/arch/mprisence.service "$RELEASE_REPO/mprisence.service"

print "Publishing release package..."
(
    cd "$RELEASE_REPO"
    git add PKGBUILD .SRCINFO mprisence.install mprisence.service
    if git diff --cached --quiet; then
        warn "Release package already up to date; skipping commit and push"
    else
        git commit -m "Update to version $VERSION"
        git push
        print "Successfully published release package version $VERSION"
    fi
)

print "Publishing completed successfully!"
