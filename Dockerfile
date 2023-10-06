FROM rust:bookworm AS builder

WORKDIR /src
COPY . .
RUN cargo build --release

FROM arm64v8/debian:bookworm-slim
WORKDIR /app

# Install libssl1.1
RUN apt-get update && apt-get install -y libssl-dev

# Copy the binary from the builder stage
COPY --from=builder /src/target/release/uqbar /app/uqbar

ENTRYPOINT ["/app/uqbar"]
CMD ["/app/userdir", "--rpc=default_rpc_value"]

