export RUST_LOG := "krustlet=info,main=debug"

build:
    cargo build

test:
    cargo clippy
    cargo test

run: _cleanup_kube
    cargo run

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