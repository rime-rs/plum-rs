SRCDIR ?= $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
OUTPUT ?= $(SRCDIR)/output
PLUM_DIR ?= $(SRCDIR)/.plum
PREFIX ?= /usr
DESTDIR ?=
RIME_DATA_DIR ?= $(PREFIX)/share/rime-data
RIME_INSTALL ?= cargo run --manifest-path "$(SRCDIR)/Cargo.toml" --release --bin rime-install --
INSTALL_ARGS = --plum-dir "$(PLUM_DIR)" --rime-dir "$(OUTPUT)"
VERSION ?= $(shell date +%Y%m%d)
.DEFAULT_GOAL := preset

preset extra all minimal:
	$(RIME_INSTALL) $(INSTALL_ARGS) --build $@
preset-bin all-bin minimal-bin:
	$(RIME_INSTALL) $(INSTALL_ARGS) --build $(patsubst %-bin,%,$@) --compile
build:
	$(RIME_INSTALL) $(INSTALL_ARGS) --compile
install:
	$(RIME_INSTALL) $(INSTALL_ARGS) --install-shared "$(DESTDIR)$(RIME_DATA_DIR)"
clean:
	$(RIME_INSTALL) $(INSTALL_ARGS) --clean-output
dist:
	$(RIME_INSTALL) $(INSTALL_ARGS) --source-dir "$(SRCDIR)" --dist "$(SRCDIR)/plum-rs-$(VERSION).tar.gz"
.PHONY: preset extra all minimal preset-bin all-bin minimal-bin build install clean dist
