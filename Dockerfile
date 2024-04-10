# syntax=docker/dockerfile:1
FROM rust AS builder

COPY . /tmp/source

WORKDIR /tmp/source

RUN apt-get update
RUN apt-get install clang -y

RUN cargo install wasm-tools && \
    rustup install nightly && \
    rustup target add wasm32-wasi && \
    rustup target add wasm32-wasi --toolchain nightly && \
    cargo install cargo-wasi

RUN cargo +nightly build -p kinode --release

FROM debian:12-slim

RUN apt-get update
RUN apt-get install openssl -y

COPY --from=builder /tmp/source/target/release/kinode /bin/kinode

ENV LD_LIBRARY_PATH=/lib
ENV RUST_BACKTRACE=full
ENTRYPOINT [ "/bin/kinode" ]
CMD [ "/kinode-home" ]

EXPOSE 8080
EXPOSE 9000