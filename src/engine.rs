use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use futures::lock::Mutex;
use ethers::utils::keccak256;
use hasher::{Hasher, HasherKeccak};
use cita_trie::MemoryDB;
use cita_trie::{PatriciaTrie, Trie};
use ethers::prelude::*;
use serde::{Serialize, Deserialize};

// use anyhow::{anyhow, Result};
use wasmtime::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateItem {
    pub source: H256,
    pub holder: H256,
    pub town_id: u32,
    pub salt: Bytes,
    pub label: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractItem {
    pub source: H256,
    pub holder: H256,
    pub town_id: u32,
    pub code_hex: String, // source code of contract represented as hex string?
}

struct _ContractContext {
    this: H256,
}
type _Process = Arc<Mutex<_ContractContext>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub from: H256,
    pub signature: Option<Signature>,
    pub to: H256, // contract address
    pub town_id: u32,
    pub calldata: serde_json::Value,
    pub nonce: U256,
    pub gas_price: U256,
    pub gas_limit: U256,
}

impl Transaction {
    pub fn hash(&self) -> H256 {
        let mut _hasher = HasherKeccak::new();
        let message = format!("{}{}{}{}{}{}{}",
            self.from,
            self.to,
            self.town_id,
            self.calldata,
            self.nonce,
            self.gas_price,
            self.gas_limit,
        );
        keccak256(message).into()
    }
}

pub struct UqChain {
    state: PatriciaTrie<MemoryDB, HasherKeccak>,
    nonces: HashMap<H256, U256>,
}

impl UqChain {
    pub fn new() -> UqChain {
        let memdb = Arc::new(MemoryDB::new(true));
        let hasher = Arc::new(HasherKeccak::new());

        let my_item = StateItem {
                    source: H256::zero(),
                    holder: H256::zero(),
                    town_id: 0,
                    salt: Bytes::new(),
                    label: String::new(),
                    data: serde_json::Value::Null,
                  };

        let my_contract = ContractItem {
                    source: H256::zero(),
                    holder: H256::zero(),
                    town_id: 0,
                    code_hex: "(module
                        (func (export \"write\") (param i32 i32) (result i32)
                            local.get 0
                            local.get 1
                            i32.add
                        )
                      )".into(),
                  };

        let item_key = HasherKeccak::digest(&hasher, &bincode::serialize(&my_item).unwrap());

        let contract_key: H256 = "0x0000000000000000000000000000000000000000000000000000000000005678".parse().unwrap();

        println!("contract id: {:?}", contract_key);

        let mut trie = PatriciaTrie::new(Arc::clone(&memdb), Arc::clone(&hasher));
        trie.insert(item_key.to_vec(), bincode::serialize(&my_item).unwrap()).unwrap();
        trie.insert(contract_key.as_bytes().to_vec(), bincode::serialize(&my_contract).unwrap()).unwrap();

        UqChain {
            state: trie,
            nonces: HashMap::new(),
        }
    }

    pub fn run_batch(self, txns: Vec<Transaction>) -> UqChain {
        return engine(self, txns);
    }
}

pub fn engine(chain: UqChain, txns: Vec<Transaction>) -> UqChain {
    let start_time = Instant::now();
    // An engine stores and configures global compilation settings like
    // optimization level, enabled wasm features, etc.
    let wasm_engine = Engine::default();

    // sort txns by gas_price
    let txns = sort_transactions(txns);
    // execute txns in order
    for txn in txns {
        // check signature
        match txn.signature {
            Some(sig) => {
                let message = txn.hash();
                match sig.verify(message, txn.from) {
                    Ok(_) => {},
                    Err(_) => { continue; }
                }
            },
            None => {}, // TODO handle unsigned transactions to abstract accounts
        }
        // check nonce
        let last_nonce = *chain.nonces.get(&txn.from).unwrap_or(&U256::zero());
        if txn.nonce != last_nonce + 1 {
            continue;
        }
        // audit account's gas balance
        // XX
        // execute transaction against current chain state
        // We start off by creating a `Module` which represents a compiled form
        // of our input wasm module. In this case it'll be JIT-compiled after
        // we parse the text format.
        let contract_item = chain.state.get(&txn.to.as_bytes()).unwrap().unwrap();
        let contract_item = bincode::deserialize::<ContractItem>(&contract_item).unwrap();
        let contract = Module::new(&wasm_engine, contract_item.code_hex).unwrap();

        // A `Store` is what will own instances, functions, globals, etc. All wasm
        // items are stored within a `Store`, and it's what we'll always be using to
        // interact with the wasm world. Custom data can be stored in stores but for
        // now we just use `()`.
        let mut store = Store::new(&wasm_engine, txn.to);

        // With a compiled `Module` we can then instantiate it, creating
        // an `Instance` which we can actually poke at functions on.
        let instance = Instance::new(&mut store, &contract, &[]).unwrap();

        // The `Instance` gives us access to various exported functions and items,
        // which we access here to pull out our `answer` exported function and
        // run it.
        let write_func = instance.get_func(&mut store, "write")
            .expect("`write` was not an exported function");

        // There's a few ways we can call the `answer` `Func` value. The easiest
        // is to statically assert its signature with `typed` (in this case
        // asserting it takes no arguments and returns one i32) and then call it.
        let typed_write_func = write_func.typed::<(i32, i32), i32>(&store).unwrap();

        // And finally we can call our function! Note that the error propagation
        // with `?` is done to handle the case where the wasm function traps.
        let result = typed_write_func.call(&mut store, (3, 5)).unwrap();
        println!("Answer: {:?}", result);
            // validate output
    }

    let exec_duration = start_time.elapsed();
    println!("engine: time taken to execute: {:?}", exec_duration);
    // return updated chain
    chain
}

/// produce ordered vector of transactions by gas_price, adjusting for nonce of caller.
/// XX check for correctness
fn sort_transactions(mut txns: Vec<Transaction>) -> Vec<Transaction> {
    txns.sort_unstable_by(|a, b|
        a.gas_price.cmp(&b.gas_price)
    );
    txns.sort_by(|a, b|
        a.nonce.cmp(&b.nonce)
    );
    txns
}
