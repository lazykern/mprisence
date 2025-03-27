# Default paths for local installation
PREFIX ?= $(HOME)/.local
CONFIG_DIR ?= $(HOME)/.config/mprisence
SYSTEMD_USER_DIR ?= $(HOME)/.config/systemd/user

# System-wide paths (used by packagers)
SYS_PREFIX ?= /usr
SYS_CONFIG_DIR ?= /etc/mprisence
SYS_SYSTEMD_USER_DIR ?= $(SYS_PREFIX)/lib/systemd/user

# Installation options
ENABLE_SERVICE ?= 1

# Version management
CARGO_VERSION != grep '^version = ' Cargo.toml | head -n1 | cut -d'"' -f2

.PHONY: all install-local uninstall-local clean help pkg-prepare check-deps build sync-version check-existing-install

all: install-local

check-existing-install:
	@echo "Checking for existing installations..."
	@if command -v mprisence >/dev/null 2>&1; then \
		SYSTEM_PATH=$$(command -v mprisence); \
		echo "Found mprisence at: $$SYSTEM_PATH"; \
		if [ "$$SYSTEM_PATH" != "$(PREFIX)/bin/mprisence" ]; then \
			echo "WARNING: Existing mprisence installation found at $$SYSTEM_PATH"; \
			echo "This might conflict with the local installation."; \
			echo "If installed via package manager, you should remove it first:"; \
			echo "  sudo pacman -R mprisence"; \
			echo "  systemctl --user disable --now mprisence"; \
			echo ""; \
			echo "Press Ctrl+C to abort, or Enter to continue anyway..."; \
			read REPLY; \
		else \
			echo "Found existing local installation at $$SYSTEM_PATH"; \
		fi; \
	else \
		echo "No existing mprisence installation found."; \
	fi

check-deps:
	@command -v cargo >/dev/null 2>&1 || { echo "Error: cargo is required but not installed." >&2; exit 1; }

build: check-deps
	cargo build --release

sync-version:
	@echo "Syncing version $(CARGO_VERSION) across package files..."
	@if ! cargo install --quiet semver-cli 2>/dev/null; then \
		echo "Installing semver-cli..."; \
		cargo install semver-cli; \
	fi
	@if ! semver "$(CARGO_VERSION)" >/dev/null 2>&1; then \
		echo "Error: Invalid version format in Cargo.toml"; \
		exit 1; \
	fi
	@sed -i 's/^pkgver=.*$$/pkgver=$(CARGO_VERSION)/' packaging/arch/release/PKGBUILD
	@echo "Version updated in release package file"

install-local: build
	@$(MAKE) check-existing-install
	@echo "Starting installation..."
	@if systemctl --user is-active mprisence >/dev/null 2>&1; then \
		echo "Service is running, will restart after installation"; \
		SHOULD_RESTART=1; \
	fi
	install -Dm755 target/release/mprisence "$(PREFIX)/bin/mprisence"
	install -dm755 "$(CONFIG_DIR)"
	install -Dm644 config/config.example.toml "$(CONFIG_DIR)/config.example.toml"
	@if [ ! -f "$(CONFIG_DIR)/config.toml" ]; then \
		echo "Creating default config..."; \
		cp "$(CONFIG_DIR)/config.example.toml" "$(CONFIG_DIR)/config.toml"; \
	fi
	@if [ -f "$(SYSTEMD_USER_DIR)/mprisence.service" ]; then \
		if ! cmp -s mprisence.service "$(SYSTEMD_USER_DIR)/mprisence.service"; then \
			NEED_RELOAD=1; \
		fi; \
	else \
		NEED_RELOAD=1; \
	fi; \
	sed "s|@BINDIR@|$(PREFIX)/bin|g" mprisence.service > mprisence.service.tmp; \
	install -Dm644 mprisence.service.tmp "$(SYSTEMD_USER_DIR)/mprisence.service"; \
	rm mprisence.service.tmp
	@if [ "$$NEED_RELOAD" = "1" ]; then \
		systemctl --user daemon-reload || true; \
	fi
	@if [ "$(ENABLE_SERVICE)" = "1" ]; then \
		if ! systemctl --user is-enabled mprisence >/dev/null 2>&1; then \
			systemctl --user enable mprisence || true; \
		fi; \
		if ! systemctl --user is-active mprisence >/dev/null 2>&1; then \
			systemctl --user start mprisence || { \
				echo "Service failed to start, check: journalctl --user -u mprisence"; \
				exit 1; \
			}; \
		elif [ "$$SHOULD_RESTART" = "1" ]; then \
			if systemctl --user restart mprisence; then \
				echo "Service restarted successfully"; \
			else \
				echo "Service restart failed, check: journalctl --user -u mprisence"; \
				exit 1; \
			fi; \
		fi; \
	else \
		echo "Service installation complete. To start:"; \
		echo "  systemctl --user enable --now mprisence"; \
	fi

uninstall-local:
	systemctl --user disable --now mprisence || true
	rm -f "$(PREFIX)/bin/mprisence"
	rm -rf "$(CONFIG_DIR)"
	rm -f "$(SYSTEMD_USER_DIR)/mprisence.service"
	systemctl --user daemon-reload || true

clean:
	cargo clean

pkg-prepare: build
	install -Dm755 target/release/mprisence "$(DESTDIR)$(SYS_PREFIX)/bin/mprisence"
	install -dm755 "$(DESTDIR)$(SYS_CONFIG_DIR)"
	install -Dm644 config/config.example.toml "$(DESTDIR)$(SYS_CONFIG_DIR)/config.example.toml"
	@sed "s|@BINDIR@|$(SYS_PREFIX)/bin|g" mprisence.service > mprisence.service.tmp
	install -Dm644 mprisence.service.tmp "$(DESTDIR)$(SYS_SYSTEMD_USER_DIR)/mprisence.service"
	rm mprisence.service.tmp
	@echo "=== MPRISence package preparation complete ==="
	@echo "Files installed to DESTDIR:"
	@echo "  Binary:       $(DESTDIR)$(SYS_PREFIX)/bin/mprisence"
	@echo "  Example conf: $(DESTDIR)$(SYS_CONFIG_DIR)/config.example.toml"
	@echo "  Service file: $(DESTDIR)$(SYS_SYSTEMD_USER_DIR)/mprisence.service"
	@echo ""
	@echo "Note for packagers: Don't forget to include post-install messages about:"
	@echo "  - User configuration (~/.config/mprisence/config.toml)"
	@echo "  - Service activation (systemctl --user enable --now mprisence)"

help:
	@echo "Usage:"
	@echo "  make                    Install for current user (enables service by default)"
	@echo "  make install-local      Same as 'make'"
	@echo "  make build              Build only without installing"
	@echo "  make uninstall-local    Remove mprisence from your system"
	@echo "  make clean              Clean build files"
	@echo "  make pkg-prepare        Prepare files for packaging (maintainers)"
	@echo "  make sync-version       Sync version from Cargo.toml to release package"
	@echo
	@echo "Installation Options:"
	@echo "  ENABLE_SERVICE=0        Install without enabling the service"
	@echo "  PREFIX=$(PREFIX)        Install binary to different prefix"
	@echo "  CONFIG_DIR=$(CONFIG_DIR)        Config directory location"
	@echo
	@echo "Default Paths:"
	@echo "  Binary:     $(PREFIX)/bin/mprisence"
	@echo "  Config:     $(CONFIG_DIR)"
	@echo "  Service:    $(SYSTEMD_USER_DIR)/mprisence.service"