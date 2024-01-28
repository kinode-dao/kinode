use crate::ml::mixtral_sharded::Model;
use anyhow::Result;

use crate::ml::device;
use crate::ml::mixtral_sharded::LinkModel;
use crate::ml::util::create_paths;
use crate::ml::util::load_model;
use crate::ml::util::Args;
use candle_core::{Device, Tensor};

use super::model::Model;

pub struct LinkProcessor {
    model: Option<LinkModel>,
    model_path: std::path::PathBuf,
    device: Device,
    iteration: usize,
    kv_caches: Option<Vec<(Tensor, Tensor)>>,

    shard_num: usize,
}

impl LinkProcessor {
    pub fn new(args: &Args, shard_num: usize) -> Result<Self> {
        let paths = create_paths(&args);
        let device = device::device(args.cpu)?;
        let model_path = paths[shard_num].clone();
        println!("Loading model from {:?}", model_path);
        let result = Self {
            model: None,
            model_path,
            device,
            iteration: Default::default(),
            shard_num,
            kv_caches: None,
        };
        Ok(result)
    }
}

impl Model for LinkProcessor {
    fn load_model(&mut self) -> Result<()> {
        let Model::Link(model) = load_model(&self.device, &self.model_path, self.shard_num)? else {
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

    #[allow(unused)] // TODO: Luc
    fn clear(&mut self) {
        self.iteration = 0;
    }

    fn forward(&mut self, activation: &Tensor, start_pos: usize) -> Result<Tensor> {
        if self.model.is_none() {
            if let Err(e) = self.load_model() {
                println!("Error loading model: {:?}", e);
            }
        }

        if let Some(model) = &mut self.model {
            return Ok(model.forward(activation, start_pos)?);
        }
        panic!("Something went terribly wrong")
    }
}
