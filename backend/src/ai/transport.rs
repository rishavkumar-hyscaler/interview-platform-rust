//! Bounded, non-retrying HTTP requests for Rig generation and embeddings.
use bytes::Bytes;
use rig::http_client::{self, HttpClientExt, LazyBody, Request, Response};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub(super) struct Transport(reqwest::Client);

impl Transport {
    pub fn new(timeout: Duration) -> anyhow::Result<Self> {
        Ok(Self(
            reqwest::Client::builder()
                .timeout(timeout)
                .connect_timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        ))
    }
}

impl HttpClientExt for Transport {
    fn send<T, U>(
        &self,
        request: Request<T>,
    ) -> impl Future<Output = http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        T: Into<Bytes> + Send,
        U: From<Bytes> + Send + 'static,
    {
        let (parts, body) = request.into_parts();
        let body = body.into();
        let mut payload = serde_json::from_slice::<serde_json::Value>(&body).ok();
        let embedding_count = if parts.uri.path().ends_with("/embeddings") {
            payload.as_mut().and_then(|value| {
                value["encoding_format"] = serde_json::json!("float");
                value["input"].as_array().map(Vec::len)
            })
        } else {
            None
        };
        let ordered = payload
            .as_ref()
            .and_then(|value| crate::generation::provider::ordered_payload(value).ok());
        let body = ordered.map(Bytes::from).unwrap_or(body);
        let request = self
            .0
            .request(parts.method, parts.uri.to_string())
            .headers(parts.headers)
            .body(body);
        async move {
            let mut response = request
                .send()
                .await
                .map_err(|error| http_client::Error::Instance(Box::new(error)))?;
            let status = response.status();
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| http_client::Error::Instance(Box::new(error)))?
            {
                if bytes.len() + chunk.len() > 1024 * 1024 {
                    return Err(http_client::Error::Instance(
                        "AI service response exceeded size limit".into(),
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            if !status.is_success() {
                return Err(http_client::Error::InvalidStatusCodeWithMessage(
                    status,
                    String::from_utf8_lossy(&bytes).into_owned(),
                ));
            }
            // Rig 0.24 zips embeddings in response order rather than by index.
            // Validate and reorder first to keep every vector paired with its text.
            if let Some(count) = embedding_count {
                bytes = super::embeddings::ordered_response(&bytes, count)
                    .map_err(|error| http_client::Error::Instance(error.into()))?;
            }
            let body: LazyBody<U> = Box::pin(async move { Ok(U::from(Bytes::from(bytes))) });
            Response::builder()
                .status(status)
                .body(body)
                .map_err(http_client::Error::Protocol)
        }
    }

    fn send_multipart<U>(
        &self,
        request: Request<reqwest::multipart::Form>,
    ) -> impl Future<Output = http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        U: From<Bytes> + Send + 'static,
    {
        self.0.send_multipart(request)
    }

    fn send_streaming<T>(
        &self,
        request: Request<T>,
    ) -> impl Future<
        Output = http_client::Result<http_client::StreamingResponse<http_client::sse::BoxedStream>>,
    > + Send
    where
        T: Into<Bytes>,
    {
        let (parts, body) = request.into_parts();
        let body = body.into();
        let payload = serde_json::from_slice::<serde_json::Value>(&body)
            .map_err(|_| http_client::Error::Instance("AI request is invalid".into()));
        let client = self.0.clone();
        async move {
            let body = crate::generation::provider::ordered_payload(&payload?)
                .map_err(|_| http_client::Error::Instance("AI request is invalid".into()))?;
            let mut response = client
                .request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .body(body)
                .send()
                .await
                .map_err(|_| {
                    http_client::Error::Instance("AI streaming connection failed".into())
                })?;
            let status = response.status();
            if !status.is_success() {
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(|_| {
                    http_client::Error::Instance("AI error response interrupted".into())
                })? {
                    if bytes.len() + chunk.len() > 1024 * 1024 {
                        return Err(http_client::Error::Instance(
                            "AI error response exceeded size limit".into(),
                        ));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                return Err(http_client::Error::InvalidStatusCodeWithMessage(
                    status,
                    String::from_utf8_lossy(&bytes).into_owned(),
                ));
            }
            let headers = response.headers().clone();
            let stream = async_stream::try_stream! {
                let mut guard = StreamGuard::default();
                while let Some(chunk) = response.chunk().await
                    .map_err(|_| http_client::Error::Instance("AI stream interrupted".into()))? {
                    for frame in guard.push(&chunk).map_err(|_| http_client::Error::Instance("AI stream invalid, incomplete or refused".into()))? {
                        yield Bytes::from(frame);
                    }
                    if guard.completed { break; }
                }
                if !guard.completed {
                    Err(http_client::Error::Instance("AI stream ended without completion".into()))?;
                }
            };
            let mut result = Response::builder().status(status);
            *result.headers_mut().expect("valid response builder") = headers;
            result
                .body(Box::pin(stream) as http_client::sse::BoxedStream)
                .map_err(http_client::Error::Protocol)
        }
    }
}

/// Validate raw SSE before Rig parses it: 0.24 otherwise treats refusals as text
/// and emits a final result even when the provider never completed successfully.
#[derive(Default)]
struct StreamGuard {
    buffer: Vec<u8>,
    bytes: usize,
    text_bytes: usize,
    completed: bool,
}
impl StreamGuard {
    fn push(&mut self, chunk: &[u8]) -> anyhow::Result<Vec<Vec<u8>>> {
        use anyhow::ensure;
        self.bytes += chunk.len();
        ensure!(
            self.bytes <= 2 * 1024 * 1024,
            "AI stream exceeded size limit"
        );
        self.buffer.extend_from_slice(chunk);
        let mut frames = Vec::new();
        loop {
            let boundary = self
                .buffer
                .windows(2)
                .position(|v| v == b"\n\n")
                .map(|i| (i, 2))
                .into_iter()
                .chain(
                    self.buffer
                        .windows(4)
                        .position(|v| v == b"\r\n\r\n")
                        .map(|i| (i, 4)),
                )
                .min_by_key(|(i, _)| *i);
            let Some((end, delimiter)) = boundary else {
                break;
            };
            let frame: Vec<u8> = self.buffer.drain(..end + delimiter).collect();
            let decoded = std::str::from_utf8(&frame)?;
            let data = decoded
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let event: serde_json::Value = serde_json::from_str(&data)?;
            let kind = event["type"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing event type"))?;
            ensure!(
                !matches!(kind, "response.failed" | "response.incomplete" | "error")
                    && !kind.contains("refusal")
                    && !kind.contains("function_call"),
                "AI stream failed or refused"
            );
            if kind == "response.output_text.delta" {
                let delta = event["delta"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid delta"))?;
                self.text_bytes += delta.len();
                ensure!(self.text_bytes <= 1024 * 1024, "AI text too large");
            }
            if matches!(
                kind,
                "response.output_item.added" | "response.output_item.done"
            ) {
                validate_output(&event["item"])?;
            }
            if kind == "response.completed" {
                ensure!(
                    event["response"]["status"] == "completed",
                    "AI response incomplete"
                );
                if let Some(output) = event["response"]["output"].as_array() {
                    for item in output {
                        validate_output(item)?;
                    }
                }
                self.completed = true;
            }
            frames.push(frame);
            if self.completed {
                break;
            }
        }
        Ok(frames)
    }
}
fn validate_output(item: &serde_json::Value) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(item["type"].as_str(), Some("message" | "reasoning")),
        "Unexpected AI output"
    );
    if let Some(content) = item["content"].as_array() {
        for part in content {
            anyhow::ensure!(part["type"] == "output_text", "AI stream refused");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::StreamGuard;
    #[test]
    fn sse_guard_handles_split_frames_and_enforces_bounds() {
        let wire = b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\r\n\r\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\r\n\r\n";
        let mut guard = StreamGuard::default();
        let mut frames = 0;
        for byte in wire {
            frames += guard.push(&[*byte]).unwrap().len();
        }
        assert_eq!(frames, 2);
        assert!(guard.completed);
        assert!(StreamGuard::default().push(b"data: not-json\n\n").is_err());
        assert!(
            StreamGuard::default()
                .push(&vec![b'x'; 2 * 1024 * 1024 + 1])
                .is_err()
        );
        let mut guard = StreamGuard {
            text_bytes: 1024 * 1024,
            ..Default::default()
        };
        assert!(
            guard
                .push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n")
                .is_err()
        );
    }
}
