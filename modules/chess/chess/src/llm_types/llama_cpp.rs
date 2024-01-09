use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct LlamaCppPrompt {
    pub prompt: String, // TODO can be a string or an array of strings
    pub temperature: Option<f64>,
    pub top_k: Option<usize>,
    pub top_p: Option<f64>,
    pub n_predict: Option<isize>, // isize to accommodate -1
    pub n_keep: Option<isize>,    // isize to accommodate -1
    pub stream: Option<bool>,
    pub stop: Option<Vec<String>>,
    pub tfs_z: Option<f64>,
    pub typical_p: Option<f64>,
    pub repeat_penalty: Option<f64>,
    pub repeat_last_n: Option<isize>, // isize to accommodate -1
    pub penalize_nl: Option<bool>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub mirostat: Option<u8>, // u8 as it's 0, 1, or 2
    pub mirostat_tau: Option<f64>,
    pub mirostat_eta: Option<f64>,
    pub grammar: Option<String>,
    pub seed: Option<isize>, // isize to accommodate -1
    pub ignore_eos: Option<bool>,
    pub logit_bias: Option<Vec<(usize, f64)>>,
    pub n_probs: Option<usize>,
    pub image_data: Option<Vec<LlamaCppImageData>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LlamaCppImageData {
    data: String, // Base64 string
    id: usize,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LlamaCppResponse {
    pub content: String,
    pub generation_settings: Option<LlamaCppGenerationSettings>,
    pub model: Option<String>,
    pub prompt: Option<String>,
    pub slot_id: Option<u64>,
    pub stop: Option<bool>,
    pub stopped_eos: Option<bool>,
    pub stopped_limit: Option<bool>,
    pub stopped_word: Option<bool>,
    pub stopping_word: Option<String>,
    pub timings: Option<LlamaCppTimings>,
    pub tokens_cached: Option<u64>,
    pub tokens_evaluated: Option<u64>,
    pub tokens_predicted: Option<u64>,
    pub truncated: Option<bool>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LlamaCppGenerationSettings {
    pub frequency_penalty: Option<f64>,
    pub grammar: Option<String>,
    pub ignore_eos: Option<bool>,
    pub logit_bias: Option<Vec<(usize, f64)>>,
    pub mirostat: Option<u64>,
    pub mirostat_eta: Option<f64>,
    pub mirostat_tau: Option<f64>,
    pub model: Option<String>,
    pub n_ctx: Option<u64>,
    pub n_keep: Option<u64>,
    pub n_predict: Option<isize>,
    pub n_probs: Option<u64>,
    pub penalize_nl: Option<bool>,
    pub presence_penalty: Option<f64>,
    pub repeat_last_n: Option<u64>,
    pub repeat_penalty: Option<f64>,
    pub seed: Option<u64>,
    pub stop: Option<Vec<String>>,
    pub stream: Option<bool>,
    pub temp: Option<f64>,
    pub tfs_z: Option<f64>,
    pub top_k: Option<u64>,
    pub top_p: Option<f64>,
    pub typical_p: Option<f64>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LlamaCppTimings {
    pub predicted_ms: Option<f64>,
    pub predicted_n: Option<u64>,
    pub predicted_per_second: Option<f64>,
    pub predicted_per_token_ms: Option<f64>,
    pub prompt_ms: Option<f64>,
    pub prompt_n: Option<u64>,
    pub prompt_per_second: Option<f64>,
    pub prompt_per_token_ms: Option<f64>,
}
