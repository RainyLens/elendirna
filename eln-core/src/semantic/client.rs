use crate::error::ElfError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct EmbeddingsClient {
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl EmbeddingsClient {
    pub fn new(endpoint: String, model: String, api_key: Option<String>) -> Self {
        Self {
            endpoint,
            model,
            api_key,
        }
    }

    pub async fn embed(&self, inputs: &[&str]) -> Result<Vec<Vec<f32>>, ElfError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/embeddings", self.endpoint.trim_end_matches('/'));
        let body = EmbeddingsRequest {
            model: &self.model,
            input: inputs,
        };
        let client = reqwest::Client::new();
        let mut request = client.post(url).json(&body);
        if let Some(api_key) = self.api_key.as_deref() {
            request = request.bearer_auth(api_key);
        }

        let response = request.send().await.map_err(|e| ElfError::InvalidInput {
            message: format!(
                "semantic embeddings endpoint request failed: {e}. hint: {}",
                crate::semantic::SEMANTIC_HINT
            ),
        })?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ElfError::InvalidInput {
                message: format!(
                    "semantic embeddings endpoint returned {status}: {text}. hint: {}",
                    crate::semantic::SEMANTIC_HINT
                ),
            });
        }

        let parsed: EmbeddingsResponse =
            response.json().await.map_err(|e| ElfError::ParseError {
                message: format!("embeddings response parse failed: {e}"),
            })?;
        let mut data = parsed.data;
        data.sort_by_key(|item| item.index);
        if data.len() != inputs.len() {
            return Err(ElfError::ParseError {
                message: format!(
                    "embeddings response count mismatch: expected {}, got {}",
                    inputs.len(),
                    data.len()
                ),
            });
        }
        Ok(data.into_iter().map(|item| item.embedding).collect())
    }
}

#[derive(Serialize)]
struct EmbeddingsRequest<'a> {
    model: &'a str,
    input: &'a [&'a str],
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Deserialize)]
struct EmbeddingItem {
    index: usize,
    embedding: Vec<f32>,
}
