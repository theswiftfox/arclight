PREFIX ?= /usr/local
BINDIR = $(PREFIX)/bin
DATADIR = $(PREFIX)/share
ICONDIR = $(DATADIR)/icons/hicolor
DESKTOPDIR = $(DATADIR)/applications

.PHONY: build install uninstall clean

build:
	cargo build --release

install: build
	install -Dm755 target/release/arclight $(DESTDIR)$(BINDIR)/arclight
	install -Dm644 dist/arclight.desktop $(DESTDIR)$(DESKTOPDIR)/arclight.desktop
	install -Dm644 assets/arclight.svg $(DESTDIR)$(ICONDIR)/scalable/apps/arclight.svg
	install -Dm644 assets/arclight-16.png $(DESTDIR)$(ICONDIR)/16x16/apps/arclight.png
	install -Dm644 assets/arclight-24.png $(DESTDIR)$(ICONDIR)/24x24/apps/arclight.png
	install -Dm644 assets/arclight-32.png $(DESTDIR)$(ICONDIR)/32x32/apps/arclight.png
	install -Dm644 assets/arclight-48.png $(DESTDIR)$(ICONDIR)/48x48/apps/arclight.png
	install -Dm644 assets/arclight-64.png $(DESTDIR)$(ICONDIR)/64x64/apps/arclight.png
	install -Dm644 assets/arclight-128.png $(DESTDIR)$(ICONDIR)/128x128/apps/arclight.png
	install -Dm644 assets/arclight-256.png $(DESTDIR)$(ICONDIR)/256x256/apps/arclight.png
ifndef DESTDIR
	-gtk-update-icon-cache $(ICONDIR)/.. 2>/dev/null || true
endif

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/arclight
	rm -f $(DESTDIR)$(DESKTOPDIR)/arclight.desktop
	rm -f $(DESTDIR)$(ICONDIR)/scalable/apps/arclight.svg
	for size in 16 24 32 48 64 128 256; do \
		rm -f $(DESTDIR)$(ICONDIR)/$${size}x$${size}/apps/arclight.png; \
	done

clean:
	cargo clean
