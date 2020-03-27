export RUST_LOG := "wascc_host=debug,wascc_provider=debug,wasi_provider=debug,main=debug"
export PFX_PASSWORD := "testing"
export KEY_DIR := env_var_or_default('KEY_DIR', '$HOME/.krustlet/config')

run: run-wascc

build:
    cargo build

prefetch:
    cargo fetch --manifest-path ./Cargo.toml

test:
    cargo fmt --all -- --check
    cargo clippy --workspace
    cargo test --workspace --lib
    cargo test --doc --all

test-e2e:
    cargo test --test integration_tests

run-wascc: _cleanup_kube bootstrap-ssl
    @# Change directories so we have access to the ./lib dir
    cd ./crates/wascc-provider && cargo run --bin krustlet-wascc --manifest-path ../../Cargo.toml -- --node-name krustlet-wascc --port 3000

run-wasi: _cleanup_kube bootstrap-ssl
    cargo run --bin krustlet-wasi -- --node-name krustlet-wasi --port 3001

dockerize:
    docker build -t technosophos/krustlet:latest .

push:
    docker push technosophos/krustlet:latest

bootstrap-ssl:
    @# This is to get around an issue with the default function returning a string that gets escaped
    @mkdir -p $(eval echo $KEY_DIR)
    @test -f  $(eval echo $KEY_DIR)/host.key && test -f $(eval echo $KEY_DIR)/host.cert || openssl req -x509 -sha256 -newkey rsa:2048 -keyout $(eval echo $KEY_DIR)/host.key -out $(eval echo $KEY_DIR)/host.cert -days 365 -nodes -subj "/C=AU/ST=./L=./O=./OU=./CN=."
    @test -f $(eval echo $KEY_DIR)/certificate.pfx || openssl pkcs12 -export -out  $(eval echo $KEY_DIR)/certificate.pfx -inkey  $(eval echo $KEY_DIR)/host.key -in  $(eval echo $KEY_DIR)/host.cert -password "pass:${PFX_PASSWORD}"
    @chmod 400 $(eval echo $KEY_DIR)/*

docs: 	
    cargo +nightly  doc --features docs --open

_cleanup_kube:
    kubectl delete node krustlet-wasi krustlet-wascc || true
    kubectl delete --all pods --namespace=default || true
