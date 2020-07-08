export RUST_LOG := "wascc_host=debug,wascc_provider=debug,wasi_provider=debug,main=debug"
export PFX_PASSWORD := "testing"
export CONFIG_DIR := env_var_or_default('CONFIG_DIR', '$HOME/.krustlet/config')

run: build

build +FLAGS='':
    cargo build {{FLAGS}}

test:
    cargo fmt --all -- --check
    cargo clippy --workspace
    cargo test --workspace --lib
    cargo test --doc --all

test-e2e:
    cargo test --test integration_tests

run-wascc +FLAGS='': bootstrap
    KUBECONFIG=$(eval echo $CONFIG_DIR)/kubeconfig-wascc cargo run --bin krustlet-wascc {{FLAGS}} -- --node-name krustlet-wascc --port 3000 --bootstrap-file $(eval echo $CONFIG_DIR)/bootstrap.conf --cert-file $(eval echo $CONFIG_DIR)/krustlet-wascc.crt --private-key-file $(eval echo $CONFIG_DIR)/krustlet-wascc.key

run-wasi +FLAGS='': bootstrap
    KUBECONFIG=$(eval echo $CONFIG_DIR)/kubeconfig-wasi cargo run --bin krustlet-wasi {{FLAGS}} -- --node-name krustlet-wasi --port 3001 --bootstrap-file $(eval echo $CONFIG_DIR)/bootstrap.conf --cert-file $(eval echo $CONFIG_DIR)/krustlet-wasi.crt --private-key-file $(eval echo $CONFIG_DIR)/krustlet-wasi.key

bootstrap:
    @# This is to get around an issue with the default function returning a string that gets escaped
    @mkdir -p $(eval echo $CONFIG_DIR)
    @test -f  $(eval echo $CONFIG_DIR)/bootstrap.conf || CONFIG_DIR=$(eval echo $CONFIG_DIR) ./docs/howto/assets/bootstrap.sh
    @chmod 600 $(eval echo $CONFIG_DIR)/*
