Bootstrapping the kernel

1. We have a bunch of packages that are built and zipped into "packages"

2. On startup, if we don't yet have a filesystem, we grab these packages by name from unix

3. For each package, "unzip" it and read the "manifest" file

4. For each entry in the manifest, start the named process by sending a message to kernel.


```
package.zip
    key_value.wasm
    key_value_worker.wasm
    index.html
    my_directory
        cool_image.png
    .manifest
```

inside .manifest:
(describes processes to start on-install)
```
    [
        {
            "process_id": "key_value",
            "process_wasm": "key_value.wasm",
            "on_panic": {"on_panic": true},
            "networking": true,
            "messaging": ["vfs", "http_server", "http_bindings"]
        }
    ]
```