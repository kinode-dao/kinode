# Local LLM Integration
1. Clone and build [llama.cpp](https://github.com/ggerganov/llama.cpp) on the same machine where you will run your uqbar node
  - follow their README for details on how to do this. In most cases simply running `make` works
  - make sure to get your model as a .gguf file
2. Within the llama.cpp directory, run this command in llama.cpp on the same machine you will run your uqbar node: `./server --port <PORT>`
  - Note: you can pass in whatever other command line arguments to the llama cpp server you want depending on your preferences/hardware/model/etc.
3. Run your Uqbar node with `--features llm` and `--llm http://localhost:<PORT>`. For example `cargo +nightly run --features llm --release home --rpc wss://eth-sepolia.g.alchemy.com/v2/<YOUR_API_KEY> --llm http://localhost:<PORT>`
