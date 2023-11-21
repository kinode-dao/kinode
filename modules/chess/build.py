import os
import subprocess
import sys
import json
import glob
import shutil

def compile_process(process_dir, pkg_dir, root_dir):
    # Get the path to the source code and the compiled WASM file
    src_path = os.path.join(process_dir, "src")
    wasm_path = os.path.join(pkg_dir, os.path.basename(process_dir) + ".wasm")

    # Check if the source code or the Cargo.toml file has been modified since the last compile
    src_mtime = max(os.path.getmtime(f) for f in glob.glob(os.path.join(src_path, '**'), recursive=True))
    wasm_mtime = os.path.getmtime(wasm_path) if os.path.exists(wasm_path) else 0

    # Change to the process directory
    os.chdir(process_dir)

    # Create the target/bindings/$name/ directory
    bindings_dir = os.path.join(process_dir, "target", "bindings", os.path.basename(process_dir))
    os.makedirs(bindings_dir, exist_ok=True)

    # Create target.wasm (compiled .wit) & world
    subprocess.check_call([
        "wasm-tools", "component", "wit",
        os.path.join(root_dir, "wit"),
        "-o", os.path.join(bindings_dir, "target.wasm"),
        "--wasm"
    ])

    # Copy /wit (world is empty file currently)
    shutil.copytree(os.path.join(root_dir, "wit"), os.path.join(bindings_dir, "wit"), dirs_exist_ok=True)
    # shutil.copy(os.path.join(root_dir, "world"), os.path.join(bindings_dir, "world"))

    # Create an empty world file
    with open(os.path.join(bindings_dir, "world"), 'w') as f:
        pass

    # Build the module using Cargo
    subprocess.check_call([
        "cargo", "+nightly", "build",
        "--release",
        "--no-default-features",
        "--target", "wasm32-wasi"
    ])

    # Adapt the module using wasm-tools
    wasm_file = os.path.join(process_dir, "target", "wasm32-wasi", "release", os.path.basename(process_dir) + ".wasm")
    adapted_wasm_file = wasm_file.replace(".wasm", "_adapted.wasm")
    subprocess.check_call([
        "wasm-tools", "component", "new",
        wasm_file,
        "-o", adapted_wasm_file,
        "--adapt", os.path.join(root_dir, "wasi_snapshot_preview1.wasm")
    ])

    # Embed "wit" into the component and place it in the expected location
    subprocess.check_call([
        "wasm-tools", "component", "embed", os.path.join(root_dir, "wit"),
        "--world", "process",
        adapted_wasm_file,
        "-o", wasm_path
    ])

if __name__ == "__main__":
    root_dir = os.getcwd()
    pkg_dir = os.path.join(root_dir, "pkg")

    # If a specific process is provided, compile it
    if len(sys.argv) > 1:
        process_dir = os.path.abspath(os.path.join(root_dir, sys.argv[1]))
        compile_process(process_dir, pkg_dir, root_dir)
    else:
        # Compile each base dir folder that has a Cargo.toml
        for root, dirs, files in os.walk(root_dir):
            if 'Cargo.toml' in files and "process_lib" not in root:
                process_dir = os.path.abspath(root)
                compile_process(process_dir, pkg_dir, root_dir)
