#!/bin/bash

# Check if there are enough parameters provided.
if [ "$#" -ne 4 ]; then
    echo "Usage: $0 <url> <node-id> <vfs-identifier> <destination_process>"
    exit 1
fi

URL="$1"
NODE="$2"
IDENTIFIER="$3"
DEST_PROC="$4"

# Generate new vfs capabilities and give "read" to the destination process
curl "$URL/rpc/message" -H 'content-type: application/json' -d "{\"node\": \"$NODE\", \"process\": \"vfs\", \"inherit\": false, \"expects_response\": false, \"ipc\": \"{\\\"New\\\": {\\\"identifier\\\": \\\"$IDENTIFIER\\\"}}\", \"metadata\": null, \"context\": null, \"mime\": null, \"data\": null}"
curl "$URL/rpc/capabilities/transfer" -H 'content-type: application/json' -d "{\"destination_node\": \"$NODE\", \"destination_process\": \"$DEST_PROC\", \"node\": \"$NODE\", \"process\": \"vfs\", \"params\": \"{\\\"identifier\\\": \\\"$IDENTIFIER\\\",\\\"kind\\\":\\\"read\\\"}\"}"
