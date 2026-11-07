use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

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
pub(crate) fn provider_failure(status: u16, body: &Value) -> ProviderFailure {
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
#[derive(Debug)]
pub struct TransportFailure(pub(crate) &'static str);
impl std::fmt::Display for TransportFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for TransportFailure {}
fn generation_schema_bounds(schema: &mut Value, name: &str, input: &Value) {
    if name == "interview_reference_answer" {
        // The approved prompt fixes the answer type before reference generation.
        // Enforce it in the schema as well as in the final domain validation.
        match input["approved_question"]["kind"].as_str() {
            Some("text" | "scenario") => {
                schema["properties"]["coding"] = json!({"type":"null"});
            }
            Some("code") => {
                schema["properties"]["coding"] = json!({"$ref":"#/$defs/CodeReference"});
            }
            _ => {}
        }
        return;
    }
    if name == "interview_questions" {
        // Sequential generation always has one slot; bind nullable coding to it.
        if let Some(slots) = input["blueprint"]["slots"].as_array()
            && slots.len() == 1
        {
            if let Some(minutes) = input["duration_minutes"]
                .as_u64()
                .filter(|n| (1..=30).contains(n))
            {
                schema["$defs"]["Question"]["properties"]["estimated_minutes"] =
                    json!({"type":"integer","minimum":minutes,"maximum":minutes});
            }
            match slots[0]["kind"].as_str() {
                Some("text" | "scenario") => {
                    schema["$defs"]["Question"]["properties"]["coding"] = json!({"type":"null"});
                }
                Some("code") => {
                    schema["$defs"]["Question"]["properties"]["coding"] =
                        json!({"$ref":"#/$defs/CodeReference"});
                }
                _ => {}
            }
        }
    }
    let (property, count) = match name {
        "interview_blueprint" => (
            "slots",
            input["question_count"].as_u64().map(|n| n as usize),
        ),
        "interview_questions" | "interview_question_prompts" => (
            "questions",
            input["blueprint"]["slots"].as_array().map(Vec::len),
        ),
        "interview_prompt_review" => (
            "questions",
            input["proposed_prompts"]["questions"]
                .as_array()
                .map(Vec::len),
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
pub(crate) fn ordered_payload(value: &Value) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&OrderedPayload {
        value,
        properties: false,
    })?)
}
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
    /// Extract a typed blueprint through the shared Rig adapter.
    pub(crate) async fn blueprint(
        &self,
        instructions: &str,
        input: Value,
    ) -> Result<super::types::Blueprint> {
        self.structured("interview_blueprint", instructions, input)
            .await
    }

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
            "model":self.model,"store":false,"max_output_tokens":match name {"interview_question_prompts" | "interview_prompt_review" | "interview_quality_review" | "interview_reference_review" => 4000,"interview_reference_answer" | "interview_questions" => 8000,_ => 12000},
            "instructions":instructions,"input":[{"role":"user","content":serde_json::to_string(&input)?}],
            "text":{"format":{"type":"json_schema","name":name,"strict":true,"schema":schema}}
        });
        if self.model == "gpt-5-mini" {
            if matches!(
                name,
                "interview_blueprint"
                    | "interview_questions"
                    | "interview_question_prompts"
                    | "interview_prompt_review"
                    | "interview_reference_answer"
            ) {
                let effort = std::env::var("GENERATION_DRAFT_REASONING_EFFORT")
                    .ok()
                    .filter(|value| matches!(value.as_str(), "minimal" | "low" | "medium" | "high"))
                    .unwrap_or_else(|| "low".into());
                body["reasoning"] = json!({"effort":effort});
            } else if matches!(
                name,
                "interview_quality_review" | "interview_reference_review"
            ) {
                body["reasoning"] = json!({"effort":"medium"});
            }
        }
        crate::ai::structured::generate(&self.base, &self.llm_key, body).await
    }
    pub async fn structured_stream<T: JsonSchema + DeserializeOwned, F, Fut>(
        &self,
        name: &str,
        instructions: &str,
        input: Value,
        on_delta: F,
    ) -> Result<T>
    where
        F: FnMut(String) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let mut schema = serde_json::to_value(schemars::schema_for!(T))?;
        clean_schema(&mut schema);
        crate::ai::streaming::generate(
            &self.base, &self.llm_key,
            json!({"model":self.model,"store":false,"max_output_tokens":12000,
                "instructions":instructions,"input":[{"role":"user","content":input.to_string()}],
                "text":{"format":{"type":"json_schema","name":name,"strict":true,"schema":schema}}}),
            on_delta,
        ).await
    }
    pub async fn embeddings(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        crate::ai::embeddings::generate(
            &self.base,
            &self.embedding_key,
            &self.embedding_model,
            self.dimensions,
            inputs,
        )
        .await
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
    fn reference_schema_matches_the_approved_question_kind() {
        for kind in ["text", "scenario", "code"] {
            let mut schema = serde_json::to_value(schemars::schema_for!(
                crate::generation::types::ReferenceAnswer
            ))
            .unwrap();
            clean_schema(&mut schema);
            generation_schema_bounds(
                &mut schema,
                "interview_reference_answer",
                &json!({"approved_question":{"kind":kind}}),
            );
            let coding = &schema["properties"]["coding"];
            if kind == "code" {
                assert_eq!(coding["$ref"], "#/$defs/CodeReference");
                assert!(schema["$defs"]["CodeReference"].is_object());
            } else {
                assert_eq!(coding, &json!({"type":"null"}));
            }
            assert!(
                schema["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("coding"))
            );
        }
    }
    #[test]
    fn combined_question_schema_enforces_coding_for_the_slot() {
        for kind in ["text", "scenario", "code"] {
            let mut schema =
                serde_json::to_value(schemars::schema_for!(crate::generation::types::QuestionSet))
                    .unwrap();
            clean_schema(&mut schema);
            generation_schema_bounds(
                &mut schema,
                "interview_questions",
                &json!({"blueprint":{"slots":[{"kind":kind}]}}),
            );
            let coding = &schema["$defs"]["Question"]["properties"]["coding"];
            if kind == "code" {
                assert_eq!(coding["$ref"], "#/$defs/CodeReference");
                assert!(schema["$defs"]["CodeReference"].is_object());
            } else {
                assert_eq!(coding, &json!({"type":"null"}));
            }
            assert_eq!(schema["properties"]["questions"]["maxItems"], 1);
        }
    }
    #[test]
    fn question_time_schema_uses_the_current_slot_budget() {
        for budget in [1, 2, 5, 30] {
            let mut schema =
                serde_json::to_value(schemars::schema_for!(crate::generation::types::QuestionSet))
                    .unwrap();
            clean_schema(&mut schema);
            generation_schema_bounds(
                &mut schema,
                "interview_questions",
                &json!({"blueprint":{"slots":[{"kind":"text"}]},"duration_minutes":budget}),
            );
            assert_eq!(
                schema["$defs"]["Question"]["properties"]["estimated_minutes"],
                json!({"type":"integer","minimum":budget,"maximum":budget})
            );
        }
    }
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
