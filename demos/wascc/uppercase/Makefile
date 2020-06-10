COLOR ?= always # Valid COLOR options: {always, auto, never}
CARGO = cargo --color $(COLOR)
TARGET = target/wasm32-unknown-unknown
DEBUG = $(TARGET)/debug
RELEASE = $(TARGET)/release
KEYDIR ?= .keys
VERSION = 0.3

.PHONY: all bench build check clean doc test update keys

all: build

bench:
	@$(CARGO) bench

build: keys
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

release: keys
	@$(CARGO) build --release
	wascap sign $(RELEASE)/uppercase.wasm $(RELEASE)/uppercase_signed.wasm -i $(KEYDIR)/account.nk -u $(KEYDIR)/module.nk -s -l -n uppercase

push:
	wasm-to-oci push $(RELEASE)/uppercase_signed.wasm webassembly.azurecr.io/uppercase-wascc:v$(VERSION)


keys: $(KEYDIR)/account.nk $(KEYDIR)/module.nk

$(KEYDIR)/account.nk:
	@mkdir -p $(KEYDIR)
	nk gen account > $(KEYDIR)/account.txt
	awk '/Seed/{ print $$2 }' $(KEYDIR)/account.txt > $(KEYDIR)/account.nk

$(KEYDIR)/module.nk:
	@mkdir -p $(KEYDIR)
	nk gen module > $(KEYDIR)/module.txt
	awk '/Seed/{ print $$2 }' $(KEYDIR)/module.txt > $(KEYDIR)/module.nk
