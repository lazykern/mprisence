#!/usr/bin/env bash

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

print() {
    printf '%b\n' "${GREEN}==>${NC} $1"
}

warn() {
    printf '%b\n' "${YELLOW}Warning:${NC} $1"
}

error() {
    printf '%b\n' "${RED}Error:${NC} $1" >&2
    exit 1
}

show_help() {
    cat <<'EOF'
Usage: ./scripts/publish-crate.sh [OPTIONS]

Publish the current tagged commit to crates.io.

Options:
  --no-verify   Skip fmt, clippy, test, and dry-run checks
  -h, --help    Show this help message
EOF
}

VERIFY=true

for arg in "$@"; do
    case "$arg" in
        --no-verify)
            VERIFY=false
            ;;
        -h|--help)
            show_help
            exit 0
            ;;
        *)
            error "Unknown option: $arg"
            ;;
    esac
done

[ -f "Cargo.toml" ] || error "This script must be run from the project root"
[ -f "Cargo.lock" ] || warn "Cargo.lock not found; continuing"

if ! command -v cargo >/dev/null 2>&1; then
    error "cargo is required but not installed"
fi

if ! command -v git >/dev/null 2>&1; then
    error "git is required but not installed"
fi

VERSION=$(python - <<'PY'
import tomllib

with open("Cargo.toml", "rb") as f:
    print(tomllib.load(f)["package"]["version"])
PY
) || error "Failed to read package version from Cargo.toml"

TAG=$(git describe --tags --exact-match 2>/dev/null || true)

[ -n "$TAG" ] || error "Current commit is not tagged"

if [ "$TAG" != "$VERSION" ] && [ "$TAG" != "v$VERSION" ]; then
    error "Git tag '$TAG' does not match Cargo.toml version '$VERSION'"
fi

if [ -n "$(git status --porcelain)" ]; then
    error "Working tree is not clean"
fi

print "Preparing to publish mprisence ${VERSION} from tag ${TAG}"

if [ "$VERIFY" = true ]; then
    print "Running formatting check..."
    cargo fmt --check

    print "Running clippy..."
    cargo clippy --all-targets --all-features -- -D warnings

    print "Running tests..."
    cargo test

    print "Running cargo publish dry-run..."
    cargo publish --dry-run
else
    warn "Skipping verification checks"
fi

print "Publishing crate to crates.io..."
cargo publish

print "Successfully published mprisence ${VERSION}"
