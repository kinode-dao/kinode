<p align="center">
    <img width="551" alt="Screenshot 2024-05-08 at 2 38 11 PM" src="https://github.com/kinode-dao/kinode/assets/93405247/24c7982b-9d76-419a-96dc-ec4a25dda562">
    <br />
    <img src="https://img.shields.io/twitter/follow/kinodeOS">

</p>

Kinode is a general-purpose sovereign cloud computer, built for crypto.

This repo contains the core runtime and processes.
Most developers need not build the runtime.
Instead, check out the [Kinode book](https://book.kinode.org/), and in particular the ["My First App" tutorial](https://book.kinode.org/my_first_app/chapter_1.html).

If you want to get on the network, you can download a binary, rather than building it yourself, from [the releases page](https://github.com/kinode-dao/kinode/tags).
Then follow the instructions to [install it](https://book.kinode.org/install.html) and [join the network](https://book.kinode.org/login.html).

If you have questions, join the [Kinode discord](https://discord.gg/TCgdca5Bjt) and drop us a line in `#dev-support`.

## Setup

On certain operating systems, you may need to install these dependencies if they are not already present:

- openssl-sys: https://docs.rs/crate/openssl-sys/0.9.19
- libclang 5.0: https://rust-lang.github.io/rust-bindgen/requirements.html

```bash
# Clone the repo.

git clone git@github.com:kinode-dao/kinode.git

# Get some stuff so we can build Wasm.

cd kinode
cargo install wasm-tools
rustup install nightly
rustup target add wasm32-wasi
rustup target add wasm32-wasi --toolchain nightly
rustup target add wasm32-wasip1
rustup target add wasm32-wasip1 --toolchain nightly
cargo install cargo-wasi

# Install NPM so we can build frontends for "distro" packages.
# https://docs.npmjs.com/downloading-and-installing-node-js-and-npm
# If you want to skip this step, run cargo build with the environment variable SKIP_BUILD_FRONTEND=true

# Build the runtime, along with a number of "distro" Wasm modules.
# The compiled binary will be at `kinode/target/debug/kinode`
# OPTIONAL: --release flag (slower build; faster runtime; binary at `kinode/target/release/kinode`)

cargo +nightly build -p kinode
```

## Security Status

No security audits of this crate have ever been performed. This software is under active development and should be **used at your own risk**.

## Boot

Make sure not to use the same home directory for two nodes at once! You can use any name for the home directory: here we just use `home`. The `--` here separates cargo arguments from binary arguments.

TODO: document feature flags in `--simulation-mode`

```bash
# OPTIONAL: --release flag
cargo +nightly run -p kinode -- home
```

On boot you will be prompted to navigate to `localhost:8080` (or whatever HTTP port your node bound to: it will try 8080 and go up from there, or use the port passed with the `--http-port` boot flag. Make sure your browser wallet matches the network that the node is being booted on. Follow the registration UI -- if you want to register a new ID you will either need Optimism ETH or an invite code.

## Configuring the ETH RPC Provider

By default, a node will use the [hardcoded providers](./kinode/src/eth/default_providers_mainnet.json) for the network it is booted on. A node can use a WebSockets RPC URL directly, or use another Kinode as a relay point. To adjust the providers a node uses, just create and modify the `.eth_providers` file in the node's home folder (set at boot). See the Kinode Book for more docs, and see the [default providers file here](./kinode/src/eth/default_providers_mainnet.json) for a template to create `.eth_providers`.

You may also add a RPC provider or otherwise modify your configuration by sending messages from the terminal to the `eth:distro:sys` process. You can get one for free at `alchemy.com`. Use this message format to add a provider -- this will make your node's performance better when accessing a blockchain:

```
m our@eth:distro:sys '{"AddProvider": {"chain_id": <SOME_CHAIN_ID>, "trusted": true, "provider": {"RpcUrl": "<WS_RPC_URL>"}}}'
```

You can also do the same thing by using the `--rpc` boot flag with an Optimism WebSockets RPC URL, or going to the Settings app once booted into a node.

## Distro and Runtime processes

The base OS install comes with certain runtime modules. These are interacted with in the same way as userspace processes, but are deeply ingrained to the system and the APIs they present at their Process IDs are assumed to be available by userspace processes. All of these are identified in the `distro:sys` package.

This distribution of the OS also comes with userspace packages pre-installed. Some of these packages are intimately tied to the runtime: `terminal`, `homepage`, and `kns_indexer`. Modifying, removing or replacing the distro userspace packages should only be done in highly specialized use-cases.

The runtime distro processes are:

- `eth:distro:sys`
- `http_client:distro:sys`
- `http_server:distro:sys`
- `kernel:distro:sys`
- `kv:distro:sys`
- `net:distro:sys`
- `state:distro:sys`
- `terminal:distro:sys`
- `timer:distro:sys`
- `sqlite:distro:sys`
- `vfs:distro:sys`

The distro userspace packages are:

- `app_store:sys`
- `chess:sys`
- `homepage:sys`
- `kino_updates:sys`
- `kns_indexer:sys`
- `settings:sys`
- `terminal:sys`
- `tester:sys` (used with `kit` for running test suites, only installed in `simulation-mode`)

The `sys` publisher is not a real node ID, but it's also not a special case value. Packages, whether runtime or userspace, installed from disk when a node bootstraps do not have their package ID or publisher node ID validated. Packages installed (not injected locally, as is done during development) after a node has booted will have their publisher field validated.

## Terminal syntax

- CTRL+C or CTRL+D to gracefully shutdown node
- CTRL+V to toggle through verbose modes (0-3, 0 is default and lowest verbosity)

- CTRL+J to toggle debug mode
- CTRL+S to step through events in debug mode

- CTRL+L to toggle logging mode, which writes all terminal output to the `.terminal_log` file. Off by default, this will write all events and verbose prints with timestamps.

- CTRL+A to jump to beginning of input
- CTRL+E to jump to end of input
- UpArrow/DownArrow or CTRL+P/CTRL+N to move up and down through command history
- CTRL+R to search history, CTRL+R again to toggle through search results, CTRL+G to cancel search

### Built-in terminal scripts

The terminal package contains a number of built-in scripts.
Users may also call scripts from other packages in the terminal by entering the (full) ID of the script process followed by any arguments.
In order to call a script with shorthand, a user may apply an *alias* using the terminal `alias` script, like so:
```
alias <shorthand> <full_name>
```
Subsequent use of the shorthand will then be interpolated as the process ID.

A list of the terminal scripts included in this distro:

- `alias <shorthand> <process_id>`: create an alias for a script.
    - Example: `alias get_block get_block:kns_indexer:sys`
    - note: all of these listed commands are just default aliases for terminal scripts.
- `cat <vfs-file-path>`: print the contents of a file in the terminal.
    - Example: `cat /terminal:sys/pkg/scripts.json`
- `echo <text>`: print text to the terminal.
    - Example: `echo foo`
- `help <command>`: print the help message for a command. Leave the command blank to print the help message for all commands.
- `hi <name> <string>`: send a text message to another node's command line.
    - Example: `hi mothu.kino hello world`
- `kfetch`: print system information a la neofetch. No arguments.
- `kill <process-id>`: terminate a running process. This will bypass any restart behavior–use judiciously.
    - Example: `kill chess:chess:sys`
- `m <address> '<json>'`: send an inter-process message. <address> is formatted as <node>@<process_id>. <process_id> is formatted as <process_name>:<package_name>:<publisher_node>. JSON containing spaces must be wrapped in single-quotes (`''`).
    - Example: `m our@eth:distro:sys "SetPublic" -a 5`
    - the '-a' flag is used to expect a response with a given timeout
    - `our` will always be interpolated by the system as your node's name
- `net_diagnostics`: print some useful networking diagnostic data.
- `peer <name>`: print the peer's PKI info, if it exists.
- `peers`: print the peers the node currently hold connections with.
- `top <process_id>`: display kernel debugging info about a process. Leave the process ID blank to display info about all processes and get the total number of running processes.
    - Example: `top net:distro:sys`
    - Example: `top`

## Running as a Docker container

This image expects a volume mounted at `/kinode-home`. This volume may be empty or may contain another Kinode's data. It will be used as the home directory of your Kinode.

The image includes EXPOSE directives for TCP port `8080` and TCP port `9000`. Port `8080` is used for serving the Kinode web dashboard over HTTP, and it may be mapped to a different port on the host. Port `9000` is optional and is only required for a direct node.

If you are running a direct node, you must map port `9000` to the same port on the host and on your router. Otherwise, your Kinode will not be able to connect to the rest of the network as connection info is written to the chain, and this information is based on the view from inside the Docker container.

To build a local Docker image, run the following command in this project root.

```bash
# The `VERSION` may be replaced with the tag of a GitHub release

# Build for your system's architecture
docker build . -t 0xlynett/kinode --build-arg VERSION=v0.9.1

# Build a multiarch image
docker buildx build . --platform arm64,amd64 --build-arg VERSION=v0.9.1 -t 0xlynett/kinode
```

For example:

```bash
docker volume create kinode-volume

docker run -d -p 8080:8080 -it --name my-kinode \
    --mount type=volume,source=kinode-volume,destination=/kinode-home \
    0xlynett/kinode
```
