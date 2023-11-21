#!/usr/bin/env python3

import sys
import json
import base64
import os
import shutil
import http.client
from urllib.parse import urlparse

### helpers
def send_request(path, json_data):
    conn = http.client.HTTPConnection(HOST, PORT)
    headers = {'Content-Type': 'application/json'}
    conn.request("POST", path, json_data, headers)

    response = conn.getresponse()

    conn.close()
    return response

def new_package(package_name, publisher_node, zip_file):
    request = {
        "node": NODE,
        "process": "main:app_store:uqbar",
        "inherit": False,
        "expects_response": None,
        "ipc": json.dumps({
           "NewPackage": {
                "package": {"package_name": package_name, "publisher_node": publisher_node },
                "mirror": True
            }
        }),
        "metadata": None,
        "context": None,
        "mime": "application/zip",
        "data": zip_file
    }
    return json.dumps(request)


def install_package(package_name, publisher_node):
    request = {
        "node": NODE,
        "process": "main:app_store:uqbar",
        "inherit": False,
        "expects_response": None,
        "ipc": json.dumps({
            "Install": {"package_name": package_name, "publisher_node": publisher_node },
        }),
        "metadata": None,
        "context": None,
        "mime": None,
        "data": None,
    }
    return json.dumps(request)

# zip a directory
def zip_directory(directory, zip_filename):
    shutil.make_archive(zip_filename, 'zip', directory)

# encode a file with base64
def encode_file_in_base64(file_path):
    with open(file_path, 'rb') as file:
        return base64.b64encode(file.read()).decode('utf-8')




# check if there are enough parameters provided.
if len(sys.argv) < 3 or len(sys.argv) > 4:
    print("Usage: python3 start-package.py <url> <pkg-dir> [node-id]")
    sys.exit(1)

URL = sys.argv[1]
PKG_DIR = os.path.abspath(sys.argv[2])

# If NODE is provided, use it. Otherwise, set it to None.
NODE = sys.argv[3] if len(sys.argv) == 4 else None

parsed_url = urlparse(URL)
HOST = parsed_url.hostname
PORT = parsed_url.port

# parse metadata.json to get the package and publisher
with open(f"{PKG_DIR}/metadata.json", 'r') as f:
    metadata = json.load(f)

PACKAGE = metadata['package']
PUBLISHER = metadata['publisher']
PKG_PUBLISHER = f"{PACKAGE}:{PUBLISHER}"

print(PKG_PUBLISHER)

# create zip and put it in /target
parent_dir = os.path.dirname(PKG_DIR)
parent_dir = os.path.abspath(parent_dir)
os.makedirs(os.path.join(parent_dir, 'target'), exist_ok=True)

zip_filename = os.path.join(parent_dir, 'target', PKG_PUBLISHER)
zip_directory(PKG_DIR, zip_filename)


encoded_zip_file = encode_file_in_base64(zip_filename + '.zip')


# create a new package
new_pkg = new_package(PACKAGE, PUBLISHER, encoded_zip_file)
res = send_request("/rpc:sys:uqbar/message", new_pkg)

if not res.status == 200:
    print("Failed to send new package request, status: ", res.status)
    sys.exit(1)

# install/start/reboot the package
install_pkg = install_package(PACKAGE, PUBLISHER)

resp = send_request("/rpc:sys:uqbar/message", install_pkg)
if not resp.status == 200:
    print("Failed to send install package request, status: ", resp.status)
    sys.exit(1)

print("Successfully installed package: ", PKG_PUBLISHER)
sys.exit(0)
