Last updated: 11/01/23
## Setup

### Building components

```bash
# Clone the repo.

git clone git@github.com:uqbar-dao/uqbar.git


# Get some stuff so we can build wasm.

cd uqbar
cargo install wasm-tools
rustup install nightly
rustup target add wasm32-wasi
rustup target add wasm32-wasi --toolchain nightly
cargo install cargo-wasi

# Build the runtime, along with a number of booted-at-startup WASM modules including terminal and key_value
# OPTIONAL: --release flag

cargo +nightly build --release
```

### Boot
Get an eth-sepolia-rpc API key and pass that as an argument. You can get one for free at `alchemy.com`.

Make sure not to use the same home directory for two nodes at once! You can use any name for the home directory: here we just use `home`.
```bash
cargo +nightly run --release home --rpc wss://eth-sepolia.g.alchemy.com/v2/<your-api-key>
```

On boot you will be prompted to navigate to `localhost:8080`. Make sure your ETH wallet is connected to the Sepolia test network. Login should be straightforward, just submit the transactions and follow the flow. If you want to register a new ID you will either need [Sepolia testnet tokens](https://www.infura.io/faucet/sepolia) or an invite code.


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

Download and install an app:
```
!message our main:app_store:uqbar {"Download": {"package": {"package_name": "<pkg>", "publisher_node": "<node>"}, "install_from": "<node>"}}
!message our main:app_store:uqbar {"Install": {"package_name": "<pkg>", "publisher_node": "<node>"}}
```
