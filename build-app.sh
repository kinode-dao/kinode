#!/bin/bash

release_flag=""

if [ $# -ne 1 ] && [ $# -ne 2 ]; then
    echo "Usage: $0 <name> [--release]"
    exit 1
fi

name="$1"

if [[ "$2" == "--release" ]]; then
    release_flag="--release"
fi

pwd=$(pwd)

# Check if the --release flag is present
if [[ "$@" == *"--release"* ]]; then
    release_flag="--release"
fi

rm -rf "$pwd/modules/$name/wit" || { echo "Command failed"; exit 1; }
cp -r wit "$pwd/modules/$name" || { echo "Command failed"; exit 1; }
mkdir -p "$pwd/modules/$name/target/bindings/$name" || { echo "Command failed"; exit 1; }

cp target.wasm "$pwd/modules/$name/target/bindings/$name/" || { echo "Command failed"; exit 1; }
cp world "$pwd/modules/$name/target/bindings/$name/" || { echo "Command failed"; exit 1; }

mkdir -p "$pwd/modules/$name/target/wasm32-unknown-unknown/release" || { echo "Command failed"; exit 1; }

# Build the module using Cargo
cargo build \
  $release_flag \
  --no-default-features \
  --manifest-path="$pwd/modules/$name/Cargo.toml"\
  --target "wasm32-wasi" || {
    echo "Command failed"; exit 1;
  }

# Adapt the module using wasm-tools
wasm-tools component new "$pwd/modules/$name/target/wasm32-wasi/release/$name.wasm" -o "$pwd/modules/$name/target/wasm32-wasi/release/${name}_adapted.wasm" --adapt "$pwd/wasi_snapshot_preview1.wasm" || { echo "Command failed"; exit 1; }

# Embed "wit" into the component and place it in the expected location
wasm-tools component embed wit --world uq-process "$pwd/modules/$name/target/wasm32-wasi/release/${name}_adapted.wasm" -o "$pwd/modules/$name/target/wasm32-unknown-unknown/release/$name.wasm" || { echo "Command failed"; exit 1; }
