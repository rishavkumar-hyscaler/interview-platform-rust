# Interview platform

Two independent projects communicating over HTTP:

- [backend](backend/README.md): Rust/Axum API with in-process durable expiry jobs, PostgreSQL/pgvector, interviewer ownership isolation, and interview domain.
- [frontend](frontend/README.md): React/TypeScript interviewer and reviewer workspaces, job/invitation forms, session details, and audit history.

The database uses one baseline and seven application tables, with interviewer/interviewee user roles. JD-driven question preparation and the timed interview room are implemented, including autosave, navigation, reconnect recovery, submission, and interviewer answer review. Adaptive difficulty, AI scoring, proctoring, and reviewer RAG are subsequent phases. See the [API contract](backend/docs/api.md) for current capabilities and limits.

The backend uses the existing `.env` PostgreSQL connection at `localhost:5432/interview-platform`; `.env` is private and ignored by Git. Each project builds independently and has its own CI and configuration. The backend has one Cargo package, one `Cargo.toml`, and one executable; API, database, and background job code are internal modules.
