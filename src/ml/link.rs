use core::panic;

use crate::ml::mixtral_sharded::MixtralModel;
use anyhow::Result;

use crate::ml::device;
use crate::ml::mixtral_sharded::LinkModel;
use crate::ml::util::create_paths;
use crate::ml::util::load_model;
use crate::ml::util::Args;
use candle_core::{Device, Tensor};

use super::model::Model;

pub struct LMLinkShard {
    model: Option<LinkModel>,
    model_path: std::path::PathBuf,
    device: Device,
    start_pos: usize,

    kv_caches: Option<Vec<(Tensor, Tensor)>>,

    shard_num: usize,
}

impl LMLinkShard {
    pub fn new(args: &Args, shard_num: usize) -> Result<Self> {
        let paths = create_paths(&args);
        let device = device::device(args.cpu)?;
        let model_path = paths[shard_num].clone();
        println!("Loading model from {:?}", model_path);
        let result = Self {
            model: None,
            model_path,
            device,
            start_pos: 0,
            kv_caches: None,
            shard_num,
        };
        Ok(result)
    }

    fn set_start_pos(&mut self, activation: &Tensor) {
        let received_ctx_len = activation.shape().dims()[1];
        self.start_pos += received_ctx_len;
    }
}

impl Model for LMLinkShard {
    fn load_model_if_not_loaded(&mut self) -> Result<()> {
        if self.model.is_some() {
            return Ok(());
        }
        let MixtralModel::Link(model) = load_model(&self.device, &self.model_path, self.shard_num)?
        else {
            panic!("Model is not link")
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
        self.start_pos = 0;
    }

    fn forward(&mut self, activation: &Tensor) -> Result<Tensor> {
        _ = self.load_model_if_not_loaded();

        let output: Tensor = if let Some(model) = &mut self.model {
            model.forward(activation, self.start_pos)?
        } else {
            panic!("No model was loaded, even though we tried to load one")
        };

        self.set_start_pos(activation);
        Ok(output)
    }
}
