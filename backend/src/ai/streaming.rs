//! Reviewer streaming through Rig. Every request owns its text and callback state.
use super::{structured::safe_error, transport::Transport};
use anyhow::{Result, ensure};
use futures_util::StreamExt;
use rig::{
    client::CompletionClient, completion::CompletionModel, providers::openai,
    streaming::StreamedAssistantContent,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::instrument::WithSubscriber;

pub(crate) async fn generate<T: DeserializeOwned, F, Fut>(
    base: &str,
    key: &str,
    body: Value,
    mut on_delta: F,
) -> Result<T>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let model = body["model"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("AI model is not configured"))?;
    let prompt = body["input"][0]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("AI request input is invalid"))?;
    let instructions = body["instructions"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("AI request instructions are invalid"))?;
    let client = openai::Client::builder(key)
        .base_url(base)
        .with_client(Transport::new(Duration::from_secs(90))?)
        .build();
    let model = client.completion_model(model);
    let started = Instant::now();
    let schema = body["text"]["format"]["name"]
        .as_str()
        .unwrap_or("review_reply");
    tracing::info!(
        event = "provider.started",
        operation = "ai.stream",
        schema,
        engine = "rig"
    );
    let work = async {
        let mut stream = model
            .completion_request(prompt.to_owned())
            .preamble(instructions.to_owned())
            .max_tokens(12000)
            .additional_params(serde_json::json!({"store":false,"text":body["text"]}))
            .stream()
            .await
            .map_err(safe_error)?;
        let mut text = String::new();
        let mut final_response = false;
        while let Some(item) = stream.next().await {
            match item.map_err(safe_error)? {
                StreamedAssistantContent::Text(part) => {
                    let delta = part.text;
                    ensure!(text.len() + delta.len() <= 1024 * 1024, "AI text too large");
                    text.push_str(&delta);
                    on_delta(delta).await?;
                }
                StreamedAssistantContent::Reasoning(_) => {}
                StreamedAssistantContent::Final(_) => final_response = true,
                _ => anyhow::bail!("AI tool calls are not allowed"),
            }
        }
        // The transport additionally requires a real response.completed event;
        // Rig 0.24's synthetic FinalResponse alone does not establish completion.
        ensure!(final_response, "AI stream ended without completion");
        serde_json::from_str(&text).map_err(|_| anyhow::anyhow!("AI stream did not match schema"))
    };
    let result = tokio::time::timeout(Duration::from_secs(90), work)
        .with_subscriber(tracing::subscriber::NoSubscriber::default())
        .await
        .map_err(|_| anyhow::anyhow!("AI streaming request timed out"))
        .and_then(|value| value);
    tracing::info!(
        event = "provider.completed",
        operation = "ai.stream",
        schema,
        engine = "rig",
        success = result.is_ok(),
        duration_ms = started.elapsed().as_millis() as u64
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::provider::AiProvider;
    use axum::{Json, Router, routing::post};
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Debug, Deserialize, JsonSchema)]
    struct Reply {
        answer: String,
    }

    async fn mock(
        mode: &'static str,
    ) -> (AiProvider, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new().route("/v1/responses", post(move |Json(body): Json<Value>| {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                assert_eq!(body["store"], false);
                assert_eq!(body["stream"], true);
                assert_eq!(body["text"]["format"]["strict"], true);
                assert_eq!(body["text"]["format"]["name"], "review_reply");
                assert!(body.get("tools").is_none_or(|v| v.as_array().is_some_and(Vec::is_empty)));
                if mode == "http" {
                    return (axum::http::StatusCode::TOO_MANY_REQUESTS, [("content-type", "application/json")], json!({"error":{"code":"rate_limit_exceeded","message":"private remote text"}}).to_string());
                }
                let input: Value = serde_json::from_str(body["input"][0]["content"][0]["text"].as_str().unwrap()).unwrap();
                let answer = input["marker"].as_str().unwrap_or("local");
                let reply = json!({"answer":answer}).to_string();
                let mut wire = String::new();
                for (index, character) in reply.chars().enumerate() {
                    wire.push_str(&format!("data: {}\n\n", json!({"type":"response.output_text.delta","item_id":"msg_mock","output_index":0,"content_index":0,"sequence_number":index,"delta":character.to_string()})));
                }
                let final_event = match mode {
                    "refusal" => json!({"type":"response.refusal.delta","delta":"refused"}),
                    "failed" => json!({"type":"response.failed"}),
                    "incomplete" => json!({"type":"response.completed","response":{"status":"incomplete"}}),
                    "tool" => json!({"type":"response.output_item.done","item":{"type":"function_call"}}),
                    _ => json!({"type":"response.completed","sequence_number":1000,"response":{"id":"resp_mock","object":"response","created_at":0,"model":"gpt-5-mini","status":"completed","output":[]}}),
                };
                if mode != "truncated" { wire.push_str(&format!("data: {final_event}\n\n")); }
                (axum::http::StatusCode::OK, [("content-type", "text/event-stream")], wire)
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (
            AiProvider::new(
                base,
                "dummy-local".into(),
                "dummy-embedding".into(),
                "gpt-5-mini".into(),
                "unused".into(),
            )
            .unwrap(),
            calls,
            task,
        )
    }

    #[tokio::test]
    async fn concurrent_streams_keep_inputs_and_deltas_separate() {
        let (provider, calls, server) = mock("ok").await;
        let mut first = String::new();
        let mut second = String::new();
        let (a, b) = tokio::join!(
            provider.structured_stream::<Reply, _, _>(
                "review_reply",
                "Session only",
                json!({"marker":"first"}),
                |delta| {
                    first.push_str(&delta);
                    std::future::ready(Ok(()))
                }
            ),
            provider.structured_stream::<Reply, _, _>(
                "review_reply",
                "Session only",
                json!({"marker":"second"}),
                |delta| {
                    second.push_str(&delta);
                    std::future::ready(Ok(()))
                }
            )
        );
        assert_eq!(a.unwrap().answer, "first");
        assert_eq!(b.unwrap().answer, "second");
        assert_eq!(
            serde_json::from_str::<Value>(&first).unwrap()["answer"],
            "first"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&second).unwrap()["answer"],
            "second"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn invalid_streams_and_http_errors_fail_without_retry_or_remote_text() {
        for mode in [
            "truncated",
            "refusal",
            "failed",
            "incomplete",
            "tool",
            "http",
        ] {
            let (provider, calls, server) = mock(mode).await;
            let error = provider
                .structured_stream::<Reply, _, _>(
                    "review_reply",
                    "Session only",
                    json!({}),
                    |_| async { Ok(()) },
                )
                .await
                .unwrap_err();
            assert!(!error.to_string().contains("private remote text"));
            assert_eq!(calls.load(Ordering::SeqCst), 1, "{mode}");
            server.abort();
        }
    }

    #[tokio::test]
    async fn disconnected_reviewer_cancels_stream() {
        let (provider, calls, server) = mock("ok").await;
        let result = provider
            .structured_stream::<Reply, _, _>(
                "review_reply",
                "Session only",
                json!({}),
                |_| async { anyhow::bail!("Reviewer disconnected") },
            )
            .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Reviewer disconnected")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        server.abort();
    }
}
