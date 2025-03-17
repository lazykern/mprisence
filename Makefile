.PHONY: build debug clean install uninstall test release package autostart config tar-release

# Get version from Cargo.toml
VERSION := $(shell grep "^version" < Cargo.toml | cut -d '"' -f 2)
ARCH := x86_64

build:
	cargo build --release

debug:
	cargo build

clean:
	cargo clean

test:
	cargo test

install: build
	@echo "Installing mprisence..."
	sudo cp target/release/mprisence /usr/local/bin/
	@echo "mprisence has been installed to /usr/local/bin/"

uninstall:
	@echo "Uninstalling mprisence..."
	sudo rm -f /usr/local/bin/mprisence
	@echo "mprisence has been uninstalled"

release: build tar-release
	@echo "Release v$(VERSION) prepared"

tar-release:
	@echo "Creating release archive..."
	@mkdir -p release
	@cp target/release/mprisence release/
	@cp LICENSE release/
	@cp systemd/mprisence.service release/
	cd release && tar -czf "mprisence-$(VERSION)-$(ARCH).tar.gz" mprisence mprisence.service LICENSE
	@mv release/mprisence-$(VERSION)-$(ARCH).tar.gz .
	@rm -rf release
	@echo "Created mprisence-$(VERSION)-$(ARCH).tar.gz"
	@sha256sum "mprisence-$(VERSION)-$(ARCH).tar.gz"

package: release
	@echo "Updating PKGBUILD..."
	@sed -i "s/^pkgver=.*/pkgver=$(VERSION)/" PKGBUILD
	@SHA256=$(shell sha256sum "mprisence-$(VERSION)-$(ARCH).tar.gz" | cut -d ' ' -f 1) && \
	sed -i "/sha256sums=/s/sha256sums=('.*'/sha256sums=('$$SHA256'/" PKGBUILD
	@echo "PKGBUILD updated to version $(VERSION)"

autostart:
	@echo "Setting up systemd service for autostart..."
	@mkdir -p "$(HOME)/.config/systemd/user"
	@cp systemd/mprisence.service "$(HOME)/.config/systemd/user/"
	@systemctl --user daemon-reload
	@systemctl --user enable --now mprisence.service
	@echo "mprisence service enabled and started"

config:
	@echo "Setting up example configuration..."
	@mkdir -p "$(HOME)/.config/mprisence"
	@cp config/example.toml "$(HOME)/.config/mprisence/config.toml"
	@echo "Example configuration copied to $(HOME)/.config/mprisence/config.toml"

deps-debian:
	sudo apt install build-essential libssl-dev libdbus-1-dev pkg-config

deps-arch:
	sudo pacman -S base-devel openssl dbus pkgconf

deps-redhat:
	sudo yum groupinstall 'Development Tools'
	sudo yum install openssl-devel dbus-devel pkgconfig

help:
	@echo "mprisence Makefile targets:"
	@echo "  build         - Build release version of mprisence"
	@echo "  debug         - Build debug version of mprisence"
	@echo "  clean         - Clean build artifacts"
	@echo "  test          - Run tests"
	@echo "  install       - Build and install mprisence to /usr/local/bin"
	@echo "  uninstall     - Remove mprisence from /usr/local/bin"
	@echo "  release       - Prepare a release (build and create tarball)"
	@echo "  package       - Create release and update PKGBUILD version"
	@echo "  autostart     - Set up systemd service for autostart"
	@echo "  config        - Create example configuration in ~/.config/mprisence"
	@echo "  deps-debian   - Install dependencies for Debian-based systems"
	@echo "  deps-arch     - Install dependencies for Arch Linux"
	@echo "  deps-redhat   - Install dependencies for Red Hat-based systems"
	@echo "  help          - Show this help message" 