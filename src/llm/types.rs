use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize)]
pub struct LlmPrompt {
    pub prompt: String,
    pub n_predict: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LlmResponse {
    pub content: String,
    pub generation_settings: GenerationSettings,
    pub model: String,
    pub prompt: String,
    pub slot_id: u64,
    pub stop: bool,
    pub stopped_eos: bool,
    pub stopped_limit: bool,
    pub stopped_word: bool,
    pub stopping_word: String,
    pub timings: Timings,
    pub tokens_cached: u64,
    pub tokens_evaluated: u64,
    pub tokens_predicted: u64,
    pub truncated: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GenerationSettings {
    pub frequency_penalty: f64,
    pub grammar: String,
    pub ignore_eos: bool,
    pub logit_bias: Vec<serde_json::Value>, // This should be changed to the appropriate type
    pub mirostat: u64,
    pub mirostat_eta: f64,
    pub mirostat_tau: f64,
    pub model: String,
    pub n_ctx: u64,
    pub n_keep: u64,
    pub n_predict: u64,
    pub n_probs: u64,
    pub penalize_nl: bool,
    pub presence_penalty: f64,
    pub repeat_last_n: u64,
    pub repeat_penalty: f64,
    pub seed: u64,
    pub stop: Vec<serde_json::Value>, // This should be changed to the appropriate type
    pub stream: bool,
    pub temp: f64,
    pub tfs_z: f64,
    pub top_k: u64,
    pub top_p: f64,
    pub typical_p: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Timings {
    pub predicted_ms: f64,
    pub predicted_n: u64,
    pub predicted_per_second: f64,
    pub predicted_per_token_ms: f64,
    pub prompt_ms: f64,
    pub prompt_n: u64,
    pub prompt_per_second: f64,
    pub prompt_per_token_ms: f64,
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum LlmError {
    #[error("llm: rsvp is None but message is expecting response")]
    BadRsvp,
    #[error("llm: no json in request")]
    NoJson,
    #[error(
        "llm: JSON payload could not be parsed to LlmPrompt: {error}. Got {:?}.",
        json
    )]
    BadJson { json: String, error: String },
    #[error("llm: http method not supported: {:?}", method)]
    BadMethod { method: String },
    #[error("llm: failed to execute request {:?}", error)]
    RequestFailed { error: String },
    #[error("llm: failed to deserialize response {:?}", error)]
    DeserializationToLlmResponseFailed { error: String },
}
