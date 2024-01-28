use anyhow::{Error as E, Result};

use crate::ml::mixtral_sharded::Model;
use crate::ml::mixtral_sharded::OriginModel;
use crate::ml::token_output_stream::TokenOutputStream;
use crate::MLInput;

use super::model::Model;
use super::util::create_paths;
use super::util::load_model;
use super::util::tokenizer;
use super::util::Args;
use crate::ml::device;
use candle_core::{Device, Tensor};

#[derive(Debug, Clone, PartialEq)]
pub struct LMOriginShard {
    model: Option<OriginModel>,
    model_path: std::path::PathBuf,
    device: Device,
    iteration: usize, // TODO: Zen: Remove
    start_pos: usize, 
    kv_caches: Option<Vec<(Tensor, Tensor)>>,

    tokenizer: TokenOutputStream,
    tokens: Vec<u32>,
    verbose: bool,
}

impl LMOriginShard {
    pub fn new(args: &Args) -> Result<Self> {
        let paths = create_paths(&args);
        let device = device::device(args.cpu)?;
        println!("Device: {:?}", device);
        let tokenizer = tokenizer(&args)?;
        let path = paths[0].clone();
        println!("Loading model from {:?}", path);
        let result = Self {
            model: None,
            model_path: path, 
            device,
            iteration: Default::default(),
            start_pos: 0,
            kv_caches: None,
            tokenizer: TokenOutputStream::new(tokenizer),
            tokens: Default::default(),
            verbose: true,
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

    fn set_verbose(&mut self, verbose: bool) {
        self.verbose = verbose;
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

impl Model for LMOriginShard {
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

    fn unload_model(&mut self) {
        if let Some(model) = &self.model {
            self.kv_caches = Some(model.get_kv_caches());
        }
        self.model = None;
    }

    fn clear(&mut self) {
        self.tokenizer.clear();
        self.iteration = 0;
    }

    /// Returns the activations and the start pos
    fn forward(&mut self, input: MLInput) -> Result<(Tensor, usize)> {
        // TODO: Zen will we need this? 
        if self.model.is_none() {
            let _ = self.load_model();
        }

        match input {
            MLInput::Text(text) => {
                let _ = self.fill_tokens_with_prompt(&text);
            }
            MLInput::NextTokIdx(idx) => {
                self.tokens.push(idx);
            }
            _ => panic!("OriginProcessor::forward() called with invalid input"),
        }

        if self.verbose {
            let _ = self.print_context();
        }

        self.start_pos = {
            match input {
                MLInput::Text(_) => {
                    if start_pos == 0 {
                        self.tokens.len()
                    } else {
                        self.tokens.len().saturating_sub(1)
                    }
                },
                MLInput::NextTokIdx(_) => self.start_pos + 1,
                _ => panic!("OriginProcessor::forward() called with invalid input"),
            }
        };

        let ctxt = &self.tokens[start_pos..];
        let input = Tensor::new(ctxt, &self.device)?.unsqueeze(0)?;


        println!("Start pos: {}", start_pos);
        println!("Ctxt: {:?}", ctxt);
        if let Some(model) = &mut self.model {
            return Ok((model.forward(&input, start_pos)?, start_pos));
        }
        // TODO: Luc: If we encounter an eos token, we need to somehow stop generating
        panic!("Something went terribly wrong")
    }
}
