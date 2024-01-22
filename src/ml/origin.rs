use anyhow::{Error as E, Result};

use crate::ml::mixtral_sharded::Model;
use crate::ml::mixtral_sharded::OriginModel;
use crate::ml::token_output_stream::TokenOutputStream;

use super::processor::Processor;
use super::util::create_paths;
use super::util::load_model;
use super::util::tokenizer;
use super::util::Args;
use crate::ml::device;
use candle_core::{Device, Tensor};

#[derive(Debug, Clone, PartialEq)]
pub enum OriginInput {
    Prompt(String),
    NextTokIdx(u32),
}

pub struct OriginProcessor {
    model: Option<OriginModel>,
    model_path: std::path::PathBuf,
    device: Device,
    tokenizer: TokenOutputStream,
    iteration: usize,
    tokens: Vec<u32>,
    kv_caches: Option<Vec<(Tensor, Tensor)>>,
}

impl OriginProcessor {
    pub fn new(args: &Args) -> Result<Self> {
        let paths = create_paths(&args);
        let device = device::device(args.cpu)?;
        println!("Device: {:?}", device);
        let tokenizer = tokenizer(&args)?;
        let path = paths[0].clone();
        println!("Loading model from {:?}", path);
        let result = Self {
            model: None,
            model_path: path, // Origin Processor always has shard num 0
            device,
            tokenizer: TokenOutputStream::new(tokenizer),
            iteration: Default::default(),
            tokens: Default::default(),
            kv_caches: None,
        };
        Ok(result)
    }

    fn fill_tokens_with_prompt(&mut self, prompt: &str) -> Result<()> {
        self.tokens = self
            .tokenizer
            .tokenizer()
            .encode(prompt, true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();
        Ok(())
    }

    fn print_context(&mut self) -> Result<()> {
        let mut context = String::new();
        for &t in self.tokens.iter() {
            if let Ok(Some(t)) = self.tokenizer.next_token(t) {
                context.push_str(&t.clone());
            }
        }
        println!("Context: {}", context);
        Ok(())
    }
}

impl Processor for OriginProcessor {
    fn load_model(&mut self) -> Result<()> {
        let Model::Origin(model) = load_model(&self.device, &self.model_path, 0)? else {
            panic!("Model is not origin")
        };
        self.model = Some(model);
        if let Some(kv_caches) = self.kv_caches.take() {
            if let Some(model) = &mut self.model {
                model.set_kv_caches(kv_caches);
                println!("Size of kv_caches: {}", model.get_kv_caches().len());
            }
        }
        Ok(())
    }
    fn unload(&mut self) {
        if let Some(model) = &self.model {
            self.kv_caches = Some(model.get_kv_caches());
        }
        self.model = None;
    }

    #[allow(unused)] // TODO: Luc
    fn clear(&mut self) {
        self.tokenizer.clear();
        self.iteration = 0;
    }

    /// Returns the activations and the start pos
    fn forward(
        &mut self,
        iteration: usize,
        input: OriginInput,
        verbose: bool,
    ) -> Result<(Tensor, usize)> {
        if self.model.is_none() {
            let _ = self.load_model();
        }

        match input {
            OriginInput::Prompt(prompt) => {
                let _ = self.fill_tokens_with_prompt(&prompt);
            }
            OriginInput::NextTokIdx(idx) => {
                self.tokens.push(idx);
            }
        }
        if verbose {
            let _ = self.print_context();
        }
        let context_size = if iteration > 0 { 1 } else { self.tokens.len() };

        self.iteration += 1;

        let start_pos = self.tokens.len().saturating_sub(context_size);
        println!("Start pos: {}", start_pos);
        let ctxt = &self.tokens[start_pos..];
        println!("Ctxt: {:?}", ctxt);
        let input = Tensor::new(ctxt, &self.device)?.unsqueeze(0)?;

        if let Some(model) = &mut self.model {
            return Ok((model.forward(&input, start_pos)?, start_pos));
        }
        // TODO: Luc: If we encounter an eos token, we need to somehow stop generating
        panic!("Something went terribly wrong")
    }
}
