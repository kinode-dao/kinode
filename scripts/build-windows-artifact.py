#!/usr/bin/env python3

import argparse
import os
import subprocess
import sys
import zipfile

def is_excluded(path, excludes, include_files):
    path = os.path.abspath(path)
    # If the path is in include_files, do not exclude it
    if path in include_files:
        return False
    for exclude in excludes:
        if os.path.commonpath([path, exclude]) == exclude:
            return True
    return False

def parse_args(repo_root):
    parser = argparse.ArgumentParser(description='Build Windows artifact.')
    parser.add_argument(
        '--exclude',
        action='append',
        default=[],
        help='Exclude directories (relative to repo root). Can be used multiple times.'
    )
    parser.add_argument(
        '--output',
        default=os.path.join(repo_root, 'target', 'windows-artifact.zip'),
        help='Output zip file path.'
    )
    args = parser.parse_args()
    return args

def main():
    # Get the directory where the script is located
    script_dir = os.path.dirname(os.path.abspath(__file__))
    # Assume the repo root is one level up from the script directory
    repo_root = os.path.abspath(os.path.join(script_dir, '..'))

    args = parse_args(repo_root)

    default_excludes = [
        '.git',
        'hyperdrive/packages',
        'target',
        'hyperdrive/src/register-ui/node_modules',
    ]
    excludes = default_excludes + args.exclude

    # Convert exclude paths to absolute paths
    excludes = [os.path.abspath(os.path.join(repo_root, p)) for p in excludes]

    # Include 'target/packages.zip' even though 'target' is excluded
    include_files = [
        os.path.abspath(os.path.join(repo_root, 'target', 'packages.zip'))
    ]

    # Run the build scripts
    build_script_dir = os.path.join(repo_root, 'hyperdrive', 'src', 'register-ui')
    build_script_name = 'build.sh'
    build_script = os.path.join(build_script_dir, build_script_name)
    if not os.path.exists(build_script):
        print(f'Build script not found at {build_script}')
        sys.exit(1)

    # Execute the build script
    subprocess.check_call([f'./{build_script_name}'], cwd=build_script_dir)

    # Run cargo build
    subprocess.check_call(['cargo', 'build', '-p', 'build-packages'], cwd=repo_root)

    # Create the zip file
    output_zip = args.output
    output_zip_abs = os.path.abspath(output_zip)
    output_dir = os.path.dirname(output_zip_abs)
    if output_dir and not os.path.exists(output_dir):
        os.makedirs(output_dir)

    # Exclude the output zip file itself
    excludes.append(output_zip_abs)

    with zipfile.ZipFile(output_zip_abs, 'w', zipfile.ZIP_DEFLATED) as zipf:
        for root, dirs, files in os.walk(repo_root):
            for file in files:
                file_path = os.path.join(root, file)
                if is_excluded(file_path, excludes, include_files):
                    continue
                rel_path = os.path.relpath(file_path, repo_root)
                if ':' in str(rel_path):
                    # Replace ':' in filenames to make them valid on Windows
                    rel_path = rel_path.replace(':', '_')
                    print(f'Unexpected `:` in filename: {rel_path}; replacing with `_` in zip file')
                zipf.write(file_path, rel_path)

if __name__ == '__main__':
    main()
