export RUST_LOG := "wascc_provider=debug,wasi_provider=debug,main=debug"

build:
    cargo build

test:
    cargo clippy
    cargo test --workspace

run-wascc: _cleanup_kube
    # Change directories so we have access to the ./lib dir
    cd ./crates/wascc-provider && cargo run --bin krustlet-wascc --manifest-path ../../Cargo.toml

run-wasi: _cleanup_kube
    cargo run --bin krustlet-wasi

dockerize:
    docker build -t technosophos/krustlet:latest .

push:
    docker push technosophos/krustlet:latest

itest:
    kubectl create -f examples/greet.yaml
    sleep 5
    for i in 1 2 3 4 5; do sleep 3 && kubectl get po greet2; done

_cleanup_kube:
    kubectl delete no krustlet || true
    kubectl delete po greet || true

testfor:
    for i in 1 2 3 4 5; do sleep 3 && echo hello $i; done