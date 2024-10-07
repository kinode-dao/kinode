#!/usr/bin/env python3

import argparse
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import zipfile

def get_features(args):
    # Join the features into a comma-separated string
    features = ','.join(args.features)
    return features

def zip_directory(directory_path):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, 'w', zipfile.ZIP_DEFLATED) as zip_file:
        for root, dirs, files in os.walk(directory_path):
            # Adding directories explicitly to ensure they are included in the zip
            for dir in dirs:
                dir_path = os.path.join(root, dir)
                arcname = os.path.relpath(dir_path, start=directory_path)
                # Create a ZipInfo object for the directory
                zi = zipfile.ZipInfo(arcname + '/')
                permissions = 0o755
                zi.external_attr = permissions << 16 | 0x10  # MS-DOS directory flag
                zi.date_time = (2023, 6, 19, 0, 0, 0)
                zip_file.writestr(zi, '')
            for file in files:
                file_path = os.path.join(root, file)
                arcname = os.path.relpath(file_path, start=directory_path)
                # Get file info
                st = os.stat(file_path)
                # Create ZipInfo object
                zi = zipfile.ZipInfo(arcname)
                # Set permissions
                permissions = st.st_mode
                zi.external_attr = permissions << 16
                # Set date_time
                zi.date_time = (2023, 6, 19, 0, 0, 0)
                # Read file data
                with open(file_path, 'rb') as f:
                    file_data = f.read()
                zip_file.writestr(zi, file_data)
    zip_contents = buffer.getvalue()
    return zip_contents

def build_and_zip_package(entry_path, parent_pkg_path, features):
    # Build the package
    build_cmd = ['kit', 'build', entry_path, '--no-ui', '--skip-deps-check']
    if features:
        build_cmd += ['--features', features]
    result = subprocess.run(build_cmd, cwd=entry_path)
    if result.returncode != 0:
        raise Exception(f'Failed to build package at {entry_path}')

    # Now zip up the parent_pkg_path directory
    zip_filename = f'{os.path.basename(entry_path)}.zip'
    zip_contents = zip_directory(parent_pkg_path)

    return (str(entry_path), zip_filename, zip_contents)

def main():
    parser = argparse.ArgumentParser(description='Build and zip Rust packages.')
    parser.add_argument('--features', nargs='*', default=[], help='List of features to compile packages with')
    parser.add_argument('--skip-build-frontend', action='store_true', help='Skip building the frontend')
    args = parser.parse_args()

    script_path = Path(os.path.abspath(__file__))
    top_level_dir = script_path.parent.parent
    kinode_dir = top_level_dir / 'kinode'
    packages_dir = kinode_dir / 'packages'

    if args.skip_build_frontend:
        print("skipping frontend builds")
    else:
        # Build core frontends
        core_frontends = [
            'src/register-ui',
            'packages/app_store/ui',
            'packages/homepage/ui',
            # chess when brought in
        ]

        # For each frontend, execute build.sh
        for frontend in core_frontends:
            frontend_path = kinode_dir / frontend
            build_script = frontend_path / 'build.sh'
            if not build_script.exists():
                print(f'Build script not found for frontend: {frontend} at {build_script}')
                continue
            result = subprocess.run(['sh', './build.sh'], cwd=frontend_path)
            if result.returncode != 0:
                raise Exception(f'Failed to build frontend: {frontend}')

    features = get_features(args)

    results = []
    for entry in os.scandir(packages_dir):
        if not entry.is_dir():
            continue
        entry_path = Path(entry.path)
        child_pkg_path = entry_path / 'pkg'
        if not child_pkg_path.exists():
            continue
        result = build_and_zip_package(str(entry_path), str(child_pkg_path), features)
        results.append(result)

    # Process results
    bootstrapped_processes = []
    bootstrapped_processes.append('pub static BOOTSTRAPPED_PROCESSES: &[(&str, &[u8], &[u8])] = &[')

    target_dir = top_level_dir / 'target'
    target_packages_dir = target_dir / 'packages'
    if not target_packages_dir.exists():
        os.makedirs(target_packages_dir)

    for (entry_path, zip_filename, zip_contents) in results:
        # Save zip_contents to zip_path
        zip_path = target_packages_dir / zip_filename
        with open(zip_path, 'wb') as f:
            f.write(zip_contents)

        metadata_path = os.path.join(entry_path, 'metadata.json')

        # Update bootstrapped_processes
        bootstrapped_processes.append(
            f'    ("{zip_filename}", include_bytes!("{metadata_path}"), include_bytes!("{zip_path}")),'
        )

    bootstrapped_processes.append('];')

    bootstrapped_processes_path = target_packages_dir / 'bootstrapped_processes.rs'
    with open(bootstrapped_processes_path, 'w') as f:
        f.write('\n'.join(bootstrapped_processes))

    zip_contents = zip_directory(target_packages_dir)
    zip_path = target_dir / 'packages.zip'

    with open(zip_path, 'wb') as f:
        f.write(zip_contents)

if __name__ == '__main__':
    main()
