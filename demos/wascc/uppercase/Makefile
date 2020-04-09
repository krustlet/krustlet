COLOR ?= always # Valid COLOR options: {always, auto, never}
CARGO = cargo --color $(COLOR)
TARGET = ../../../target/wasm32-unknown-unknown
DEBUG = $(TARGET)/debug
RELEASE = $(TARGET)/release
KEYDIR ?= .keys
VERSION = 0.2

.PHONY: all bench build check clean doc test update keys keys-account keys-module

all: build

bench:
	@$(CARGO) bench

build:
	@$(CARGO) build
	wascap sign $(DEBUG)/uppercase.wasm $(DEBUG)/uppercase_signed.wasm -i $(KEYDIR)/account.nk -u $(KEYDIR)/module.nk -s -l -n uppercase

check:
	@$(CARGO) check

clean:
	@$(CARGO) clean

doc:
	@$(CARGO) doc

test: build
	@$(CARGO) test

update:
	@$(CARGO) update

release:
	@$(CARGO) build --release
	wascap sign $(RELEASE)/uppercase.wasm $(RELEASE)/uppercase_signed.wasm -i $(KEYDIR)/account.nk -u $(KEYDIR)/module.nk -s -l -n uppercase 

push:
	wasm-to-oci push ../../../target/wasm32-unknown-unknown/release/uppercase_signed.wasm webassembly.azurecr.io/uppercase-wascc:v$(VERSION)


keys: keys-account
keys: keys-module

keys-account:
	@mkdir -p $(KEYDIR)
	nk gen account > $(KEYDIR)/account.txt
	awk '/Seed/{ print $$2 }' $(KEYDIR)/account.txt > $(KEYDIR)/account.nk

keys-module:
	@mkdir -p $(KEYDIR)
	nk gen module > $(KEYDIR)/module.txt
	awk '/Seed/{ print $$2 }' $(KEYDIR)/module.txt > $(KEYDIR)/module.nk
