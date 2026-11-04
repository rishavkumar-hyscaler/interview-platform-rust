# Interview platform frontend

Independent React/TypeScript application. Requires Node 22.12+ (or a supported newer LTS). No Rust, database, or backend checkout is needed to build it.

```bash
cp .env.example .env
npm ci
npm run dev
```

Set VITE_API_BASE_URL to the API deployment URL (no trailing slash). The default is http://localhost:8080. Set the API's FRONTEND_ORIGIN to the exact browser origin, normally http://localhost:5173. Development uses port 5173 with strict port selection. Environment values are public build-time configuration, never secrets.

```bash
npm run lint
npm run build
npm run preview
```

The UI supports recruiter registration with email/password, direct login, and logout. The same recruiter/reviewer account can set up interviews and review candidates. There is no owner role, organization-name form, or workspace-ID login step. Existing accounts retain their credentials and interview records after the compatibility migration.

Interview setup creates jobs and candidate invitation links. Review shows session details, pinned job requirements, and paginated audit history. Session lists support local search/status filters over loaded pages, plus pagination and refresh. Dashboard counts come from server totals.

Candidates use private invitation links and can accept an invitation. Question generation and the timed interview room arrive in later phases. Recruiter tokens are kept in per-tab sessionStorage so refresh preserves login. On reload, /api/me validates the token and supplies identity and record scope. Expired/revoked tokens are removed; network failures offer reconnection without discarding the stored session. Logout revokes the server session and clears browser storage. Closing the tab ends local persistence. Passwords are never stored. Registration depends on backend policy.

If port 5173 is occupied, use `npm run dev -- --port 5174` and set the API's `FRONTEND_ORIGIN=http://localhost:5174`. Do not stop unrelated applications.

Deploy the dist directory on a static host. Configure VITE_API_BASE_URL before building and configure HTTPS and the API origin allowlist independently.

The workflow in .github/workflows/ci.yml is ready when this directory is used as a repository root.

Session details provide larger, darker text and a saved invitation link action. Retrieve and copy an active invitation link to share again. Unaccepted legacy invitations can create a replacement, with an explicit warning that the old link is invalidated. Email delivery is not connected.

Active saved invitation links load directly when session details open. Older API responses without invitation flags use state/deadline checks, instead of incorrectly labelling the invitation expired. Restart the API after backend changes; cargo run does not hot reload Rust code.

Candidate invitation URLs open automatically, without a second Open invitation button. The token stays in the URL fragment (never sent with HTTP page requests), making refresh/back/forward resumable. Hash changes switch candidate context even when a recruiter page is already open. Candidate access never requires a recruiter password; invalid/expired links show a clear error with retry.
