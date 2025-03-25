PREFIX ?= /usr
DESTDIR ?=

.PHONY: all install uninstall clean sync-version

all:
	cargo build --release

install:
	# Create directories
	install -Dm755 target/release/mprisence "$(DESTDIR)$(PREFIX)/bin/mprisence"
	install -Dm644 mprisence.service "$(DESTDIR)/usr/lib/systemd/user/mprisence.service"
	install -Dm644 config/example.toml "$(DESTDIR)/etc/mprisence/config.example.toml"
	install -Dm644 config/default.toml "$(DESTDIR)/etc/mprisence/config.default.toml"
	install -Dm644 README.md "$(DESTDIR)/usr/share/doc/mprisence/README.md"

uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/mprisence"
	rm -f "$(DESTDIR)/usr/lib/systemd/user/mprisence.service"
	rm -rf "$(DESTDIR)/etc/mprisence"
	rm -rf "$(DESTDIR)/usr/share/doc/mprisence"

clean:
	cargo clean

sync-version:
	@VERSION=$$(grep '^version = ' Cargo.toml | cut -d '"' -f2 | sed 's/-/./g'); \
	sed -i "s/^pkgver=.*/pkgver=$$VERSION/" pkg/mprisence/PKGBUILD 