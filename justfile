export RUST_LOG := "krustlet=info,main=debug"

build:
    cargo build

test:
    cargo test
    cargo clippy

run:
    cargo run

dockerize:
    docker build -t technosophos/krustlet:latest .

just push:
    docker push technosophos/krustlet:latest