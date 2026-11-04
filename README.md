# Interview platform

Two independent projects communicating over HTTP:

- [backend](backend/README.md): Rust/Axum API with in-process durable expiry jobs, PostgreSQL/pgvector, tenant isolation, and interview domain.
- [frontend](frontend/README.md): React/TypeScript interviewer and reviewer workspaces, job/invitation forms, session details, and audit history.

Phase 1 is implemented. Question generation, adaptive interviews, scoring, proctoring, and reviewer RAG are subsequent phases. See the [API contract](backend/docs/api.md) for current capabilities and limits.

The backend uses the existing `.env` PostgreSQL connection at `localhost:5432/interview-platform`; `.env` is private and ignored by Git. Each project builds independently and has its own CI and configuration. The backend has one Cargo package, one `Cargo.toml`, and one executable; API, database, and background job code are internal modules.
