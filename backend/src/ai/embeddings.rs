//! Rig embeddings with stable input ordering and application vector validation.
use super::transport::Transport;
use crate::generation::{
    provider::{TransportFailure, provider_failure},
    types::vector_text,
};
use anyhow::{Result, ensure};
use rig::{
    client::EmbeddingsClient,
    embeddings::{EmbeddingError, EmbeddingModel},
    providers::openai,
};
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::instrument::WithSubscriber;

/// Keep indices trustworthy before Rig discards them during response conversion.
pub(super) fn ordered_response(bytes: &[u8], count: usize) -> Result<Vec<u8>> {
    let mut response: Value = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Embedding response is invalid"))?;
    let data = response["data"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Embedding response is invalid"))?;
    ensure!(data.len() == count, "Embedding count is invalid");
    let mut seen = vec![false; count];
    for item in data.iter() {
        let index = item["index"]
            .as_u64()
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(|| anyhow::anyhow!("Embedding index is invalid"))?;
        ensure!(
            index < count && !seen[index],
            "Embedding indices are invalid"
        );
        seen[index] = true;
    }
    data.sort_by_key(|item| item["index"].as_u64());
    Ok(serde_json::to_vec(&response)?)
}

fn safe_error(error: EmbeddingError) -> anyhow::Error {
    match error {
        EmbeddingError::HttpError(rig::http_client::Error::InvalidStatusCodeWithMessage(
            status,
            body,
        )) => {
            let body = serde_json::from_str(&body).unwrap_or(Value::Null);
            provider_failure(status.as_u16(), &body).into()
        }
        EmbeddingError::HttpError(_) => {
            TransportFailure("Embedding request or response failed").into()
        }
        _ => anyhow::anyhow!("Embedding response is invalid"),
    }
}

pub(crate) async fn generate(
    base: &str,
    key: &str,
    model: &str,
    dimensions: usize,
    inputs: &[String],
) -> Result<Vec<Vec<f32>>> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    ensure!(
        (1..=4096).contains(&dimensions),
        "Embedding dimensions are invalid"
    );
    ensure!(inputs.len() <= 1024, "Embedding batch is too large");
    let client = openai::Client::builder(key)
        .base_url(base)
        .with_client(Transport::new(Duration::from_secs(90))?)
        .build();
    let embedding_model = client.embedding_model_with_ndims(model, dimensions);
    let started = Instant::now();
    tracing::info!(
        event = "provider.started",
        operation = "ai.embeddings",
        engine = "rig",
        batch_size = inputs.len()
    );
    let result = embedding_model
        .embed_texts(inputs.iter().cloned())
        .with_subscriber(tracing::subscriber::NoSubscriber::default())
        .await;
    tracing::info!(
        event = "provider.completed",
        operation = "ai.embeddings",
        engine = "rig",
        success = result.is_ok(),
        duration_ms = started.elapsed().as_millis() as u64
    );
    let embeddings = result.map_err(safe_error)?;
    ensure!(
        embeddings.len() == inputs.len(),
        "Embedding count is invalid"
    );
    embeddings
        .into_iter()
        .map(|embedding| {
            ensure!(
                embedding.vec.len() == dimensions,
                "Embedding dimensions are invalid"
            );
            let vector: Vec<f32> = embedding
                .vec
                .into_iter()
                .map(|value| value as f32)
                .collect();
            vector_text(&vector)?;
            Ok(vector)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::provider::{AiProvider, ProviderFailure};
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn reply(data: Value) -> Value {
        json!({"object":"list","model":"text-embedding-3-small","usage":{"prompt_tokens":1,"total_tokens":1},"data":data})
    }
    fn item(index: usize, vector: Value) -> Value {
        json!({"object":"embedding","index":index,"embedding":vector})
    }
    async fn mock(
        status: StatusCode,
        reply: Value,
    ) -> (
        AiProvider,
        Arc<Mutex<Vec<Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let app = Router::new().route(
            "/v1/embeddings",
            post(move |headers: HeaderMap, Json(request): Json<Value>| {
                let captured = captured.clone();
                let reply = reply.clone();
                async move {
                    assert_eq!(headers["authorization"], "Bearer dummy-embedding-key");
                    captured.lock().unwrap().push(request);
                    (status, Json(reply))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut provider = AiProvider::new(
            base,
            "unused-llm-key".into(),
            "dummy-embedding-key".into(),
            "unused".into(),
            "text-embedding-3-small".into(),
        )
        .unwrap();
        provider.dimensions = 3;
        (provider, requests, server)
    }

    #[tokio::test]
    async fn rig_embeddings_preserve_order_settings_and_batching() {
        for count in [1, 32] {
            let data: Vec<Value> = (0..count)
                .rev()
                .map(|index| item(index, json!([index as f32 + 1.0, 0.5, 0.25])))
                .collect();
            let (provider, requests, server) = mock(StatusCode::OK, reply(json!(data))).await;
            let inputs: Vec<String> = (0..count)
                .map(|index| format!("Synthetic document {index}"))
                .collect();
            let vectors = provider.embeddings(&inputs).await.unwrap();
            server.abort();
            for (index, vector) in vectors.iter().enumerate() {
                assert_eq!(vector, &vec![index as f32 + 1.0, 0.5, 0.25]);
            }
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["model"], "text-embedding-3-small");
            assert_eq!(requests[0]["dimensions"], 3);
            assert_eq!(requests[0]["encoding_format"], "float");
            assert_eq!(requests[0]["input"], json!(inputs));
        }
    }

    #[tokio::test]
    async fn rig_embeddings_reject_invalid_indices_counts_and_vectors() {
        for data in [
            json!([]),
            json!([item(0, json!([1, 0, 0])), item(0, json!([0, 1, 0]))]),
            json!([item(2, json!([1, 0, 0])), item(1, json!([0, 1, 0]))]),
            json!([item(0, json!([1, 0])), item(1, json!([0, 1, 0]))]),
            json!([item(0, json!([0, 0, 0])), item(1, json!([0, 1, 0]))]),
            json!([item(0, json!([1e100, 0, 0])), item(1, json!([0, 1, 0]))]),
            json!([item(0, json!(["invalid", 0, 0])), item(1, json!([0, 1, 0]))]),
            json!([{"object":"embedding","embedding":[1,0,0]},item(1,json!([0,1,0]))]),
        ] {
            let (provider, requests, server) = mock(StatusCode::OK, reply(data)).await;
            assert!(
                provider
                    .embeddings(&["First".into(), "Second".into()])
                    .await
                    .is_err()
            );
            server.abort();
            assert_eq!(requests.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn rig_embeddings_keep_retry_classification_without_extra_calls() {
        for (status, code, retryable) in [
            (StatusCode::TOO_MANY_REQUESTS, "rate_limit_exceeded", true),
            (StatusCode::TOO_MANY_REQUESTS, "insufficient_quota", false),
            (StatusCode::UNAUTHORIZED, "invalid_api_key", false),
            (StatusCode::INTERNAL_SERVER_ERROR, "unknown", true),
        ] {
            let (provider, requests, server) = mock(
                status,
                json!({"error":{"code":code,"message":"private provider detail"}}),
            )
            .await;
            let error = provider
                .embeddings(&["Synthetic document".into()])
                .await
                .unwrap_err();
            server.abort();
            assert_eq!(
                error.downcast_ref::<ProviderFailure>().unwrap().retryable,
                retryable
            );
            assert!(!error.to_string().contains("private provider detail"));
            assert_eq!(requests.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn rig_embeddings_empty_or_invalid_requests_make_no_calls() {
        let (mut provider, requests, server) = mock(StatusCode::OK, reply(json!([]))).await;
        assert!(provider.embeddings(&[]).await.unwrap().is_empty());
        provider.dimensions = 0;
        assert!(provider.embeddings(&["Synthetic".into()]).await.is_err());
        provider.dimensions = 3;
        assert!(
            provider
                .embeddings(&vec!["Synthetic".into(); 1025])
                .await
                .is_err()
        );
        server.abort();
        assert!(requests.lock().unwrap().is_empty());
    }
}
