# Foundation architecture

The API owns HTTP validation and orchestration; infrastructure owns database connectivity and tenant transaction setup. An in-process background job loop processes durable PostgreSQL session-expiry jobs with transactional acknowledgement, row locking, retry limits, and tenant scope. See api.md for exact delivery semantics. Interview routes are isolated in the API interview module; database/configuration and expiry processing live in the infrastructure module. One Cargo package and executable own all backend modules. Generation and scoring services arrive with the next phases.

## Authentication and tenant boundaries

Passwords use Argon2id defaults with random salts, off the asynchronous executor. Random 256-bit opaque session tokens are hashed with SHA-256 in PostgreSQL and expire after 12 hours. Logout deletes the session. The frontend persists the opaque recruiter token in per-tab sessionStorage to survive refresh, validates it through /api/me before restoring identity/scope, and clears it on logout or 401. Passwords are never stored in the browser. As with any JavaScript-accessible token, XSS prevention remains necessary. Do not log authorization headers or password bodies.

Identity tables are global and accessible only to the trusted backend role. Organization-owned records use forced RLS with transaction-local `app.tenant_id`. A requested organization UUID is not authorization: every tenant endpoint must also verify the authenticated recruiter's membership before accessing tenant resources. RLS limits accidental cross-tenant queries; a compromised backend credential remains a serious security breach. No user-supplied SQL is accepted.

The runtime role has no update/delete grant on audit records. This protects against ordinary application mutations, not privileged database administrators. Cryptographic evidence chains and external archival arrive with proctoring.

## Operational boundaries

Liveness does not query PostgreSQL; readiness does. Connections are pooled with acquisition timeouts. Logs are structured JSON and HTTP tracing excludes request bodies. HTTP and background jobs share one pool and a shutdown signal. Ctrl-C and SIGTERM stop intake, finish the current short expiry transaction, and close the pool. Managed infrastructure will be hardened before production deployment.

Local Compose exposes PostgreSQL only on loopback. The frontend is an independent HTTP client, with no source, build, environment, or database dependencies on this project. Configure FRONTEND_ORIGIN to its exact origin. Production requires HTTPS for both deployments.

## Remaining production release gates

This is a foundation, not a production-ready release. Before public exposure: deploy TLS, edge request/login throttling and concurrency limits; add verified-email onboarding/invitations, account recovery, credential rotation, session management, and an abuse policy; implement retention and deletion; provision least-privilege managed secrets; test backup restoration; add telemetry export, alerts, load testing, security review, and deployment rollback. Keep public registration disabled unless onboarding is intentionally supported.

AI providers, object storage, vector dimensions/indexes, generation/scoring job handlers, request correlation, and OpenAPI generation will be introduced with the workflows that consume them. PostgreSQL has the vector extension enabled but no speculative embedding table.

Product access is recruiter/reviewer plus candidate capabilities. Legacy organization and membership tables remain internal isolation boundaries. Login resolves the recruiter record scope automatically; there is no user-facing owner or workspace-selection feature.
