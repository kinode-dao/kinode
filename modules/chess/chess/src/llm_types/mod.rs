// NOTE at some point these should be published as a crate
// and should also contain convenience functions

use serde::{Deserialize, Serialize};

mod llama_cpp;
mod open_ai;

#[allow(unused_imports)]
pub use llama_cpp::*;
#[allow(unused_imports)]
pub use open_ai::*;

/// Actions that can be taken by the main process
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MainAction {
    NewModel(NewModel),
    ListModels,
    RequestAccess(RequestAccess),
}

/// Actions that can be taken by the an LLM process
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum LlmAction {
    Chat(Chat),
    Embedding(Vec<String>),
    RequestAccess((String, u32)),
}

/// Create a new model
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewModel {
    pub name: String,
    pub config: LlmConfig,
}

/// A chat completion request to an LLM process
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Chat {
    pub prompt: String,
    pub params: ChatParams,
}

/// Additional configuration that can be used for chat requests
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatParams {
    pub max_tokens: Option<u64>, // aka n_predict
    pub stops: Option<Vec<String>>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    // pub logit_bias: Option<HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    // pub usage: Option<OpenAiChatCompletionUsage>,
    // retries: u64,
    // ms: u64,
}

/// A request for getting the capability to message an LLM process
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RequestAccess {
    /// Address i.e. someone.nec@model:llm:nectar
    pub model: String,
    /// of requests
    pub quantity: u32,
}

/// A request for getting the capability to message an LLM process
#[derive(Serialize, Deserialize, Debug, Clone, Hash, Eq, PartialEq)]
pub struct AccessCapability {
    /// of requests
    pub quantity: u32,
    /// source used to salt the capability
    pub who: String,
}

/// Configuration parameters for an OpenAI LLM process
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiConfig {
    pub api_key: String,
    /// | "gpt-4"
    /// | "gpt-4-0613"
    /// | "gpt-4-32k"
    /// | "gpt-4-32k-0613"
    /// | "gpt-3.5-turbo"
    /// | "gpt-3.5-turbo-0613"
    /// | "gpt-3.5-turbo-16k"
    /// | "gpt-3.5-turbo-16k-0613"
    /// | "gpt-4-1106-preview"
    pub chat_model: String,
    pub embedding_model: String,
    pub public: bool,
}

/// Configuration parameters for a LlamaCpp LLM process
/// Note that more configuration is required *outside* of nectar.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LlamaCppConfig {
    pub url: String,
    pub public: bool,
}

/// The kinds of models we currently support
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum LlmConfig {
    // TODO/NOTE there will probably be a *lot* more config options added here later
    OpenAi(OpenAiConfig),
    LlamaCpp(LlamaCppConfig),
}

impl LlmConfig {
    pub fn public(&self) -> bool {
        match self {
            LlmConfig::OpenAi(config) => config.public,
            LlmConfig::LlamaCpp(config) => config.public,
        }
    }
}

/// capabilities for limiting messaging to models
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QuantityCapability {
    pub model: String,
    pub quantity: u32, // maybe u64...tricky with json encoding
    pub salt: u32,
}
