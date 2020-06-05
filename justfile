export RUST_LOG := "wascc_host=debug,wascc_provider=debug,wasi_provider=debug,main=debug"
export PFX_PASSWORD := "testing"
export CONFIG_DIR := env_var_or_default('CONFIG_DIR', '$HOME/.krustlet/config')

run: run-wascc

build:
    cargo build

test:
    cargo fmt --all -- --check
    cargo clippy --workspace
    cargo test --workspace --lib
    cargo test --doc --all

test-e2e:
    cargo test --test integration_tests

run-wascc: (bootstrap "bootstrap-wascc.conf")
    KUBECONFIG=$(eval echo $CONFIG_DIR)/kubeconfig cargo run --bin krustlet-wascc -- --node-name krustlet-wascc --port 3000 --bootstrap-file $(eval echo $CONFIG_DIR)/bootstrap-wascc.conf --tls-cert-file $(eval echo $CONFIG_DIR)/krustlet-wascc.crt --tls-private-key-file $(eval echo $CONFIG_DIR)/krustlet-wascc.key

run-wasi: (bootstrap "bootstrap-wasi.conf")
    KUBECONFIG=$(eval echo $CONFIG_DIR)/kubeconfig cargo run --bin krustlet-wasi -- --node-name krustlet-wasi --port 3001 --bootstrap-file $(eval echo $CONFIG_DIR)/bootstrap-wasi.conf --tls-cert-file $(eval echo $CONFIG_DIR)/krustlet-wasi.crt --tls-private-key-file $(eval echo $CONFIG_DIR)/krustlet-wasi.key

bootstrap file_name="bootstrap.conf":
    @# This is to get around an issue with the default function returning a string that gets escaped
    @mkdir -p $(eval echo $CONFIG_DIR)
    @test -f  $(eval echo $CONFIG_DIR)/kubeconfig || CONFIG_DIR=$(eval echo $CONFIG_DIR) FILE_NAME={{file_name}} ./hack/bootstrap.sh
    @chmod 600 $(eval echo $CONFIG_DIR)/*
