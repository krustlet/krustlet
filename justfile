export RUST_LOG := "wasi_provider=debug,main=debug,kubelet=debug,warp=debug"
export PFX_PASSWORD := "testing"
export CONFIG_DIR := env_var_or_default('CONFIG_DIR', '$HOME/.krustlet/config')

# For backward compatibility with those running `just run-wasi`
run-wasi: run

build +FLAGS='':
    cargo build {{FLAGS}}
    cd crates/krator && cargo build {{FLAGS}} --example=moose --features=admission-webhook,derive

lint-docs:
    markdownlint '**/*.md' -c .markdownlint.json

ui-test +FLAGS='':
    cargo test --test ui {{FLAGS}}

test:
    cargo fmt --all -- --check
    cargo clippy --workspace
    cargo test --workspace --lib
    cargo test --doc --all

test-e2e:
    cargo test --test integration_tests

test-e2e-standalone:
    netstat -tulnp || true
    curl -vvv localhost:3001 || true
    cargo run --bin oneclick

test-e2e-ci:
    KRUSTLET_TEST_ENV=ci cargo test --test integration_tests

test-e2e-standalone-ci:
    KRUSTLET_TEST_ENV=ci cargo run --bin oneclick

run +FLAGS='': bootstrap
    KUBECONFIG=$(eval echo $CONFIG_DIR)/kubeconfig-wasi cargo run --bin krustlet-wasi {{FLAGS}} -- --node-name krustlet-wasi --port 3001 --bootstrap-file $(eval echo $CONFIG_DIR)/bootstrap.conf --cert-file $(eval echo $CONFIG_DIR)/krustlet-wasi.crt --private-key-file $(eval echo $CONFIG_DIR)/krustlet-wasi.key

bootstrap:
    @# This is to get around an issue with the default function returning a string that gets escaped
    @mkdir -p $(eval echo $CONFIG_DIR)
    @test -f  $(eval echo $CONFIG_DIR)/bootstrap.conf || CONFIG_DIR=$(eval echo $CONFIG_DIR) ./docs/howto/assets/bootstrap.sh
    @chmod 600 $(eval echo $CONFIG_DIR)/*
