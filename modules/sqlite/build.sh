#!/bin/bash

# export CC_wasm32_wasi="$(realpath ~/wasi-sdk/wasi-sdk-20.0/bin/clang)" && export CARGO_TARGET_WASM32_WASI_LINKER="$(realpath ~/wasi-sdk/wasi-sdk-20.0/bin/clang)" && export RUSTFLAGS="-C target-feature=-crt-static -C link-arg=-Wl,--no-entry,--export=init,--export=cabi_realloc" && cargo build --release --no-default-features --target wasm32-wasi
# RUSTFLAGS="-C target-feature=-crt-static -C link-arg=-Wl,--no-entry,--export=init,--export=cabi_realloc" cargo build --release --no-default-features --target wasm32-wasi

# We write env vars to `.cargo/config.toml` here because:
# 1. Doing `export foo=/path && export bar=/path2 && RUSTFLAGS=baz cargo build ...`
#    does not properly pass the RUSTFLAGS (cargo bug?).
# 2. Specifying `~/path` inside `.cargo/config.toml` doesn't expand.

mkdir -p .cargo

CC_PATH=$(realpath ~/wasi-sdk/wasi-sdk-20.0/bin/clang)

# Write to the .cargo/config.toml file
cat <<EOF > .cargo/config.toml
[env]
CC_wasm32_wasi = "$CC_PATH"
CARGO_TARGET_WASM32_WASI_LINKER = "$CC_PATH"
EOF

RUSTFLAGS="-C target-feature=-crt-static -C link-arg=-Wl,--no-entry,--export=init,--export=cabi_realloc" cargo build --release --no-default-features --target wasm32-wasi
