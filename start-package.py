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

def new_package(package, zip_file):
    request = {
        "node": NODE,
        "process": "app_tracker:app_tracker:uqbar",
        "inherit": False,
        "expects_response": None,
        "ipc": json.dumps({
           "New": {
                "package": package
            }
        }),
        "metadata": None,
        "context": None,
        "mime": "application/zip",
        "data": zip_file
    }
    return json.dumps(request)
    

def install_package(package):
    request = {
        "node": NODE,
        "process": "app_tracker:app_tracker:uqbar",
        "inherit": False,
        "expects_response": None,
        "ipc": json.dumps({
            "Install": {
                "package": package
            }
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
if len(sys.argv) != 4:
    print("Usage: <script> <url> <node-id> <pkg-dir>")
    sys.exit(1)

URL = sys.argv[1]
NODE = sys.argv[2]
PKG_DIR = sys.argv[3]

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
new_pkg = new_package(PKG_PUBLISHER, encoded_zip_file)
res = send_request("/rpc/message", new_pkg)

if not res.status == 200:
    print("Failed to send new package request, response: ", res)
    #sys.exit(1)

# install/start/reboot the package
install_pkg = install_package(PKG_PUBLISHER)

resp = send_request("/rpc/message", install_pkg)
if not resp.status == 200:
    print("Failed to send install package request, response: ", resp)
    sys.exit(1)

print("Successfully installed package: ", PKG_PUBLISHER, " response: ", resp)
sys.exit(0)

