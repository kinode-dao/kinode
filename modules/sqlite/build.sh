#!/bin/bash

RUSTFLAGS="-C target-feature=-crt-static -C link-arg=-Wl,--no-entry,--export=init,--export=cabi_realloc" cargo build --release --no-default-features --target wasm32-wasi
