Last updated: 10/02/23
## Setup

### Building components

```bash
# Clone the repo.

git clone git@github.com:uqbar-dao/uqbar.git

# Get some stuff so we can build wasm.

cargo install wasm-tools
rustup install nightly
rustup target add wasm32-wasi
rustup target add wasm32-wasi --toolchain nightly
cargo install cargo-wasi
cargo install --git https://github.com/bytecodealliance/cargo-component --locked cargo-component

# Build the runtime, along with a number of booted-at-startup WASM modules including terminal and key_value
# OPTIONAL: --release flag
cargo +nightly build

# Create the home directory for your node
# If you boot multiple nodes, make sure each has their own home directory.
mkdir home
```

### Boot

Before booting, compile all the apps with `./build.sh --all` (this may take some time). Then, booting your node takes one argument: the home directory where all your files will be stored. You can use your own custom eth-rpc URL using the `--rpc` flag (NOTE: RPC URL must begin with `wss://` NOT `https://`). You can also include the `--release` if you want optimized performance.

If you do not receive QNS updates in terminal, it's a sign that the default public-access RPC endpoint is rate-limiting or blocking you. Get an eth-sepolia-rpc API key and pass that as an argument. You can get one for free at `alchemy.com`.

Also, make sure not to use the same home directory for two nodes at once! You can use any name for the home directory.
```bash
cargo +nightly run --release home
```

On boot you will be prompted to navigate to `localhost:8080`. Make sure your eth wallet is connected to the Sepolia test network. Login should be very straightforward, just submit the transactions and follow the flow.

### Development
Running `./build.sh` will automatically build any apps that have changes in git. Developing with `./build.sh && cargo +nightly run home` anytime you make a change to your app should be very fast. You can also manually recompile just your app with, for example, `./build-app chess`, where `chess` is the folder name inside `/modules`.

## Terminal syntax

- CTRL+C or CTRL+D to shutdown node
- CTRL+V to toggle verbose mode, which is on by default
- CTRL+J to toggle debug mode
- CTRL+S to step through events in debug mode

- CTRL+A to jump to beginning of input
- CTRL+E to jump to end of input
- UpArrow/DownArrow or CTRL+P/CTRL+N to move up and down through command history
- CTRL+R to search history, CTRL+R again to toggle through search results, CTRL+G to cancel search

- `!message <name> <app> <json>`: send a card with a JSON value to another node or yourself. <name> can be `our`, which will be interpreted as our node's username.
- `!hi <name> <string>`: send a text message to another node's command line.
- `<name>` is either the name of a node or `our`, which will fill in the present node name
- more to come

## Example usage

TODO
