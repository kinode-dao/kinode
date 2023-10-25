#!/bin/bash

cd sqlite
cargo build --release --no-default-features --target wasm32-wasi

cd ../sqlite_worker

# Get special clang compiler required to build & link sqlite3 C lib.
mkdir -p target
cd target

WASI_VERSION=20
WASI_VERSION_FULL=${WASI_VERSION}.0
CC_PATH=$(realpath ./wasi-sdk-${WASI_VERSION_FULL}/bin/clang)

# Determine operating system
OS_TYPE="$(uname)"
if [ "$OS_TYPE" = "Darwin" ]; then
    WASI_PLATFORM="macos"
elif [ "$OS_TYPE" = "Linux" ]; then
    WASI_PLATFORM="linux"
else
    echo "Unsupported OS: $OS_TYPE"
    exit 1
fi

if [ ! -e "$CC_PATH" ]; then
    wget https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_VERSION}/wasi-sdk-${WASI_VERSION_FULL}-${WASI_PLATFORM}.tar.gz
    tar xvf wasi-sdk-${WASI_VERSION_FULL}-${WASI_PLATFORM}.tar.gz
fi

CC_PATH=$(realpath ./wasi-sdk-${WASI_VERSION_FULL}/bin/clang)

cd ..

# We write env vars to `.cargo/config.toml` here because:
# 1. Doing `export foo=/path && export bar=/path2 && RUSTFLAGS=baz cargo build ...`
#    does not properly pass the RUSTFLAGS (cargo bug?).
# 2. Specifying `~/path` inside `.cargo/config.toml` doesn't expand.
mkdir -p .cargo

# CC_PATH=$(realpath ~/wasi-sdk/wasi-sdk-20.0/bin/clang)

# Write to the .cargo/config.toml file
cat <<EOF > .cargo/config.toml
[env]
CC_wasm32_wasi = "$CC_PATH"
CARGO_TARGET_WASM32_WASI_LINKER = "$CC_PATH"
EOF

RUSTFLAGS="-C target-feature=-crt-static -C link-arg=-Wl,--no-entry,--export=init,--export=cabi_realloc" cargo build --release --no-default-features --target wasm32-wasi

