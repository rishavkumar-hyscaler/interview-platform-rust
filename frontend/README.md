# Interview platform frontend

Independent React/TypeScript application. Requires Node 22.12+ (or a supported newer LTS). No Rust, database, or backend checkout is needed to build it.

```bash
cp .env.example .env
npm ci
npm run dev
```

For local development, leave `VITE_API_BASE_URL` empty. Vite proxies `/api` and `/health` to `http://127.0.0.1:8080`, so `localhost`, `127.0.0.1`, and alternate frontend ports work without browser CORS problems. Override `API_PROXY_TARGET` only when the backend uses another address. Restart Vite after editing environment settings.

For separately deployed production services, set `VITE_API_BASE_URL` to the HTTPS API origin and configure the backend's `FRONTEND_ORIGIN` to the exact frontend origin. Vite's development proxy is not included in the production build; a same-origin production deployment must configure its own reverse proxy. Frontend environment values are public, never secrets.

```bash
npm run lint
npm run build
npm run preview
```

The UI supports interviewer registration with email/password, direct login, and logout. The same interviewer account can set up interviews and review candidates. There is no owner role, organization-name form, or workspace-ID login step. Existing accounts retain their credentials and interview records after the compatibility migration.

Interview setup creates jobs and candidate invitation links. Review shows session details, pinned job requirements, and paginated audit history. Session lists support local search/status filters over loaded pages, plus pagination and refresh. Dashboard counts come from server totals.

Candidates use private invitation links and can accept an invitation. Question preparation is available from session details. Interviewees can start a prepared interview, answer one question at a time, revisit questions, and submit. Server-authoritative time and saved revisions survive refresh. Text/scenario and code answers use an accessible textarea editor; code is not executed. Interviewer tokens are kept in per-tab sessionStorage so refresh preserves login. On reload, /api/me validates the token and supplies identity and role. Expired/revoked tokens are removed; network failures offer reconnection without discarding the stored session. Logout revokes the server session and clears browser storage. Closing the tab ends local persistence. Passwords are never stored. Registration depends on backend policy.

If port 5173 is occupied, use `npm run dev -- --port 5174` with the default same-origin proxy. Do not stop unrelated applications.

Deploy the dist directory on a static host. Configure VITE_API_BASE_URL before building and configure HTTPS and the API origin allowlist independently.

The workflow in .github/workflows/ci.yml is ready when this directory is used as a repository root.

Session details provide larger, darker text and a saved invitation link action. Retrieve and copy an active invitation link to share again. Unaccepted legacy invitations can create a replacement, with an explicit warning that the old link is invalidated. Email delivery is not connected.

Active saved invitation links load directly when session details open. Older API responses without invitation flags use state/deadline checks, instead of incorrectly labelling the invitation expired. Restart the API after backend changes; cargo run does not hot reload Rust code.

Candidate invitation URLs open automatically, without a second Open invitation button. The token stays in the URL fragment (never sent with HTTP page requests), making refresh/back/forward resumable. Hash changes switch candidate context even when a recruiter page is already open. Candidate access never requires a recruiter password; invalid/expired links show a clear error with retry.

The room autosaves after a short typing pause and before navigation/submission. Unacknowledged drafts are journaled in localStorage per interview/question, then removed after matching acknowledgement. Refresh recovers them; conflicting saves from another tab require an explicit choice. The overall timer continues offline, while per-question elapsed/suggested time is displayed. At expiry the server submits acknowledged answers. Locally retained drafts are shown separately if they did not reach the server. Interviewers can inspect saved answers alongside each rubric.

Question preparation starts automatically when a configured interviewer creates an invitation. Older invitations trigger preparation when opened/accepted. Candidates poll preparation status and see Start interview when questions are ready; interviewer retry remains available if generation fails.
