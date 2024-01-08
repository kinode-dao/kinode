#!/bin/bash

# Check if there are enough parameters provided.
if [ "$#" -ne 5 ]; then
    echo "Usage: $0 <url> <node-id> <vfs-identifier> <index.html> <assets directory>"
    exit 1
fi

URL="$1"
NODE="$2"
IDENTIFIER="$3"
INDEX_HTML="$4"
DIRECTORY="$5"

# Upload index.html
curl "$URL/rpc/message" -H 'content-type: application/json' -d "{\"node\": \"$NODE\", \"process\": \"vfs\", \"inherit\": false, \"expects_response\": false, \"ipc\": \"{\\\"Add\\\": {\\\"identifier\\\": \\\"$IDENTIFIER\\\", \\\"full_path\\\": \\\"/index.html\\\", \\\"entry_type\\\": {\\\"NewFile\\\": null}}}\", \"metadata\": null, \"context\": null, \"mime\": null, \"data\": \"$(base64 < $INDEX_HTML)\"}"

[[ "$DIRECTORY" != */ ]] && DIRECTORY="$DIRECTORY/"

# Iterate over files in the specified directory
for FILE in "$DIRECTORY"*; do
    # Check if it's a regular file before proceeding
    if [[ -f "$FILE" ]]; then
        # Extract just the file name
        FILE_NAME=$(basename "$FILE")

        # upload file to Uqbar VFS
        curl "$URL/rpc/message" -H 'content-type: application/json' -d "{\"node\": \"$NODE\", \"process\": \"vfs\", \"inherit\": false, \"expects_response\": false, \"ipc\": \"{\\\"Add\\\": {\\\"identifier\\\": \\\"$IDENTIFIER\\\", \\\"full_path\\\": \\\"/$FILE_NAME\\\", \\\"entry_type\\\": {\\\"NewFile\\\": null}}}\", \"metadata\": null, \"context\": null, \"mime\": null, \"data\": \"$(base64 < "$FILE")\"}"
    fi
done
