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
<<<<<<< HEAD
    iteration: usize, // TODO: Zen: Remove
=======
>>>>>>> 990325b (Link start pos)
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
            start_pos: 0,
            kv_caches: None,
            tokenizer: TokenOutputStream::new(tokenizer),
            tokens: Default::default(),
            verbose: true,
        };
        Ok(result)
    }

    fn fill_tokens(&mut self, input: &MLInput) {
        match input {
            MLInput::Text(text) => {
                let _ = self.fill_tokens_with_prompt(&text);
            }
            MLInput::NextTokIdx(idx) => {
                self.tokens.push(idx);
            }
            _ => panic!("OriginProcessor::forward() called with invalid input"),
        }
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
        if !self.verbose {
            return Ok(());
        }
        let mut context = String::new();
        for &t in self.tokens.iter() {
            if let Ok(Some(t)) = self.tokenizer.next_token(t) {
                context.push_str(&t.clone());
            }
        }
        println!("Context: {}", context);
        Ok(())
    }

    // TODO: Zen: We assume that interactive mode doesn't exist yet.
    fn set_start_pos(&mut self, input: &MLInput) {
        self.start_pos = {
            match input {
<<<<<<< HEAD
                MLInput::Text(_) => {
                    if start_pos == 0 {
                        self.tokens.len()
                    } else {
                        self.tokens.len().saturating_sub(1)
                    }
                }
                MLInput::NextTokIdx(_) => self.start_pos + 1,
=======
                MLInput::Text(_) => 0,
                MLInput::NextTokIdx(_) => self.tokens.len() - 1,
>>>>>>> 990325b (Link start pos)
                _ => panic!("OriginProcessor::forward() called with invalid input"),
            }
        };
    }
}

impl Model for LMOriginShard {
    fn load_model_if_not_loaded(&mut self) -> Result<()> {
        if self.model.is_some() {
            return Ok(());
        }
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
        self.start_pos = 0;
    }

    // TODO: Zen: Don't return usize
    fn forward(&mut self, input: MLInput) -> Result<Tensor> {
        // TODO: Zen will we need this?
        let _ = self.load_model_if_not_loaded();

        self.fill_tokens(&input);
        self.print_context();
        self.set_start_pos(&input);

        let ctxt = &self.tokens[start_pos..];
        let input = Tensor::new(ctxt, &self.device)?.unsqueeze(0)?;

        println!("Start pos: {}", start_pos);
        println!("Ctxt: {:?}", ctxt);
        if let Some(model) = &mut self.model {
            return Ok(model.forward(&input, self.start_pos)?);
        }
        // TODO: Zen: If we encounter an eos token, we need to somehow stop generating
        panic!("No model was loaded, even though we tried to load one")
    }
}

// TODO: Zen: Food for thought: Why do we need to set the start_pos for origin before the forward, but for link and end, we set it after? This looks like there could be complexity that can be shaved off.
