#!/usr/bin/env python3

import os
import shutil
import subprocess

def build_and_move(feature, tmp_dir):
    print("\n" + "=" * 50)
    print(f"BUILDING {feature if feature else 'default'}")
    print("=" * 50 + "\n")

    if feature:
        subprocess.run(["cargo", "+nightly", "build", "--release", "--features", feature], check=True)
        binary_name = f"uqbar-{feature}"
    else:
        subprocess.run(["cargo", "+nightly", "build", "--release"], check=True)
        binary_name = "uqbar"

    # Move and rename the binary
    source_path = "target/release/uqbar"
    dest_path = os.path.join(tmp_dir, binary_name)
    shutil.move(source_path, dest_path)

def main():
    # Features to compile with
    features = ["", "simulation-mode"]  # Add more features as needed

    # Ensure the tmp directory is clean
    tmp_dir = "/tmp/uqbar-release"
    if os.path.exists(tmp_dir):
        shutil.rmtree(tmp_dir)
    os.makedirs(tmp_dir)

    # Loop through the features and build
    for feature in features:
        build_and_move(feature, tmp_dir)

    print("Build and move process completed.\nFind release in {tmp_dir}.")

if __name__ == "__main__":
    main()

