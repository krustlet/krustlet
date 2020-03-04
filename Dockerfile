FROM rust:1.38
WORKDIR /usr/src/krustlet

RUN curl --proto '=https' --tlsv1.2 -sSf https://just.systems/install.sh | bash -s -- --to /usr/local/bin

COPY Cargo.toml .
COPY Cargo.lock .
COPY justfile .
COPY crates ./crates

# Layer hack: Build an empty program to compile dependencies and place on their own layer.
# This cuts down build time
RUN mkdir -p ./src/ && \
    echo 'fn main() {}' > ./src/main.rs && \
    echo '' > ./src/lib.rs
RUN just prefetch
RUN cargo build --release && \
    rm -rf ./target/release/.fingerprint/krustlet-*

# Build real binaries now
COPY ./src ./src
RUN cargo build --release
CMD ["/usr/src/krustlet/target/release/krustlet"]