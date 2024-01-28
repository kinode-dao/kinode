use crate::{MLInput, MLOutput};

pub trait Model {
    fn load_model_if_not_loaded(&mut self);

    fn unload_model(&mut self);

    fn clear(&mut self);

    // TODO: Zen: Make the output a generic MLOutput
    fn forward(&mut self, input: MLInput) -> Result<MLOutput>;
}
