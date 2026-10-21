# dune: build, check and install.
#
#   make install          dune, dune-finder and dune-session into ~/.cargo/bin, plus the
#                         portal settings into ~/.config (no root needed)
#   make install-session  the "Dune" entry of the login screen (asks for sudo)
#   make check            tests and clippy for the whole workspace
#
# Run it as your own user, not with sudo: the paths below come from $HOME.
# The screenshot and record keys need grim, slurp, wf-recorder, wl-clipboard and
# libnotify installed (pacman -S grim slurp wf-recorder wl-clipboard libnotify).
# A running dune keeps the old binary until the session is restarted.

CARGO      ?= cargo
BINDIR     ?= $(HOME)/.cargo/bin
CONFIGDIR  ?= $(or $(XDG_CONFIG_HOME),$(HOME)/.config)
SESSIONDIR ?= /usr/share/wayland-sessions

.PHONY: all build check install install-bin install-portals install-session uninstall

all: build

build:
	$(CARGO) build --release --workspace

check:
	$(CARGO) test --workspace
	$(CARGO) clippy --workspace --all-targets

install: install-bin install-portals

install-bin:
	$(CARGO) install --path . --locked
	$(CARGO) install --path dune-finder --locked
	install -Dm755 dist/dune-session $(BINDIR)/dune-session

# Screen sharing and screenshots through xdg-desktop-portal-wlr (see dist/dune-portals.conf).
install-portals:
	install -Dm644 dist/dune-portals.conf $(CONFIGDIR)/xdg-desktop-portal/dune-portals.conf
	install -Dm644 dist/xdg-desktop-portal-wlr.dune $(CONFIGDIR)/xdg-desktop-portal-wlr/dune

# The login screen does not see ~/.cargo/bin, so Exec gets the absolute path.
install-session:
	mkdir -p target
	sed 's|^Exec=.*|Exec=$(BINDIR)/dune-session|' dist/dune.desktop > target/dune.desktop
	sudo install -Dm644 target/dune.desktop $(SESSIONDIR)/dune.desktop

uninstall:
	-$(CARGO) uninstall dune
	-$(CARGO) uninstall dune-finder
	rm -f $(BINDIR)/dune-session
	rm -f $(CONFIGDIR)/xdg-desktop-portal/dune-portals.conf $(CONFIGDIR)/xdg-desktop-portal-wlr/dune
	@echo "The login entry stays; remove it with: sudo rm $(SESSIONDIR)/dune.desktop"
