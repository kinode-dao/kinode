#!/bin/bash

release_flag=""

# Grab the full path to the target
target_path="$1"
name=$(basename "$target_path")

if [[ "$2" == "--release" ]]; then
    release_flag="--release"
fi

pwd=$(pwd)

rm -rf "$target_path/wit" || { echo "Command failed"; exit 1; }
cp -r wit "$target_path" || { echo "Command failed"; exit 1; }
mkdir -p "$target_path/target/bindings/$name" || { echo "Command failed"; exit 1; }

cp target.wasm "$target_path/target/bindings/$name/" || { echo "Command failed"; exit 1; }
cp world "$target_path/target/bindings/$name/" || { echo "Command failed"; exit 1; }

mkdir -p "$target_path/target/wasm32-unknown-unknown/release" || { echo "Command failed"; exit 1; }

# Build the module using Cargo
cargo +nightly build \
  $release_flag \
  --no-default-features \
  --manifest-path="$target_path/Cargo.toml" \
  --target "wasm32-wasi" || {
    echo "Command failed"; exit 1;
  }

# Adapt the module using wasm-tools
wasm-tools component new "$target_path/target/wasm32-wasi/release/$name.wasm" -o "$target_path/target/wasm32-wasi/release/${name}_adapted.wasm" --adapt "$pwd/wasi_snapshot_preview1.wasm" || { echo "Command failed"; exit 1; }

# Embed "wit" into the component and place it in the expected location
wasm-tools component embed wit --world uq-process "$target_path/target/wasm32-wasi/release/${name}_adapted.wasm" -o "$target_path/target/wasm32-unknown-unknown/release/$name.wasm" || { echo "Command failed"; exit 1; }
