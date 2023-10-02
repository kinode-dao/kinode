#!/bin/bash

all=false
release=""

# prase arguments (--all, --release)
for arg in "$@"; do
    case "$arg" in
        --all)
            all=true
            ;;
        --release)
            release="--release"
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

# if --all compile all apps
if $all; then
    modules_dir="./modules"
    for dir in "$modules_dir"/*; do
        # Check if it's a directory
        if [ -d "$dir" ]; then
            dir_name=$(basename "$dir")
            ./build-app.sh "$dir_name" $release
        fi
    done
# else just compile the ones that have git changes
# NOTE: this can screw you up if you
#   1. make a change
#   2. compile it with ./build.sh
#   3. revert those changes
# this script will not recompile it after that because it uses git to detect changes
# so every once in a while just run --all to make sure everything is in line
else
    DIRS=($(git -C . status --porcelain | grep 'modules/' | sed -n 's|^.*modules/\([^/]*\)/.*$|\1|p' | sort -u))
    for dir in "${DIRS[@]}"; do
        ./build-app.sh $dir $release
    done
fi
