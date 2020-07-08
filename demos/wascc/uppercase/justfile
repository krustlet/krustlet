COLOR   := "always" # Valid COLOR options: {always, auto, never}
CARGO   := "cargo --color $(COLOR)"
TARGET  := "target/wasm32-unknown-unknown"
DEBUG   := TARGET + "/debug"
RELEASE := TARGET + "/release"
KEYDIR  := ".keys"
VERSION := "0.3"
NAME    := "uppercase"

all: build

build: keys
	@{{CARGO}} build
	wascap sign {{DEBUG}}/{{NAME}}.wasm {{DEBUG}}/{{NAME}}_signed.wasm -i {{KEYDIR}}/account.nk -u {{KEYDIR}}/module.nk -s -f -l -n {{NAME}}

release: keys
	@{{CARGO}} build --release
	wascap sign {{RELEASE}}/{{NAME}}.wasm {{RELEASE}}/{{NAME}}_signed.wasm -i {{KEYDIR}}/account.nk -u {{KEYDIR}}/module.nk -s -f -l -n {{NAME}}

push:
	wasm-to-oci push {{RELEASE}}/{{NAME}}_signed.wasm webassembly.azurecr.io/{{NAME}}-wascc:v{{VERSION}}


keys: account module

account:
	@mkdir -p {{KEYDIR}}
	nk gen account > {{KEYDIR}}/account.txt
	awk '/Seed/{ print $$2 }' {{KEYDIR}}/account.txt > {{KEYDIR}}/account.nk

module:
	@mkdir -p {{KEYDIR}}
	nk gen module > {{KEYDIR}}/module.txt
	awk '/Seed/{ print $$2 }' {{KEYDIR}}/module.txt > {{KEYDIR}}/module.nk
