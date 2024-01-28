pub trait Model {
    fn load_model_if_not_loaded(&mut self);

    fn unload_model(&mut self);

    fn clear(&mut self);

    fn forward(&mut self);
}
