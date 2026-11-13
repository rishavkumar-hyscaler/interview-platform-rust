//! Development API reference. Payload schemas come from the HTTP DTOs.
use crate::dto::*;
use axum::{Json, Router, response::Html, routing::get};
use schemars::{JsonSchema, generate::SchemaSettings};
use serde_json::{Value, json};
use sqlx::PgPool;

pub(crate) fn routes() -> Router<PgPool> {
    Router::new()
        .route(
            "/docs",
            get(|| async { Html(include_str!("swagger.html")) }),
        )
        .route("/openapi.json", get(|| async { Json(document()) }))
}

fn request_schema<T: JsonSchema>() -> Value {
    schema::<T>(SchemaSettings::draft2020_12())
}

fn response_schema<T: JsonSchema>() -> Value {
    schema::<T>(SchemaSettings::draft2020_12().for_serialize())
}

fn schema<T: JsonSchema>(settings: SchemaSettings) -> Value {
    let generator = settings
        .with(|settings| settings.inline_subschemas = true)
        .into_generator();
    json!(generator.into_root_schema_for::<T>())
}

fn parameter(name: &str, location: &str, required: bool, schema: Value) -> Value {
    json!({"name": name, "in": location, "required": required, "schema": schema})
}

fn content(types: &[&str], schema: Value) -> Value {
    Value::Object(
        types
            .iter()
            .map(|kind| (kind.to_string(), json!({"schema": schema})))
            .collect(),
    )
}

const MEDIA: &[&str] = &[
    "audio/webm",
    "video/webm",
    "audio/ogg",
    "audio/mp4",
    "video/mp4",
];
const EVIDENCE: &[&str] = &[
    "image/jpeg",
    "audio/wav",
    "audio/webm",
    "video/webm",
    "audio/ogg",
    "audio/mp4",
    "video/mp4",
];

fn document() -> Value {
    let mut paths = json!({});
    macro_rules! endpoint {
        ($method:literal $path:literal, $tag:literal, $auth:expr, $summary:literal, $status:literal => $response:expr $(, $key:literal => $value:expr)* $(,)?) => {{
            let mut operation = json!({
                "summary": $summary, "tags": [$tag], "security": $auth,
                "responses": {$status: {"description": "Success"}, "default": {"$ref": "#/components/responses/Error"}},
                "parameters": [],
            });
            if let Some(content) = $response {
                operation["responses"][$status]["content"] = content;
            }
            $(operation[$key] = $value;)*
            let parameters = operation["parameters"].as_array_mut().unwrap();
            for segment in $path.split('/') {
                if let Some(name) = segment.strip_prefix('{').and_then(|part| part.strip_suffix('}')) {
                    let schema = match name {
                        "action" => json!({"type": "string", "enum": ["start", "submit"]}),
                        _ => json!({"type": "string", "format": "uuid"}),
                    };
                    parameters.push(parameter(name, "path", true, schema));
                }
            }
            if parameters.is_empty() {
                operation.as_object_mut().unwrap().remove("parameters");
            }
            paths[$path][$method] = operation;
        }};
    }

    let public = json!([]);
    let staff = json!([{"InterviewerToken": []}]);
    let candidate = json!([{"InvitationToken": []}]);
    let json_body =
        |schema| json!({"required": true, "content": content(&["application/json"], schema)});
    let json_response = |schema| Some(content(&["application/json"], schema));
    let binary = json!({"type": "string", "format": "binary"});
    let media_body = |types| json!({"required": true, "content": content(types, binary.clone())});
    let stream = || Some(content(&["text/event-stream"], json!({"type": "string"})));
    let pagination = || {
        vec![
            parameter(
                "cursor",
                "query",
                false,
                json!({"type": "string", "format": "uuid", "description": "Use next_cursor from the previous page."}),
            ),
            parameter(
                "limit",
                "query",
                false,
                json!({"type": "integer", "minimum": 1, "maximum": 100, "default": 20}),
            ),
        ]
    };
    endpoint!("get" "/health/live", "Health", public, "Check process liveness", "200" => json_response(json!({"type": "object", "required": ["status"], "properties": {"status": {"const": "ok"}}})));
    endpoint!("get" "/health/ready", "Health", public, "Check database readiness", "200" => json_response(json!({"type": "object", "required": ["status"], "properties": {"status": {"const": "ready"}}})));
    endpoint!("post" "/api/auth/register", "Auth", public, "Register an interviewer", "201" => json_response(response_schema::<auth::SessionResponse>()),
        "requestBody" => json_body(request_schema::<auth::RegisterRequest>()));
    endpoint!("post" "/api/auth/login", "Auth", public, "Sign in as an interviewer", "200" => json_response(response_schema::<auth::SessionResponse>()),
        "requestBody" => json_body(request_schema::<auth::LoginRequest>()));
    endpoint!("post" "/api/auth/logout", "Auth", staff, "Revoke the current session token", "204" => None);
    endpoint!("get" "/api/me", "Auth", staff, "Get the signed-in interviewer", "200" => json_response(response_schema::<auth::UserResponse>()));
    endpoint!("get" "/api/v1/summary", "Jobs", staff, "Get dashboard totals", "200" => json_response(response_schema::<interview::SummaryResponse>()));
    endpoint!("get" "/api/v1/jobs", "Jobs", staff, "List jobs", "200" => json_response(response_schema::<job::JobListResponse>()),
        "parameters" => json!(pagination()));
    if let Some(parameters) = paths["/api/v1/jobs"]["get"]["parameters"].as_array_mut() {
        parameters.extend([
            json!({"name":"search","in":"query","schema":{"type":"string","maxLength":160},"description":"Search job ID or title"}),
            json!({"name":"invitations","in":"query","schema":{"type":"string","enum":["all","with","without"],"default":"all"}}),
            json!({"name":"sort","in":"query","schema":{"type":"string","enum":["newest","oldest"],"default":"newest"}}),
        ]);
    }
    endpoint!("post" "/api/v1/jobs", "Jobs", staff, "Create a job", "201" => json_response(response_schema::<job::JobResponse>()),
        "requestBody" => json_body(request_schema::<job::CreateJobRequest>()));
    paths["/api/v1/jobs"]["post"]["responses"]["200"] =
        paths["/api/v1/jobs"]["post"]["responses"]["201"].clone();
    endpoint!("delete" "/api/v1/jobs/{job}", "Jobs", staff, "Permanently delete a job and its related data", "204" => None);
    endpoint!("post" "/api/v1/jobs/{job}/blueprint", "Generation", staff, "Retry background job blueprint preparation", "202" => json_response(response_schema::<generation::PendingResponse>()));
    endpoint!("post" "/api/v1/jobs/{job}/invitations", "Invitations", staff, "Invite a candidate", "201" => json_response(response_schema::<invitation::InvitationResponse>()),
        "requestBody" => json_body(request_schema::<invitation::InvitationRequest>()));
    paths["/api/v1/jobs/{job}/invitations"]["post"]["responses"]["200"] =
        paths["/api/v1/jobs/{job}/invitations"]["post"]["responses"]["201"].clone();
    endpoint!("get" "/api/v1/sessions", "Sessions", staff, "List interview sessions", "200" => json_response(response_schema::<interview::SessionListResponse>()),
        "parameters" => json!(pagination()));
    endpoint!("get" "/api/v1/sessions/{session}", "Sessions", staff, "Get interview session details", "200" => json_response(response_schema::<interview::SessionDetailResponse>()));
    paths["/api/v1/jobs/{job}/invitations"]["post"]["responses"]["202"] =
        paths["/api/v1/jobs/{job}/invitations"]["post"]["responses"]["201"].clone();
    endpoint!("post" "/api/v1/sessions/{session}/invitation", "Invitations", staff, "Retrieve an active invitation", "200" => json_response(response_schema::<invitation::RecoveredInvitationResponse>()));
    endpoint!("get" "/api/v1/audit", "Audit", staff, "List audit events", "200" => json_response(response_schema::<audit::AuditPageResponse>()),
        "parameters" => json!(audit_parameters(pagination())));
    endpoint!("get" "/api/v1/candidate/session", "Candidate", candidate, "Get the candidate session", "200" => json_response(response_schema::<candidate::CandidateSessionResponse>()));
    endpoint!("post" "/api/v1/candidate/session/{action}", "Candidate", candidate, "Start or submit the interview", "200" => json_response(response_schema::<candidate::CandidateSessionResponse>()));
    endpoint!("get" "/api/v1/candidate/room", "Candidate", candidate, "Open a question in the interview room", "200" => json_response(response_schema::<answer::RoomResponse>()),
        "parameters" => json!([parameter("position", "query", false, json!({"type": "integer", "minimum": 1}))]));
    endpoint!("post" "/api/v1/candidate/answers/{question}", "Candidate", candidate, "Save an answer revision", "200" => json_response(response_schema::<answer::SavedAnswerResponse>()),
        "requestBody" => json_body(request_schema::<answer::SaveAnswerRequest>()));
    endpoint!("post" "/api/v1/candidate/recommendation", "Candidate", candidate, "Select the next question", "200" => json_response(response_schema::<adaptive::RecommendationResponse>()),
        "requestBody" => json_body(request_schema::<adaptive::RecommendationRequest>()));
    endpoint!("post" "/api/v1/candidate/events", "Proctoring", candidate, "Record browser and media observations", "200" => json_response(response_schema::<proctoring::AcceptedResponse>()),
        "requestBody" => json_body(request_schema::<Vec<proctoring::ObservationRequest>>()));
    endpoint!("post" "/api/v1/candidate/events/{event}/evidence/{asset}", "Proctoring", candidate, "Upload evidence bytes", "200" => json_response(response_schema::<evidence::EvidenceResponse>()),
        "requestBody" => media_body(EVIDENCE),
        "parameters" => json!(evidence_headers()));
    endpoint!("get" "/api/v1/sessions/{session}/evidence/{asset}", "Proctoring", staff, "Read evidence bytes", "200" => Some(content(EVIDENCE, binary.clone())));
    endpoint!("post" "/api/v1/candidate/questions/{question}/recordings/{recording}", "Recordings", candidate, "Upload answer recording bytes", "200" => json_response(response_schema::<media::RecordingResponse>()),
        "requestBody" => media_body(MEDIA));
    endpoint!("get" "/api/v1/candidate/recordings", "Recordings", candidate, "List candidate recordings", "200" => json_response(response_schema::<media::RecordingsResponse>()));
    endpoint!("get" "/api/v1/candidate/recordings/{recording}", "Recordings", candidate, "Read a candidate recording", "200" => Some(content(MEDIA, binary.clone())));
    endpoint!("get" "/api/v1/sessions/{session}/recordings", "Recordings", staff, "List recordings for a session", "200" => json_response(response_schema::<media::RecordingsResponse>()));
    endpoint!("get" "/api/v1/sessions/{session}/recordings/{recording}", "Recordings", staff, "Read a session recording", "200" => Some(content(MEDIA, binary.clone())));
    endpoint!("post" "/api/v1/sessions/{session}/live-screen", "Live-screen", staff, "Exchange interviewer WebRTC signals", "200" => json_response(response_schema::<live_screen::View>()),
        "requestBody" => json_body(request_schema::<live_screen::Signal>()));
    endpoint!("post" "/api/v1/candidate/live-screen", "Live-screen", candidate, "Exchange candidate WebRTC signals", "200" => json_response(response_schema::<live_screen::View>()),
        "requestBody" => json_body(request_schema::<live_screen::Signal>()));
    endpoint!("post" "/api/v1/sessions/{session}/generation", "Generation", staff, "Retry this candidate's failed question preparation", "202" => json_response(response_schema::<generation::PendingResponse>()));
    endpoint!("get" "/api/v1/sessions/{session}/generation", "Generation", staff, "Get prepared interview questions", "200" => json_response(response_schema::<generation::SessionGenerationResponse>()));
    endpoint!("get" "/api/v1/jobs/{job}/question-sets", "Generation", staff, "List job question sets", "200" => json_response(response_schema::<generation::QuestionSetsResponse>()));
    endpoint!("post" "/api/v1/jobs/{job}/question-sets", "Generation", staff, "Retry failed question set generation", "202" => json_response(response_schema::<generation::PendingResponse>()));
    endpoint!("get" "/api/v1/sessions/{session}/assessment", "Assessment", staff, "Get scoring results and status", "200" => json_response(response_schema::<scoring::AssessmentResponse>()));
    endpoint!("post" "/api/v1/sessions/{session}/assessment/retry", "Assessment", staff, "Retry scoring", "200" => json_response(response_schema::<scoring::RetryResponse>()));
    endpoint!("get" "/api/v1/sessions/{session}/assessment/stream", "Assessment", staff, "Stream unverified scoring rationale as it is produced", "200" => stream());
    endpoint!("get" "/api/v1/sessions/{session}/rag", "Retrieval", staff, "Get evidence indexing status", "200" => json_response(response_schema::<rag::IndexStatusResponse>()));
    endpoint!("post" "/api/v1/sessions/{session}/rag", "Retrieval", staff, "Retry evidence indexing", "200" => json_response(response_schema::<rag::RetryResponse>()));
    endpoint!("get" "/api/v1/sessions/{session}/live", "Review", staff, "Stream live audit events", "200" => stream(),
        "parameters" => json!([parameter("after", "query", false, json!({"type": "string", "format": "uuid"}))]));
    endpoint!("get" "/api/v1/sessions/{session}/report", "Review", staff, "Export the interview report", "200" => json_response(response_schema::<review::ExportResponse>()));
    endpoint!("get" "/api/v1/jobs/{job}/analytics", "Review", staff, "Get question-bank analytics", "200" => json_response(response_schema::<review::AnalyticsResponse>()));
    endpoint!("get" "/api/v1/metrics", "Review", staff, "Get interviewer processing metrics", "200" => json_response(response_schema::<review::MetricsResponse>()));
    endpoint!("get" "/api/v1/sessions/{session}/integrity", "Review", staff, "Get session integrity observations", "200" => json_response(response_schema::<review::IntegrityResponse>()));
    endpoint!("get" "/api/v1/sessions/{session}/integrity/summary", "Review", staff, "Get the evidence-cited one-line AI integrity summary", "200" => json_response(response_schema::<review::IntegritySummaryResponse>()));
    endpoint!("post" "/api/v1/sessions/{session}/annotations", "Review", staff, "Save a reviewer annotation", "200" => json_response(response_schema::<review::SavedResponse>()),
        "requestBody" => json_body(request_schema::<review::AnnotationRequest>()));
    endpoint!("post" "/api/v1/sessions/{session}/review", "Review", staff, "Stream an answer from the reviewer assistant", "200" => stream(),
        "requestBody" => json_body(request_schema::<review::ChatRequest>()));
    endpoint!("get" "/docs", "Documentation", public, "Open Swagger UI", "200" => Some(content(&["text/html"], json!({"type": "string"}))));
    endpoint!("get" "/openapi.json", "Documentation", public, "Get the OpenAPI document", "200" => json_response(json!({"type": "object"})));

    json!({
        "openapi": "3.1.0",
        "info": {"title": "Interview Platform API", "version": env!("CARGO_PKG_VERSION")},
        "paths": paths,
        "components": {
            "securitySchemes": {
                "InterviewerToken": {"type": "http", "scheme": "bearer", "description": "Use the token returned by interviewer login."},
                "InvitationToken": {"type": "http", "scheme": "bearer", "description": "Use the candidate's invitation token."},
            },
            "responses": {
                "Error": {"description": "Request failed", "content": {"application/json": {"schema": {
                    "type": "object", "required": ["error", "code"],
                    "properties": {"error": {"type": "string"}, "code": {"type": "string"}}
                }}}}
            }
        }
    })
}

fn audit_parameters(mut parameters: Vec<Value>) -> Vec<Value> {
    parameters.extend([
        parameter(
            "entity_id",
            "query",
            false,
            json!({"type": "string", "format": "uuid"}),
        ),
        parameter(
            "candidate",
            "query",
            false,
            json!({"type": "string", "description": "Candidate name or email"}),
        ),
        parameter(
            "job",
            "query",
            false,
            json!({"type": "string", "description": "Job title or code"}),
        ),
        parameter(
            "from",
            "query",
            false,
            json!({"type": "integer", "description": "Inclusive Unix timestamp in seconds"}),
        ),
        parameter(
            "until",
            "query",
            false,
            json!({"type": "integer", "description": "Exclusive Unix timestamp in seconds"}),
        ),
    ]);
    parameters
}

fn evidence_headers() -> Vec<Value> {
    [
        (
            "x-evidence-source",
            json!({"type": "string", "enum": ["camera", "screen"]}),
        ),
        ("x-evidence-capture", json!({"type": "string"})),
        ("x-capture-surface", json!({"type": "string"})),
        (
            "x-evidence-captured-at",
            json!({"type": "integer", "description": "Unix timestamp in milliseconds"}),
        ),
    ]
    .into_iter()
    .map(|(name, schema)| parameter(name, "header", false, schema))
    .collect()
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    #[tokio::test]
    async fn documentation_routes_serve_the_ui_and_api_contract() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused@localhost/unused")
            .unwrap();
        let app = crate::router_with_ai(pool, None);
        let docs = app
            .clone()
            .oneshot(
                Request::get("/docs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(docs.status(), StatusCode::OK);
        let html = to_bytes(docs.into_body(), 32 * 1024)
            .await
            .unwrap();
        assert!(
            std::str::from_utf8(&html)
                .unwrap()
                .contains("SwaggerUIBundle")
        );

        let spec = app
            .oneshot(
                Request::get("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(spec.status(), StatusCode::OK);
        let bytes = to_bytes(spec.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let spec: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let paths = &spec["paths"];
        assert_eq!(spec["openapi"], "3.1.0");
        assert_eq!(
            paths["/api/v1/jobs"]["post"]["responses"]["201"]["content"]["application/json"]["schema"]
                ["properties"]["id"]["format"],
            "uuid"
        );
        assert_eq!(
            paths["/api/v1/candidate/answers/{question}"]["post"]["security"][0]["InvitationToken"],
            serde_json::json!([])
        );
        assert!(paths["/api/v1/sessions/{session}/review"]["post"]["responses"]["200"]["content"]["text/event-stream"].is_object());
    }
}
