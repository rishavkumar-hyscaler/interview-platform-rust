use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone)]
pub struct AiProvider {
    client: reqwest::Client,
    base: String,
    llm_key: String,
    embedding_key: String,
    pub model: String,
    pub embedding_model: String,
    pub dimensions: usize,
    pub count: usize,
    pub similarity: f64,
}
impl AiProvider {
    pub fn from_environment() -> Result<Self> {
        ensure!(
            std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "openai".into()) == "openai",
            "LLM_PROVIDER must be openai"
        );
        Self::new(
            std::env::var("LLM_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            std::env::var("LLM_API_KEY")?,
            std::env::var("EMBEDDING_API_KEY")?,
            std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-5-mini".into()),
            std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "text-embedding-3-small".into()),
        )
    }
    pub fn new(
        base: String,
        llm_key: String,
        embedding_key: String,
        model: String,
        embedding_model: String,
    ) -> Result<Self> {
        ensure!(
            !llm_key.trim().is_empty() && !embedding_key.trim().is_empty(),
            "AI API keys are not configured"
        );
        let url = reqwest::Url::parse(&base)?;
        ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "AI base URL is invalid"
        );
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(90))
                .connect_timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base: base.trim_end_matches('/').into(),
            llm_key,
            embedding_key,
            model,
            embedding_model,
            dimensions: 1536,
            count: 6,
            similarity: 0.92,
        })
    }
    async fn post(&self, path: &str, key: &str, body: Value) -> Result<Value> {
        let mut response = self
            .client
            .post(format!("{}{path}", self.base))
            .bearer_auth(key)
            .json(&body)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("AI service connection failed or timed out"))?;
        ensure!(
            response.status().is_success(),
            "AI service returned HTTP {}",
            response.status().as_u16()
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("AI service response could not be read"))?
        {
            ensure!(
                bytes.len() + chunk.len() <= 1024 * 1024,
                "AI service response exceeded size limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("AI service returned invalid JSON"))
    }
    pub async fn structured<T: JsonSchema + DeserializeOwned>(
        &self,
        name: &str,
        instructions: &str,
        input: Value,
    ) -> Result<T> {
        let mut schema = serde_json::to_value(schemars::schema_for!(T))?;
        clean_schema(&mut schema);
        let response=self.post("/responses",&self.llm_key,json!({
            "model":self.model,"store":false,"max_output_tokens":12000,
            "instructions":instructions,
            "input":[{"role":"user","content":serde_json::to_string(&input)?}],
            "text":{"format":{"type":"json_schema","name":name,"strict":true,"schema":schema}}
        })).await?;
        ensure!(
            response["status"] == "completed",
            "AI response was incomplete or refused"
        );
        let mut text = String::new();
        for item in response["output"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("AI response has no output"))?
        {
            if item["type"] != "message" {
                continue;
            }
            for content in item["content"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("AI message has no content"))?
            {
                ensure!(
                    content["type"] != "refusal",
                    "AI refused to generate questions"
                );
                if content["type"] == "output_text" {
                    text.push_str(
                        content["text"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("AI output text is invalid"))?,
                    );
                }
            }
        }
        serde_json::from_str(&text)
            .map_err(|_| anyhow::anyhow!("AI output did not match the required schema"))
    }
    pub async fn embeddings(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        let response=self.post("/embeddings",&self.embedding_key,json!({"model":self.embedding_model,"dimensions":self.dimensions,"encoding_format":"float","input":inputs})).await?;
        let data = response["data"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Embedding response is invalid"))?;
        ensure!(data.len() == inputs.len(), "Embedding count is invalid");
        let mut vectors = vec![None; inputs.len()];
        for item in data {
            let index = item["index"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Embedding index is invalid"))?
                as usize;
            ensure!(
                index < vectors.len() && vectors[index].is_none(),
                "Embedding indices are invalid"
            );
            let values: Vec<f32> = serde_json::from_value(item["embedding"].clone())
                .map_err(|_| anyhow::anyhow!("Embedding values are invalid"))?;
            ensure!(
                values.len() == self.dimensions,
                "Embedding dimensions are invalid"
            );
            super::types::vector_text(&values)?;
            vectors[index] = Some(values);
        }
        vectors
            .into_iter()
            .map(|v| v.ok_or_else(|| anyhow::anyhow!("Embedding is missing")))
            .collect()
    }
}

// Schemars includes integer format annotations and a schema dialect marker;
// keep the supported structural schema used by strict Structured Outputs.
fn clean_schema(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("$schema");
            map.remove("format");
            for child in map.values_mut() {
                clean_schema(child);
            }
        }
        Value::Array(items) => {
            for child in items {
                clean_schema(child);
            }
        }
        _ => {}
    }
}
