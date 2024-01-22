use anyhow::{Error as E, Result};
use clap::Parser;

use crate::ml::mixtral_sharded::{Config, EndModel, LinkModel, Model, OriginModel};

use candle_core::{DType, Device};
use candle_nn::VarBuilder;
use tokenizers::Tokenizer;

const SHARD_AMOUNT: usize = 4;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Run on CPU rather than on GPU.
    #[arg(long)]
    pub cpu: bool,

    /// Enable tracing (generates a trace-timestamp.json file).
    #[arg(long)]
    pub tracing: bool,

    #[arg(long)]
    pub use_flash_attn: bool,

    #[arg(long)]
    pub prompt: String,

    /// The temperature used to generate samples.
    #[arg(long)]
    pub temperature: Option<f64>,

    /// Nucleus sampling probability cutoff.
    #[arg(long)]
    pub top_p: Option<f64>,

    /// The seed to use when generating random samples.
    #[arg(long, default_value_t = 299792458)]
    pub seed: u64,

    /// The length of the sample to generate (in tokens).
    #[arg(long, short = 'n', default_value_t = 100)]
    pub sample_len: usize,

    #[arg(long, default_value = "mistralai/Mixtral-8x7B-v0.1")]
    pub model_id: String,

    #[arg(long, default_value = "main")]
    pub revision: String,

    #[arg(long)]
    pub tokenizer_file: Option<String>,

    #[arg(long)]
    pub weight_folder: String,

    /// Penalty to be applied for repeating tokens, 1. means no penalty.
    #[arg(long, default_value_t = 1.1)]
    pub repeat_penalty: f32,

    /// The context size to consider for the repeat penalty.
    #[arg(long, default_value_t = 64)]
    pub repeat_last_n: usize,
}

pub fn tokenizer(args: &Args) -> Result<Tokenizer> {
    let tokenizer_filename = match &args.tokenizer_file {
        Some(file) => std::path::PathBuf::from(file),
        None => panic!("--tokenizer-file is required"),
    };
    Tokenizer::from_file(tokenizer_filename).map_err(E::msg)
}

pub fn load_model(device: &Device, path: &std::path::PathBuf, shard_num: usize) -> Result<Model> {
    let start = std::time::Instant::now();

    // TODO: Luc: Should we use `use_flash_attn`? Test with and without
    let use_flash_attn = false;

    let config = Config::v0_1_8x7b(use_flash_attn);
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&vec![path], DType::F16, device)? };
    let model = if shard_num == 0 {
        Model::Origin(OriginModel::new(&config, vb)?)
    } else if shard_num == SHARD_AMOUNT - 1 {
        Model::End(EndModel::new(&config, vb)?)
    } else {
        Model::Link(LinkModel::new(&config, vb)?)
    };
    println!("loaded the model in {:?}", start.elapsed());
    Ok(model)
}

pub fn create_paths(args: &Args) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    for shard_num in 0..SHARD_AMOUNT {
        paths.push(std::path::PathBuf::from(format!(
            "{}mixtral-shard{}.safetensors",
            args.weight_folder, shard_num
        )));
    }
    paths
}
