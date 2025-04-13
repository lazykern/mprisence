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
    echo "  --release      Publish only the release package (default if no option specified)"
    echo "  --git          Publish only the git package"
    echo "  --both         Publish both release and git packages"
}

# Parse command line arguments
PUBLISH_RELEASE=false
PUBLISH_GIT=false

if [ $# -eq 0 ]; then
    PUBLISH_RELEASE=true
else
    for arg in "$@"; do
        case $arg in
            -h|--help)
                show_help
                exit 0
                ;;
            --release)
                PUBLISH_RELEASE=true
                ;;
            --git)
                PUBLISH_GIT=true
                ;;
            --both)
                PUBLISH_RELEASE=true
                PUBLISH_GIT=true
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
if [ ! -f "Makefile" ]; then
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

# Check if this is a pre-release version
if [[ "$VERSION" == *"-"* ]]; then
    if [ "$PUBLISH_RELEASE" = true ] && [ "$PUBLISH_GIT" = false ]; then
        echo -e "${RED}Error: Cannot publish pre-release version ${VERSION} to release package.${NC}"
        echo -e "${RED}Pre-release versions should only be published to the -git package.${NC}"
        echo -e "${YELLOW}Use --git to publish to the -git package instead.${NC}"
        exit 1
    elif [ "$PUBLISH_RELEASE" = true ] && [ "$PUBLISH_GIT" = true ]; then
        warn "Pre-release version ${VERSION} will only be published to the -git package"
        PUBLISH_RELEASE=false
    fi
fi

# Sync version across package files
print "Syncing version across package files..."
make sync-version

# Paths to the AUR package repos
RELEASE_REPO="aur-mprisence"
GIT_REPO="aur-mprisence-git"

# Handle release package
if [ "$PUBLISH_RELEASE" = true ]; then
    print "Generating .SRCINFO for release package..."
    generate_srcinfo "packaging/arch/release"

    if [ ! -d "$RELEASE_REPO" ]; then
        print "Cloning release AUR repository..."
        git clone ssh://aur@aur.archlinux.org/mprisence.git "$RELEASE_REPO"
    fi

    print "Updating release package..."
    cp packaging/arch/release/PKGBUILD "$RELEASE_REPO/PKGBUILD"
    cp packaging/arch/release/.SRCINFO "$RELEASE_REPO/.SRCINFO"

    print "Publishing release package..."
    (
        cd "$RELEASE_REPO"
        git add PKGBUILD .SRCINFO
        git commit -m "Update to version $VERSION"
        git push
    )
    print "Successfully published release package version $VERSION"
fi

# Handle git package
if [ "$PUBLISH_GIT" = true ]; then
    print "Generating .SRCINFO for git package..."
    generate_srcinfo "packaging/arch/git"

    if [ ! -d "$GIT_REPO" ]; then
        print "Cloning git AUR repository..."
        git clone ssh://aur@aur.archlinux.org/mprisence-git.git "$GIT_REPO"
    fi

    print "Updating git package..."
    cp packaging/arch/git/PKGBUILD "$GIT_REPO/PKGBUILD"
    cp packaging/arch/git/.SRCINFO "$GIT_REPO/.SRCINFO"

    print "Publishing git package..."
    (
        cd "$GIT_REPO"
        git add PKGBUILD .SRCINFO
        git commit -m "Update to version $VERSION"
        git push
    )
    print "Successfully published git package version $VERSION"
fi

if [ "$PUBLISH_RELEASE" = true ] || [ "$PUBLISH_GIT" = true ]; then
    print "Publishing completed successfully!"
else
    warn "No package type specified for publishing"
    show_help
    exit 1
fi 