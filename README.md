## Setup

### Building components

```bash
# Clone the repo.

git clone git@github.com:uqbar-dao/nectar.git

# Configure dependency retrieval from GitHub
mkdir .cargo
echo "net.git-fetch-with-cli = true" > .cargo/config

# Get some stuff so we can build Wasm.

cd nectar
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

Make sure not to use the same home directory for two nodes at once! You can use any name for the home directory: here we just use `home`. The `--` here separates cargo arguments from binary arguments.

TODO: document feature flags `--simulation-mode`
```bash
cargo +nightly run --release -- home --rpc wss://eth-sepolia.g.alchemy.com/v2/<your-api-key>
```

On boot you will be prompted to navigate to `localhost:8080`. Make sure your ETH wallet is connected to the Sepolia test network. Login should be straightforward, just submit the transactions and follow the flow. If you want to register a new ID you will either need [Sepolia testnet tokens](https://www.infura.io/faucet/sepolia) or an invite code.


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

- `/message <address> <json>`: send an inter-process message. <address> is formatted as <node>@<process_id>. <process_id> is formatted as <process_name>:<package_name>:<publisher_node>.
    - Example: `/message our@net:sys:nectar diagnostics`
    - `our` will always be interpolated by the system as your node's name
    - Can also use `/m` for same command: `/m our@net:sys:nectar diagnostics`
- `/app <address>`: set the terminal to a mode where all messages go to a specific app. To clear this selection, use `/app clear` or simply `/app`. This is useful for apps that have a command line interface.
    - Example: `/app our@net:sys:nectar`, then `/m diagnostics`
    - Can also use `/a` for same command: `/a our@net:sys:nectar`
    - Example of sending many messages:
        - `/a ben.uq@net:sys:nectar`
        - `/m hey there`
        - `/m how are you?`
        - `/a` (to exit app mode)
- `/hi <name> <string>`: send a text message to another node's command line.
    - Example: `/hi ben.uq hello world`

## Example usage

Download and install an app:
```
/m our@main:app_store:nectar {"Download": {"package": {"package_name": "<pkg>", "publisher_node": "<node>"}, "install_from": "<node>"}}
/m our@main:app_store:nectar {"Install": {"package_name": "<pkg>", "publisher_node": "<node>"}}
```
