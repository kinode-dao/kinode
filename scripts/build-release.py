#!/usr/bin/env python3

import os
import shutil
import subprocess
import zipfile

def get_system_info():
    # Get OS and architecture information
    os_info = subprocess.run(["uname"], capture_output=True, text=True, check=True).stdout.strip().lower()
    arch_info = subprocess.run(["uname", "-p"], capture_output=True, text=True, check=True).stdout.strip().lower()

    if os_info == "linux":
        os_info = "unknown-linux-gnu"
    elif os_info == "darwin":
        os_info = "apple-darwin"

    if arch_info == "arm":
        arch_info = "aarch64"

    return arch_info, os_info

def build_and_move(feature, tmp_dir, architecture, os_name):
    print("\n" + "=" * 50)
    print(f"BUILDING {feature if feature else 'default'}")
    print("=" * 50 + "\n")

    zip_prefix = f"kinode-{architecture}-{os_name}"
    release_env = os.environ.copy()
    release_env["CARGO_PROFILE_RELEASE_LTO"] = f"fat"
    release_env["CARGO_PROFILE_RELEASE_CODEGEN_UNITS"] = f"1"
    release_env["CARGO_PROFILE_RELEASE_STRIP"] = f"symbols"
    if feature:
        subprocess.run(["cargo", "+nightly", "build", "--release", "--features", feature], check=True, env=release_env)
        zip_name = f"{zip_prefix}-{feature}.zip"
    else:
        subprocess.run(["cargo", "+nightly", "build", "--release"], check=True, env=release_env)
        zip_name = f"{zip_prefix}.zip"

    # Move and rename the binary
    binary_name = "kinode"
    source_path = f"target/release/{binary_name}"
    dest_path = os.path.join(tmp_dir, binary_name)
    shutil.move(source_path, dest_path)

    # Create a zip archive of the binary
    zip_path = os.path.join(tmp_dir, zip_name)
    with zipfile.ZipFile(zip_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
        zipf.write(dest_path, os.path.basename(dest_path))

    # Remove the original binary
    os.remove(dest_path)

def main():
    # Get system info
    architecture, os_name = get_system_info()

    # Modify the temporary directory path
    tmp_dir = "/tmp/kinode-release"
    if os.path.exists(tmp_dir):
        shutil.rmtree(tmp_dir)
    os.makedirs(tmp_dir)

    # Features to compile with; add more features as needed
    features = ["", "simulation-mode"]

    # Loop through the features and build
    for feature in features:
        build_and_move(feature, tmp_dir, architecture, os_name)

    print(f"Build and move process completed.\nFind release in {tmp_dir}.")

if __name__ == "__main__":
    main()

