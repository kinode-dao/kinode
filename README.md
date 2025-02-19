<p align="center">
    <img width="551" alt="Screenshot 2024-05-08 at 2 38 11 PM" src="https://github.com/hyperware-ai/hyperdrive/assets/93405247/24c7982b-9d76-419a-96dc-ec4a25dda562">
    <br />
    <img src="https://img.shields.io/twitter/follow/Kinode">

</p>

Hyperware is a general-purpose sovereign cloud computer, built for crypto.

Hyperdrive is the runtime for Hyperware.

This repo contains the core runtime and processes.
Most developers need not build the runtime.
Instead, check out the [Hyperware Book](https://book.hyperware.ai/), and in particular the ["My First App" tutorial](https://book.hyperware.ai/my_first_app/chapter_1.html).

If you want to get on the network, you can download a binary, rather than building it yourself, from [the releases page](https://github.com/hyperware-ai/hyperware/tags).
Then follow the instructions to [install it](https://book.hyperware.ai/install.html) and [join the network](https://book.hyperware.ai/login.html).

If you have questions, join the [Hyperware discord](https://discord.gg/TCgdca5Bjt) and drop us a line in `#dev-support`.

## Setup

On certain operating systems, you may need to install these dependencies if they are not already present:

- openssl-sys: https://docs.rs/crate/openssl-sys/0.9.19
- libclang 5.0: https://rust-lang.github.io/rust-bindgen/requirements.html

```bash
# Clone the repo.

git clone git@github.com:hyperware-ai/hyperware.git

# Install Rust and some `cargo` tools so we can build the runtime and Wasm.

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install wasm-tools
rustup install nightly
rustup target add wasm32-wasip1 --toolchain nightly
cargo install cargo-wasi

# Install NPM so we can build frontends for "distro" packages.
# https://docs.npmjs.com/downloading-and-installing-node-js-and-npm
# If you want to skip this step, build the packages with `cargo run -p build-packages -- --skip-build-frontend` to neglect building the frontends

# Build the "distro" Wasm modules, then, build the runtime.
# The compiled packages will be at `hyperdrive/target/packages.zip`.
# The compiled binary will be at `hyperdrive/target/debug/hyperdrive`.
# OPTIONAL: --release flag (slower build; faster runtime; binary at `hyperdrive/target/release/hyperdrive`).

cd hyperdrive
cargo run -p build-packages
# OPTIONAL: --release flag
cargo build -p hyperdrive
```

[To build on Windows](https://gist.github.com/nick1udwig/f2d39a3fc6ccc7f7ad2912e8d3aeaae0)

## Security Status

This software is under active development and should be **used at your own risk**.

A security audit targeting the networking protocol, web interface, and kernel architecture was performed by [Enigma Dark](https://www.enigmadark.com/).
That report can be found [here](https://github.com/Enigma-Dark/security-review-reports/blob/main/2024-11-18_Architecture_Review_Report_Kinode.pdf).

## Boot

Make sure not to use the same home directory for two nodes at once! You can use any name for the home directory: here we just use `home`. The `--` here separates cargo arguments from binary arguments.

```bash
cargo run -p hyperdrive -- home
```

On boot you will be prompted to navigate to `localhost:8080` or whatever HTTP port your node bound to: it will try 8080 and go up from there, or use the port passed with the `--port` boot flag. Make sure your browser wallet matches the network that the node is being booted on. Follow the registration UI -- if you want to register a new ID you will either need Optimism ETH or an invite code.

#### Boot Flags

Here are all the available boot flags for the Hyperdrive runtime:

- `[home]`: (Required) Path to home directory.
- `-p, --port <PORT>`: Port to bind for HTTP. Default is the first unbound port at or above 8080.
- `--ws-port <PORT>`: Hyperdrive internal WebSockets protocol port. Default is the first unbound port at or above 9000.
- `--tcp-port <PORT>`: Hyperdrive internal TCP protocol port. Default is the first unbound port at or above 10000.
- `-v, --verbosity <VERBOSITY>`: Verbosity level: higher (up to 3)is more verbose. Default is 0.
- `-l, --logging-off`: Run in non-logging mode. Do not write terminal output to file in .terminal_logs directory.
- `-d, --detached`: Run in detached mode (don't accept input on terminal).
- `--rpc <RPC>`: Add a WebSockets Optimism RPC URL at boot.
- `--rpc-config <RPC_CONFIG_PATH>`: Add WebSockets RPC URLs specified in config at boot
- `--password <PASSWORD>`: Node password (in double quotes).
- `--max-log-size <MAX_LOG_SIZE_BYTES>`: Max size of all terminal logs in bytes. Setting to 0 means no size limit. Default is 16MB.
- `--number-log-files <NUMBER_LOG_FILES>`: Number of terminal logs to rotate. Default is 4.
- `--max-peers <MAX_PEERS>`: Maximum number of peers to hold active connections with. Default is 32.
- `--max-passthroughs <MAX_PASSTHROUGHS>`: Maximum number of passthroughs to serve as a router. Default is 0.
- `--soft-ulimit <SOFT_ULIMIT>`: Enforce a static maximum number of file descriptors. Default is fetched from system.

When compiled with the `simulation-mode` feature, two additional flags are available:

- `--fake-node-name <NAME>`: Name of fake node to boot.
- `--fakechain-port <FAKECHAIN_PORT>`: Port to bind to for local anvil-run blockchain.

`RPC_CONFIG_PATH` must point to a file containing a JSON array of JSON objects with required key of `"url"` (whose value must be a string) and optional field of `"auth"`.
`"auth"`, if included, must be a JSON object with one key, either `"Basic"`, `"Bearer"`, or `"Raw"`, and whose value must be a string.
E.g.:

```json
[
    {
        "url": "wss://path.to.my.rpc.com",
        "auth": {
            "Bearer": "this.is.my.bearer.key"
        }
    }
]
```

This allows authorization headers to be set for RPC providers.

## Configuring the ETH RPC Provider

By default, a node will use the [hardcoded providers](./hyperdrive/src/eth/default_providers_mainnet.json) for the network it is booted on.
A node can use a WebSockets RPC URL directly, or use another node as a relay point.
To adjust the providers a node uses, just create and modify the `.eth_providers` file in the node's home folder (set at boot).
See the Hyperdrive Book for more docs, and see the [default providers file here](./hyperdrive/src/eth/default_providers_mainnet.json) for a template to create `.eth_providers`.

You may also add a RPC provider or otherwise modify your configuration by sending messages from the terminal to the `eth:distro:sys` process.
You can get one for free at `alchemy.com`.
Use this message format to add a provider -- this will make your node's performance better when accessing a blockchain:

```
m our@eth:distro:sys '{"AddProvider": {"chain_id": <SOME_CHAIN_ID>, "trusted": true, "provider": {"RpcUrl": "<WS_RPC_URL>"}}}'
```

You can also do the same thing by using the `--rpc` boot flag with an Optimism WebSockets RPC URL, or going to the Settings app once booted into a node.

## Distro and Runtime processes

Hyperdrive comes with certain runtime modules.
These are interacted with in the same way as userspace processes, but are deeply ingrained to the system and the APIs they present at their Process IDs are assumed to be available by userspace processes.
All of these are identified in the `distro:sys` package.

Hyperdrive also comes with userspace packages pre-installed.
Some of these packages are intimately tied to the runtime: `terminal`, `homepage`, and `hns-indexer`.
Modifying, removing or replacing the distro userspace packages should only be done in highly specialized use-cases.

The runtime distro processes are:

- `eth:distro:sys`
- `fd-manager:distro:sys`
- `http-client:distro:sys`
- `http-server:distro:sys`
- `kernel:distro:sys`
- `kv:distro:sys`
- `net:distro:sys`
- `state:distro:sys`
- `terminal:distro:sys`
- `timer:distro:sys`
- `sqlite:distro:sys`
- `vfs:distro:sys`

The distro userspace packages are:

- `app-store:sys`
- `chess:sys`
- `contacts:sys`
- `homepage:sys`
- `hns-indexer:sys`
- `settings:sys`
- `terminal:sys`
- `tester:sys` (used with `kit` for running test suites, only installed in `simulation-mode`)

The `sys` publisher is not a real node ID, but it's also not a special case value.
Packages, whether runtime or userspace, installed from disk when a node bootstraps do not have their package ID or publisher node ID validated.
Packages installed (not injected locally, as is done during development) after a node has booted will have their publisher field validated.

## Terminal syntax

- CTRL+C or CTRL+D to gracefully shutdown node
- CTRL+V to toggle through verbose modes (0-3, 0 is default and lowest verbosity)

- CTRL+J to toggle debug mode
- CTRL+S to step through events in debug mode

- CTRL+L to toggle logging mode, which writes all terminal output to the `.terminal_log` file.
  On by default, this will write all events and verbose prints with timestamps.

- CTRL+A to jump to beginning of input
- CTRL+E to jump to end of input
- UpArrow/DownArrow or CTRL+P/CTRL+N to move up and down through command history
- CTRL+R to search history, CTRL+R again to toggle through search results, CTRL+G to cancel search

- CTRL+W to set process-level verbosities that override the verbosity mode set with CTRL+V (0-3, 0 is default and lowest verbosity)

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
    - Example: `alias get_block get-block:hns-indexer:sys`
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
- `net-diagnostics`: print some useful networking diagnostic data.
- `peer <name>`: print the peer's PKI info, if it exists.
- `peers`: print the peers the node currently hold connections with.
- `top <process_id>`: display kernel debugging info about a process. Leave the process ID blank to display info about all processes and get the total number of running processes.
    - Example: `top net:distro:sys`
    - Example: `top`

## Running as a Docker container

This image expects a volume mounted at `/hyperdrive-home`.
This volume may be empty or may contain another nodes data.
It will be used as the home directory of your node.

The image includes EXPOSE directives for TCP port `8080` and TCP port `9000`.
Port `8080` is used for serving the Hyperdrive web dashboard over HTTP, and it may be mapped to a different port on the host.
Port `9000` is optional and is only required for a direct node.

If you are running a direct node, you must map port `9000` to the same port on the host and on your router.
Otherwise, your node will not be able to connect to the rest of the network as connection info is written to the chain, and this information is based on the view from inside the Docker container.

To build a local Docker image, run the following command in this project root.

```bash
# The `VERSION` may be replaced with the tag of a GitHub release
export VERSION=0.9.8

# Build for your system's architecture
docker build . -t hyperdrive-${VERSION} --build-arg VERSION=v${VERSION} --platform linux/amd64

# Build a multiarch image
docker buildx build . -t hyperdrive-${VERSION} --build-arg VERSION=v${VERSION} --platform arm64,amd64
```

To run, for example for a node named `helloworld.os`:

```bash
export NODENAME=helloworld.os

docker volume create hyperdrive-${NODENAME}

docker run -p 8080:8080 --rm -it --name hyperdrive-${NODENAME} --mount type=volume,source=hyperdrive-${NODENAME},destination=/hyperdrive-home hyperdrive-${VERSION}
```

which will launch your Kinode container attached to the terminal.
Alternatively you can run it detached:
```
docker run -p 8080:8080 --rm -dt --name hyperdrive-${NODENAME} --mount type=volume,source=hyperdrive-${NODENAME},destination=/hyperdrive-home hyperdrive-${VERSION}
```
Note that the `-t` flag *must* be passed.
If it is not passed, you must pass the `--detached` argument to the Kinode binary, i.e.
```
docker run -p 8080:8080 --rm -d --name hyperdrive-${NODENAME} --mount type=volume,source=hyperdrive-${NODENAME},destination=/hyperdrive-home hyperdrive-${VERSION} /hyperdrive-home --detached
```
