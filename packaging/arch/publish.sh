#!/usr/bin/env bash

set -euo pipefail

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
    echo "  --release      Publish both stable AUR packages (default)"
    echo "  --dry-run      Prepare and validate packages without publishing"
}

DRY_RUN=false

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
            --dry-run)
                DRY_RUN=true
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

for command_name in curl git makepkg updpkgsums; do
    if ! command -v "$command_name" &> /dev/null; then
        echo -e "${RED}Error: ${command_name} is required but not installed${NC}"
        exit 1
    fi
done

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

prepare_repo() {
    local package_name="$1"
    local repo_dir="$2"

    if [ -d "$repo_dir/.git" ]; then
        if ! git -C "$repo_dir" diff --quiet || ! git -C "$repo_dir" diff --cached --quiet; then
            echo -e "${RED}Error: ${repo_dir} has uncommitted changes${NC}"
            exit 1
        fi
        print "Updating ${package_name} AUR repository..."
        git -C "$repo_dir" pull --ff-only
    elif [ -e "$repo_dir" ]; then
        echo -e "${RED}Error: ${repo_dir} exists but is not a Git repository${NC}"
        exit 1
    else
        print "Cloning ${package_name} AUR repository..."
        git clone "ssh://aur@aur.archlinux.org/${package_name}.git" "$repo_dir"
    fi

    if ! git -C "$repo_dir" diff --quiet || ! git -C "$repo_dir" diff --cached --quiet; then
        echo -e "${RED}Error: ${repo_dir} has uncommitted changes${NC}"
        exit 1
    fi
}

publish_repo() {
    local package_name="$1"
    local repo_dir="$2"
    shift 2

    git -C "$repo_dir" diff --check

    if git -C "$repo_dir" diff --quiet -- "$@"; then
        warn "${package_name} is already up to date"
        return
    fi

    if $DRY_RUN; then
        print "${package_name} is ready to publish:"
        git -C "$repo_dir" diff --stat -- "$@"
        return
    fi

    git -C "$repo_dir" add -- "$@"
    git -C "$repo_dir" diff --cached --check
    git -C "$repo_dir" commit -m "Update to version $VERSION"
    git -C "$repo_dir" push
    print "Successfully published ${package_name} version $VERSION"
}

# Get the current version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)
TAG="v${VERSION}"
SOURCE_REPO="aur-mprisence"
BIN_REPO="aur-mprisence-bin"
BIN_TEMPLATE="packaging/arch/bin"
BIN_ARCHIVE="mprisence-${TAG}-x86_64-unknown-linux-gnu.tar.gz"
BIN_URL="https://github.com/lazykern/mprisence/releases/download/${TAG}/${BIN_ARCHIVE}"
TEMP_DIR=$(mktemp -d)
BIN_WORK_DIR="$TEMP_DIR/mprisence-bin"
BIN_SOURCE_DIR="$TEMP_DIR/sources"
SOURCE_REPO_PREPARED=false
BIN_REPO_PREPARED=false

cleanup() {
    if $DRY_RUN; then
        if $SOURCE_REPO_PREPARED; then
            git -C "$SOURCE_REPO" restore -- \
                PKGBUILD .SRCINFO mprisence.install mprisence.service
        fi
        if $BIN_REPO_PREPARED; then
            git -C "$BIN_REPO" restore -- \
                PKGBUILD .SRCINFO mprisence-bin.install mprisence.service LICENSE
        fi
    fi
    rm -rf "$TEMP_DIR"
}

trap cleanup EXIT

print "Publishing version ${VERSION}"

# Release package should only publish stable versions
if [[ "$VERSION" == *"-"* ]]; then
    echo -e "${RED}Error: Cannot publish pre-release version ${VERSION} to release package.${NC}"
    exit 1
fi

# Sync version across package files
print "Syncing version ${VERSION} to PKGBUILD..."
sed -i "s/^pkgver=.*/pkgver=${VERSION}/" packaging/arch/release/PKGBUILD
sed -i "s/^pkgver=.*/pkgver=${TAG}/" "$BIN_TEMPLATE/PKGBUILD"

mkdir -p "$BIN_WORK_DIR" "$BIN_SOURCE_DIR"
cp "$BIN_TEMPLATE/PKGBUILD" "$BIN_WORK_DIR/PKGBUILD"
cp packaging/arch/release/mprisence.install "$BIN_WORK_DIR/mprisence-bin.install"
cp packaging/arch/mprisence.service "$BIN_WORK_DIR/mprisence.service"
cp LICENSE "$BIN_WORK_DIR/LICENSE"

print "Downloading ${BIN_ARCHIVE} to calculate its checksum..."
curl --fail --location --retry 3 --output "$BIN_SOURCE_DIR/$BIN_ARCHIVE" "$BIN_URL"
print "Updating binary package checksums..."
(cd "$BIN_WORK_DIR" && SRCDEST="$BIN_SOURCE_DIR" updpkgsums)
cp "$BIN_WORK_DIR/PKGBUILD" "$BIN_TEMPLATE/PKGBUILD"

print "Generating .SRCINFO for release package..."
generate_srcinfo "packaging/arch/release"
print "Generating .SRCINFO for binary package..."
generate_srcinfo "$BIN_WORK_DIR"
cp "$BIN_WORK_DIR/.SRCINFO" "$BIN_TEMPLATE/.SRCINFO"

print "Verifying binary package sources..."
(cd "$BIN_WORK_DIR" && SRCDEST="$BIN_SOURCE_DIR" makepkg --verifysource)

prepare_repo "mprisence" "$SOURCE_REPO"
SOURCE_REPO_PREPARED=true
prepare_repo "mprisence-bin" "$BIN_REPO"
BIN_REPO_PREPARED=true

print "Updating release package..."
cp packaging/arch/release/PKGBUILD "$SOURCE_REPO/PKGBUILD"
cp packaging/arch/release/.SRCINFO "$SOURCE_REPO/.SRCINFO"
cp packaging/arch/release/mprisence.install "$SOURCE_REPO/mprisence.install"
cp packaging/arch/mprisence.service "$SOURCE_REPO/mprisence.service"

print "Updating binary package..."
cp "$BIN_TEMPLATE/PKGBUILD" "$BIN_REPO/PKGBUILD"
cp "$BIN_TEMPLATE/.SRCINFO" "$BIN_REPO/.SRCINFO"
cp packaging/arch/release/mprisence.install "$BIN_REPO/mprisence-bin.install"
cp packaging/arch/mprisence.service "$BIN_REPO/mprisence.service"
cp LICENSE "$BIN_REPO/LICENSE"

publish_repo "mprisence" "$SOURCE_REPO" \
    PKGBUILD .SRCINFO mprisence.install mprisence.service
publish_repo "mprisence-bin" "$BIN_REPO" \
    PKGBUILD .SRCINFO mprisence-bin.install mprisence.service LICENSE

if $DRY_RUN; then
    print "Dry run completed successfully!"
else
    print "Publishing completed successfully!"
fi
