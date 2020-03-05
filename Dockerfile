FROM rust:1.41
WORKDIR /usr/src/krustlet

RUN curl --proto '=https' --tlsv1.2 -sSf https://just.systems/install.sh | bash -s -- --to /usr/local/bin

COPY Cargo.toml .
COPY Cargo.lock .
COPY justfile .
COPY crates ./crates

# Layer hack: Build an empty program to compile dependencies and place on their own layer.
# This cuts down build time
RUN mkdir -p ./examples/ && \
    echo 'fn main() {}' > ./examples/empty.rs
RUN just prefetch
RUN cargo build --example empty --release && \
    rm -rf ./target/release/.fingerprint/krustlet-*

# Build real binaries now
RUN rm ./src/main.rs ./src/lib.rs
COPY ./src ./src

RUN cargo build --release
CMD ["/usr/src/krustlet/target/release/krustlet"]