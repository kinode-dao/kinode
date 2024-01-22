pub trait Processor {
    fn load_model(&mut self);

    fn unload(&mut self);

    fn clear(&mut self);

    fn forward(&mut self);
}
