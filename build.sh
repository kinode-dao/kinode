#!/bin/bash

all=false
debug="--release"

# parse arguments (--all, --release)
for arg in "$@"; do
    case "$arg" in
        --all)
            all=true
            ;;
        --debug)
            debug="--release"
            ;;
        *)
            echo "Error: Unrecognized argument: $arg"
            exit 1
            ;;
    esac
done

pwd=$(pwd)

# create target.wasm (compiled .wit) & world
wasm-tools component wit "${pwd}/wit/" -o target.wasm --wasm || {
    echo "Command failed"
    exit 1
}

# Run the second command and exit if it fails
touch "${pwd}/world" || {
    echo "Command failed"
    exit 1
}

# Build logic for an app
build_app() {
    dir="$1"
    release="$2"
    # Check if it contains a Cargo.toml
    if [ -f "$dir/Cargo.toml" ]; then
        ./build-app.sh "$dir" $release
    elif [ -d "$dir" ]; then
        # It's a directory. Check its subdirectories
        for sub_dir in "$dir"/*; do
            if [ -f "$sub_dir/Cargo.toml" ]; then
                ./build-app.sh "$sub_dir" $release
            fi
        done
    fi
}

# if --all compile all apps
if $all; then
    modules_dir="./modules"
    for dir in "$modules_dir"/*; do
        if [ "key_value" = "$dir" ]; then
            continue
        fi
        build_app "$dir" "$release"
    done
else
    DIRS=($(git -C . status --porcelain | grep 'modules/' | sed -n 's|^.*modules/\([^/]*\)/.*$|\1|p' | sort -u))
    for dir in "${DIRS[@]}"; do
        build_app "./modules/$dir" "$release"
    done
fi
