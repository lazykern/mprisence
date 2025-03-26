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
CARGO_VERSION != grep '^version = ' Cargo.toml | head -n1 | cut -d'"' -f2 | sed 's/-beta/\.beta/'

.PHONY: all install-local uninstall-local clean help pkg-prepare check-deps build sync-version

# Default target: local user installation
all: install-local

check-deps:
	@command -v cargo >/dev/null 2>&1 || { echo "Error: cargo is required but not installed." >&2; exit 1; }

build: check-deps
	cargo build --release

# Sync version across package files
sync-version:
	@echo "Syncing version $(CARGO_VERSION) across package files..."
	@sed -i 's/^pkgver=.*$$/pkgver=$(CARGO_VERSION)/' packaging/arch/release/PKGBUILD
	@echo "Version updated in release package file"

# Local user installation
install-local: build
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

# Local user uninstallation
uninstall-local:
	systemctl --user disable --now mprisence || true
	rm -f "$(PREFIX)/bin/mprisence"
	rm -rf "$(CONFIG_DIR)"
	rm -f "$(SYSTEMD_USER_DIR)/mprisence.service"
	systemctl --user daemon-reload || true

# Clean build artifacts
clean:
	cargo clean

# System-wide installation preparation (for packagers)
pkg-prepare: build
	install -Dm755 target/release/mprisence "$(DESTDIR)$(SYS_PREFIX)/bin/mprisence"
	install -dm755 "$(DESTDIR)$(SYS_CONFIG_DIR)"
	install -Dm644 config/config.example.toml "$(DESTDIR)$(SYS_CONFIG_DIR)/config.example.toml"
	@sed "s|@BINDIR@|$(SYS_PREFIX)/bin|g" mprisence.service > mprisence.service.tmp
	install -Dm644 mprisence.service.tmp "$(DESTDIR)$(SYS_SYSTEMD_USER_DIR)/mprisence.service"
	rm mprisence.service.tmp

# Show help
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