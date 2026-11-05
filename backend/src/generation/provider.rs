use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug)]
pub struct ProviderFailure {
    pub retryable: bool,
    status: u16,
    code: &'static str,
}
impl std::fmt::Display for ProviderFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AI service returned HTTP {} ({})",
            self.status, self.code
        )
    }
}
impl std::error::Error for ProviderFailure {}
fn provider_failure(status: u16, body: &Value) -> ProviderFailure {
    // Whitelist operational error codes; never echo arbitrary remote text or API keys.
    let code = match body["error"]["code"]
        .as_str()
        .or(body["error"]["type"].as_str())
        .unwrap_or("")
    {
        "invalid_json_schema" => "invalid_json_schema",
        "invalid_api_key" => "invalid_api_key",
        "insufficient_quota" => "insufficient_quota",
        "rate_limit_exceeded" => "rate_limit_exceeded",
        "unsupported_parameter" => "unsupported_parameter",
        "model_not_found" => "model_not_found",
        "context_length_exceeded" => "context_length_exceeded",
        _ => "provider_error",
    };
    ProviderFailure {
        status,
        code,
        retryable: (status == 408 || status == 429 || status >= 500)
            && code != "insufficient_quota",
    }
}
fn generation_schema_bounds(schema: &mut Value, name: &str, input: &Value) {
    let (property, count) = match name {
        "interview_blueprint" => (
            "slots",
            input["question_count"].as_u64().map(|n| n as usize),
        ),
        "interview_questions" => (
            "questions",
            input["blueprint"]["slots"].as_array().map(Vec::len),
        ),
        "interview_quality_review" => (
            "questions",
            input["proposed_set"]["questions"].as_array().map(Vec::len),
        ),
        _ => return,
    };
    if let Some(count) = count.filter(|count| (1..=20).contains(count))
        && let Some(array) = schema["properties"][property].as_object_mut()
    {
        array.insert("minItems".into(), json!(count));
        array.insert("maxItems".into(), json!(count));
    }
}
// Serialize request schemas in authoring order without changing serde_json's
// canonical map order used by persisted audit signatures elsewhere in the app.
struct OrderedPayload<'a> {
    value: &'a Value,
    properties: bool,
}
impl serde::Serialize for OrderedPayload<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self.value {
            Value::Object(map) => {
                let order: &[&str] = if self.properties
                    && map.contains_key("prompt")
                    && map.contains_key("expected_answer")
                {
                    &[
                        "competency_id",
                        "kind",
                        "difficulty",
                        "estimated_minutes",
                        "prompt",
                        "expected_answer",
                        "assumptions",
                        "acceptable_alternatives",
                        "common_mistakes",
                        "coding",
                        "criteria",
                    ]
                } else if self.properties && map.contains_key("max_points") {
                    &[
                        "id",
                        "description",
                        "max_points",
                        "reference_answer",
                        "partial_credit",
                        "full_credit",
                    ]
                } else if self.properties && map.contains_key("solution") {
                    &["language", "solution", "complexity", "tests"]
                } else {
                    &[]
                };
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort_by_key(|key| {
                    order
                        .iter()
                        .position(|item| item == key)
                        .unwrap_or(order.len())
                });
                let mut output = serializer.serialize_map(Some(map.len()))?;
                for key in keys {
                    output.serialize_entry(
                        key,
                        &OrderedPayload {
                            value: &map[key],
                            properties: key == "properties",
                        },
                    )?;
                }
                output.end()
            }
            Value::Array(values) => {
                let mut output = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    output.serialize_element(&OrderedPayload {
                        value,
                        properties: false,
                    })?;
                }
                output.end()
            }
            _ => self.value.serialize(serializer),
        }
    }
}
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
        let operation = if path == "/embeddings" {
            "ai.embeddings"
        } else {
            "ai.generate"
        };
        let started = std::time::Instant::now();
        tracing::info!(event = "provider.started", operation);
        let result = self.post_inner(path, key, body).await;
        tracing::info!(
            event = "provider.completed",
            operation,
            success = result.is_ok(),
            duration_ms = started.elapsed().as_millis() as u64
        );
        result
    }
    async fn post_inner(&self, path: &str, key: &str, body: Value) -> Result<Value> {
        let encoded = serde_json::to_vec(&OrderedPayload {
            value: &body,
            properties: false,
        })?;
        let mut request = self
            .client
            .post(format!("{}{path}", self.base))
            .bearer_auth(key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(encoded);
        if matches!(
            body["text"]["format"]["name"].as_str(),
            Some("interview_blueprint" | "interview_questions" | "interview_quality_review")
        ) {
            let seconds = std::env::var("GENERATION_REQUEST_TIMEOUT_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(180)
                .clamp(30, 300);
            request = request.timeout(Duration::from_secs(seconds));
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("AI service connection failed or timed out"))?;
        let status = response.status();
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
        let parsed = serde_json::from_slice::<Value>(&bytes);
        if !status.is_success() {
            return Err(
                provider_failure(status.as_u16(), parsed.as_ref().unwrap_or(&Value::Null)).into(),
            );
        }
        parsed.map_err(|_| anyhow::anyhow!("AI service returned invalid JSON"))
    }
    pub async fn structured<T: JsonSchema + DeserializeOwned>(
        &self,
        name: &str,
        instructions: &str,
        input: Value,
    ) -> Result<T> {
        let mut schema = serde_json::to_value(schemars::schema_for!(T))?;
        clean_schema(&mut schema);
        generation_schema_bounds(&mut schema, name, &input);
        let mut body = json!({
            "model":self.model,"store":false,"max_output_tokens":if name=="interview_questions"{20000}else{12000},
            "instructions":instructions,"input":[{"role":"user","content":serde_json::to_string(&input)?}],
            "text":{"format":{"type":"json_schema","name":name,"strict":true,"schema":schema}}
        });
        if self.model == "gpt-5-mini" {
            if matches!(name, "interview_blueprint" | "interview_questions") {
                let effort = std::env::var("GENERATION_DRAFT_REASONING_EFFORT")
                    .ok()
                    .filter(|value| matches!(value.as_str(), "minimal" | "low" | "medium" | "high"))
                    .unwrap_or_else(|| "low".into());
                body["reasoning"] = json!({"effort":effort});
            } else if name == "interview_quality_review" {
                body["reasoning"] = json!({"effort":"medium"});
            }
        }
        let response = self.post("/responses", &self.llm_key, body).await?;
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
    pub async fn structured_stream<T: JsonSchema + DeserializeOwned, F, Fut>(
        &self,
        name: &str,
        instructions: &str,
        input: Value,
        mut on_delta: F,
    ) -> Result<T>
    where
        F: FnMut(String) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let mut schema = serde_json::to_value(schemars::schema_for!(T))?;
        clean_schema(&mut schema);
        let mut response=self.client.post(format!("{}/responses",self.base)).bearer_auth(&self.llm_key).json(&json!({"model":self.model,"store":false,"stream":true,"max_output_tokens":12000,"instructions":instructions,"input":[{"role":"user","content":input.to_string()}],"text":{"format":{"type":"json_schema","name":name,"strict":true,"schema":schema}}})).send().await.map_err(|_|anyhow::anyhow!("AI streaming connection failed"))?;
        ensure!(
            response.status().is_success(),
            "AI streaming request failed"
        );
        let mut buffer = Vec::new();
        let mut text = String::new();
        let mut completed = false;
        let mut bytes = 0;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("AI stream interrupted"))?
        {
            bytes += chunk.len();
            ensure!(bytes <= 2 * 1024 * 1024, "AI stream exceeded size limit");
            buffer.extend_from_slice(&chunk);
            loop {
                let boundary = buffer
                    .windows(2)
                    .position(|v| v == b"\n\n")
                    .map(|i| (i, 2))
                    .or_else(|| {
                        buffer
                            .windows(4)
                            .position(|v| v == b"\r\n\r\n")
                            .map(|i| (i, 4))
                    });
                let Some((end, delimiter)) = boundary else {
                    break;
                };
                let frame = String::from_utf8(buffer.drain(..end + delimiter).collect())?;
                let data = frame
                    .lines()
                    .filter_map(|l| l.strip_prefix("data:"))
                    .map(str::trim_start)
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let event: Value = serde_json::from_str(&data)?;
                match event["type"].as_str().unwrap_or("") {
                    "response.output_text.delta" => {
                        let delta = event["delta"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("Invalid delta"))?;
                        ensure!(text.len() + delta.len() <= 1024 * 1024, "AI text too large");
                        text.push_str(delta);
                        on_delta(delta.to_string()).await?;
                    }
                    "response.completed" => {
                        ensure!(
                            event["response"]["status"] == "completed",
                            "AI response incomplete"
                        );
                        completed = true;
                    }
                    "response.failed"
                    | "response.incomplete"
                    | "error"
                    | "response.refusal.delta" => anyhow::bail!("AI stream failed or refused"),
                    _ => {}
                }
            }
        }
        ensure!(completed, "AI stream ended without completion");
        serde_json::from_str(&text).map_err(|_| anyhow::anyhow!("AI stream did not match schema"))
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
            map.remove("default");
            // Strict output schemas require every property, including nullable
            // Option<T> fields that Schemars otherwise considers omittable.
            if let Some(properties) = map.get("properties").and_then(Value::as_object) {
                let required = properties
                    .keys()
                    .map(|key| Value::String(key.clone()))
                    .collect();
                map.insert("required".into(), Value::Array(required));
                map.insert("additionalProperties".into(), Value::Bool(false));
            }

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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_nullable_properties_are_required() {
        let mut schema =
            serde_json::to_value(schemars::schema_for!(crate::generation::types::QuestionSet))
                .unwrap();
        clean_schema(&mut schema);
        let question = &schema["$defs"]["Question"];
        assert!(
            question["required"]
                .as_array()
                .unwrap()
                .contains(&json!("coding"))
        );
        assert!(
            question["properties"]["coding"]
                .to_string()
                .contains("null")
        );
        assert_eq!(question["additionalProperties"], false);
    }
    #[test]
    fn provider_errors_do_not_echo_remote_payloads_and_classify_retries() {
        let fatal = provider_failure(
            400,
            &json!({"error":{"code":"invalid_json_schema","message":"private remote payload"}}),
        );
        assert!(!fatal.retryable);
        assert!(!fatal.to_string().contains("private"));
        assert!(provider_failure(429, &json!({"error":{"code":"rate_limit_exceeded"}})).retryable);
        assert!(!provider_failure(429, &json!({"error":{"code":"insufficient_quota"}})).retryable);
        assert!(!provider_failure(401, &Value::Null).retryable);
    }
    #[test]
    fn generation_schema_authors_the_prompt_before_answer_and_rubric() {
        let value =
            json!({"properties":{"criteria":{},"expected_answer":{},"prompt":{},"kind":{}}});
        let encoded = serde_json::to_string(&OrderedPayload {
            value: &value,
            properties: false,
        })
        .unwrap();
        assert!(encoded.find("\"prompt\"").unwrap() < encoded.find("\"expected_answer\"").unwrap());
        assert!(
            encoded.find("\"expected_answer\"").unwrap() < encoded.find("\"criteria\"").unwrap()
        );
        assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), value);
    }
    #[test]
    fn exact_generation_counts_are_enforced_in_the_output_schema() {
        let mut schema =
            serde_json::to_value(schemars::schema_for!(crate::generation::types::QuestionSet))
                .unwrap();
        clean_schema(&mut schema);
        generation_schema_bounds(
            &mut schema,
            "interview_questions",
            &json!({"blueprint":{"slots":[{},{},{},{},{},{}]}}),
        );
        assert_eq!(schema["properties"]["questions"]["minItems"], 6);
        assert_eq!(schema["properties"]["questions"]["maxItems"], 6);
    }
}
