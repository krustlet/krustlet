export RUST_LOG := "wascc_host=debug,wascc_provider=debug,wasi_provider=debug,main=debug"

run: run-wascc

build:
    cargo build

prefetch:
    cargo fetch --manifest-path ./Cargo.toml

test:
    cargo fmt --all -- --check
    cargo clippy --workspace
    cargo test --workspace

run-wascc: _cleanup_kube
    # Change directories so we have access to the ./lib dir
    cd ./crates/wascc-provider && cargo run --bin krustlet-wascc --manifest-path ../../Cargo.toml

run-wasi: _cleanup_kube
    # HACK: Temporary step to change to a directory so it has access to a hard
    # coded module. This should be removed once we have image support
    cd ./crates/wasi-provider && cargo run --bin krustlet-wasi --manifest-path ../../Cargo.toml

dockerize:
    docker build -t technosophos/krustlet:latest .

push:
    docker push technosophos/krustlet:latest

itest:
    kubectl create -f examples/greet.yaml
    sleep 5
    for i in 1 2 3 4 5; do sleep 3 && kubectl get po greet; done

_cleanup_kube:
    kubectl delete no krustlet || true
    kubectl delete po greet || true

testfor:
    for i in 1 2 3 4 5; do sleep 3 && echo hello $i; done