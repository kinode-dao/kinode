use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiEmbeddingData {
    pub index: u64,
    pub object: String,
    pub embedding: Vec<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiUsage {
    pub prompt_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiEmbeddingResponse {
    pub data: Vec<OpenAiEmbeddingData>,
    pub model: String,
    pub object: String,
    pub usage: OpenAiUsage,
}

// | "gpt-4"
// | "gpt-4-0613"
// | "gpt-4-32k"
// | "gpt-4-32k-0613"
// | "gpt-3.5-turbo"
// | "gpt-3.5-turbo-0613"
// | "gpt-3.5-turbo-16k"
// | "gpt-3.5-turbo-16k-0613"
// | "gpt-4-1106-preview"
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiChatCompletionRequestBody {
    pub model: String,
    pub messages: Vec<OpenAiLLMMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>, // TODO: switch false to boolean when we support it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiLLMMessage {
    pub content: Option<String>, // TODO I don't think this is an option...
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCall>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiChatCompletionMessage {
    pub role: String, // 'system' | 'user' | 'assistant' | 'function'
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiChatCompletionChoice {
    pub index: Option<u64>,
    pub message: Option<OpenAiChatCompletionMessage>,
    pub finish_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiChatCompletionUsage {
    pub completion_tokens: u64,
    pub prompt_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenAiCreateChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<OpenAiChatCompletionChoice>,
    pub usage: Option<OpenAiChatCompletionUsage>,
}
