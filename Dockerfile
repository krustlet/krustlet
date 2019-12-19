FROM rust:1.38
WORKDIR /usr/src/krustlet

COPY Cargo.toml .
COPY Cargo.lock .

# Layer hack: Build an empty program to compile dependencies and place on their own layer.
# This cuts down build time
RUN mkdir -p ./src/ && \
    echo 'fn main() {}' > ./src/main.rs && \
    echo '' > ./src/lib.rs
RUN cargo fetch
RUN cargo build --release && \
    rm -rf ./target/release/.fingerprint/krustlet-*

# Build real binaries now
COPY ./src ./src
RUN cargo build --release
CMD ["/usr/src/krustlet/target/release/krustlet"]