# Deployment revision guide

Last updated: 18 November 2026. This guide records this project's deployment,
why each resource exists, and how to repeat or troubleshoot it. It contains no
passwords or access keys. Identifiers below are a snapshot; verify them before reuse.

## 1. What is deployed

The working deployment is **Vercel + one AWS EC2 instance + Neon PostgreSQL**.
Kubernetes is not involved in the current AWS deployment.

```text
Browser
  ├── HTTPS → Vercel → React static frontend
  └── HTTPS → EC2 port 443 → Caddy → Rust backend port 8080
                                      ├── Neon PostgreSQL + pgvector (TLS)
                                      ├── AI provider APIs
                                      └── Evidence files in Docker volume
```

| Resource | Current identifier | Why it exists |
| --- | --- | --- |
| GitHub repository | `rishavkumar-hyscaler/interview-platform-rust` | Source, workflows and container package |
| Default branch | `master` | Publishes releases and deploys after CI |
| Vercel frontend | `https://interview-platform-rust-two.vercel.app` | Builds and serves React over HTTPS |
| AWS account | `216583296809` | Owns the deployment resources |
| AWS region | `ap-southeast-2` (Sydney) | EC2, SSM and Parameter Store region |
| Current EC2 instance | `i-0ded0810e26a4b071`, name `interview-platform` | Runs backend and proxy containers |
| Instance configuration | Ubuntu x86-64, `t3.micro` | Matches the published amd64 backend image |
| Last reported public IP | `3.25.167.34` | Internet access to the instance; can change on stop/start |
| API host | `3-25-167-34.sslip.io` | IP-based DNS hostname used for HTTPS |
| Security group | `sg-0900f0b25ec1cff1c` (`launch-wizard-2`, from console) | Network rules; verify exact ID in console |
| Neon | Database/branch selected in private connection URLs | Persistent relational data and vectors |
| GHCR package | `ghcr.io/rishavkumar-hyscaler/interview-platform-rust/backend` | Stores built backend images |
| GitHub environment | `production` | Holds deployment secrets and deployment protections |

The user reported successful sign-up and app access after backend CI #12 and the
Vercel environment-variable redeployment. That proves initial connectivity, not
that every interview/proctoring feature has been exercised in production.

## 2. CI/CD flow: who does what

```text
Push backend/deployment changes to master (or dispatch workflow)
  ↓
GitHub Actions: format → lint → migrations/tests against temporary PostgreSQL
  ↓
GitHub Actions: build Docker image → publish image to GHCR
  ↓
GitHub Actions
  │ IAM user access keys + DeployInterviewPlatformEC2 policy
  ▼
Systems Manager sends deployment command
  ▼
EC2 instance
  │ interview-platform-ec2-role
  ▼
Install missing prerequisites → read Parameter Store → pull image
  ↓
Stop old backend workers → run migration container → start backend + Caddy
  ↓
GitHub checks public HTTPS /health/ready
```

The image is **built on GitHub**, not EC2. The runner sends the checked-out
Compose/configuration/scripts through SSM. EC2 pulls the already-built image.
There is no repository clone or Rust compilation on EC2 in this automatic flow.
The image reference contains an immutable SHA-256 digest, rather than `latest`.

Pull requests run CI but do not publish/deploy. Push path filters cover `backend/**`,
`deploy/aws/**` and the backend workflow itself; not every frontend/docs push runs
backend CI. `workflow_dispatch` supports a manual Actions run. A `production`
concurrency group serializes deployments. Vercel separately builds the frontend.

## 3. Users, roles and policies

| Identity/resource | Used by | Purpose |
| --- | --- | --- |
| `interview-platform-github-deploy-user` | GitHub Actions | Dedicated programmatic IAM user; its access keys authenticate AWS API calls |
| `DeployInterviewPlatformEC2` | Attached to that IAM user | Allows `ssm:SendCommand` to the chosen instance using `AWS-RunShellScript`, plus polling with `ssm:GetCommandInvocation` |
| `interview-platform-ec2-role` | Attached to EC2 via an instance profile | Lets SSM Agent register/communicate and lets the instance read application parameters |
| `AmazonSSMManagedInstanceCore` | Attached to the EC2 role | Standard permissions needed by the instance's SSM Agent |
| Scoped `ssm:GetParameter` permissions | Attached to EC2 role | Read only the runtime and migration parameter ARNs; customer-managed KMS keys also need scoped decrypt permission |
| Neon owner (`neondb_owner`, or actual owner in URL) | Migration container | Creates schema/extensions and grants application table access |
| `platform_app` | Backend container | Restricted PostgreSQL login; respects row-level security |
| SSH key pair | Optional human administration | Emergency/manual connection; not used by GitHub deployment |

**Role vs policy:** a role is an identity assumed by a service; a policy is a set
of allowed actions attached to an identity. GitHub's IAM user and EC2's role have
separate jobs. Do not put GitHub access keys on EC2 or run `aws configure` there:
EC2 obtains temporary credentials through its instance profile.

The deploy-user policy's resource ARN must contain the **current** instance ID:

```text
arn:aws:ec2:ap-southeast-2:216583296809:instance/i-0ded0810e26a4b071
arn:aws:ssm:ap-southeast-2::document/AWS-RunShellScript
```

The checked-in `deploy/aws/ec2/deploy-user-policy.json` was originally created for
old instance `i-0c575eb4803c49d1c`; the user updated the live AWS policy for the new
instance. Always replace that old ID before reusing the file to create a policy.
`GetCommandInvocation` uses resource `*` with a region condition. The user does not
need AdministratorAccess, EC2FullAccess, or direct Parameter Store read rights.
However, sending root shell commands grants effective control of that server and
its application secrets. Keep repository writes and production secrets restricted.
Access keys do not override an AWS Organizations service control policy (SCP).

The EC2 role trust relationship must allow the EC2 service:

```json
{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"Service": "ec2.amazonaws.com"},
    "Action": "sts:AssumeRole"
  }]
}
```

## 4. One-time setup, in order

### A. Prepare Neon

1. Select the intended Neon branch and database. The database must already exist;
   migrations create schema, not the database itself.
2. As the database owner, create the restricted role **on that Neon branch**:

```sql
CREATE ROLE platform_app
    LOGIN
    PASSWORD 'CHOOSE_A_PRIVATE_PASSWORD'
    NOSUPERUSER
    NOCREATEDB
    NOCREATEROLE
    NOBYPASSRLS;
```

3. Obtain owner and runtime connection URLs for the same branch/database. Use a
   direct owner endpoint for migrations and TLS (`sslmode=require`). The URL must
   include the database name. URL-encode special characters in passwords.
4. The initial migration enables `vector` and grants table access to the existing
   `platform_app` role. Creating a role on local PostgreSQL does not create it in Neon.

```sql
-- Inspect roles, or update a password privately:
SELECT rolname, rolcanlogin, rolsuper, rolbypassrls
FROM pg_roles
WHERE rolname = 'platform_app';

ALTER ROLE platform_app PASSWORD 'YOUR_NEW_PRIVATE_PASSWORD';
```

### B. Launch EC2

1. Use Ubuntu **x86-64**, matching the image platform, in Sydney.
2. Use persistent encrypted gp3 storage; about 20 GiB was recommended. Confirm
   the actual volume size/encryption in EC2; the final storage settings were not
   verified from the successful deployment screenshot.
3. Use a public subnet with an internet route, public IPv4 and outbound access.
4. Allow incoming TCP **80/443** for the proxy. If using SSH, limit TCP **22** to
   your IP; SSM does not need inbound port 22. Do not expose 8080 or PostgreSQL 5432.
5. Attach `interview-platform-ec2-role`. Require IMDSv2.
6. SSM Agent must already be installed/running and appear **Online**. Official
   Ubuntu AMIs commonly include it. For a new launch, `bootstrap.sh` can also be
   supplied through Advanced details → User data.
7. Do not launch a replacement merely to restart: **Start instance** resumes a
   stopped instance. A terminated/deleted instance needs replacement and setup.

### C. Save application settings in AWS Parameter Store

In **Systems Manager → Parameter Store**, Sydney, create Standard **SecureString**
parameters using private multiline `KEY=value` text, without shell quotes:

| Parameter | Contents |
| --- | --- |
| `/interview-platform/production/runtime` | Restricted `DATABASE_URL`, stable `INVITATION_ENCRYPTION_KEY`, AI settings and application options |
| `/interview-platform/production/migrations` | Only owner-credential `DATABASE_URL` |

Runtime template (replace privately; never commit populated values):

```dotenv
DATABASE_URL=postgresql://platform_app:ENCODED_PASSWORD@NEON_HOST/DATABASE?sslmode=require
INVITATION_ENCRYPTION_KEY=YOUR_STABLE_64_HEX_CHARACTER_KEY
ALLOW_REGISTRATION=true
LLM_PROVIDER=openai
LLM_BASE_URL=https://api.openai.com/v1
LLM_MODEL=gpt-5-mini
LLM_API_KEY=YOUR_PRIVATE_KEY
EMBEDDING_MODEL=text-embedding-3-small
EMBEDDING_API_KEY=YOUR_PRIVATE_KEY
WEBRTC_ICE_SERVERS=[]
```

Disable registration when appropriate after provisioning accounts. Do not rotate
invitation encryption keys casually; existing invitations depend on the key.
The EC2 Compose file already supplies `BIND_ADDRESS=0.0.0.0:8080`, media location,
Vercel frontend origin and connection-pool limits. No `LOG_FORMAT` setting is needed.
Parameter changes take effect at the **next backend deployment**, not immediately.

### D. Configure GitHub

Create the dedicated IAM user, attach the restricted deployment policy and create
its access key. Console login is unnecessary for this user.

**Settings → Environments → production → Environment secrets:**

| Secret | Value |
| --- | --- |
| `AWS_ACCESS_KEY_ID` | Dedicated IAM user's access key ID |
| `AWS_SECRET_ACCESS_KEY` | Its secret access key |

**Settings → Secrets and variables → Actions → Repository variables:**

| Variable | Current value |
| --- | --- |
| `BACKEND_DEPLOY_TARGET` | `ec2` |
| `BACKEND_DEPLOY_ENABLED` | `true` |
| `AWS_REGION` | `ap-southeast-2` |
| `EC2_INSTANCE_ID` | `i-0ded0810e26a4b071` |
| `API_HOST` | `3-25-167-34.sslip.io` (no scheme/path) |
| `EC2_RUNTIME_PARAMETER` | `/interview-platform/production/runtime` |
| `EC2_MIGRATION_PARAMETER` | `/interview-platform/production/migrations` |

Restrict production deployment branches to `master`; protect write access. Make
GHCR's backend package public, or separately configure root's read-only registry
login on EC2. Public source visibility does not itself make a package public.
`AWS_DEPLOY_ROLE_ARN` is not used by the access-key EC2 job.

### E. Push and deploy the backend

1. Commit and push the repository deployment changes.
2. In GitHub Actions, confirm `backend` → `publish` → `deploy-ec2` succeed.
3. The alternative EKS `deploy` job should be skipped when target is `ec2`.
4. Check `https://3-25-167-34.sslip.io/health/ready`.
5. Database/configuration corrections alone can use **Re-run failed jobs**.
   Changed repository scripts require a **new push/run**; rerunning an old commit
   does not pick up new code.

### F. Connect Vercel

1. Import the repository with Root Directory **`frontend`** and Vite configuration.
2. Add `VITE_API_BASE_URL=https://3-25-167-34.sslip.io` for Production.
3. Redeploy the frontend after changing environment variables: Vite embeds them
   at build time. Saving the variable is not enough.
4. Open the production URL and test account access. Network requests should go
   to the API hostname, not Vercel's `/api/...` paths.
5. Caddy supplies HTTPS on EC2; EC2 itself does not automatically issue a TLS
   certificate. It needs correct DNS plus publicly reachable ports 80/443.

## 5. Files to revise

Paths below are relative to the repository root.

| File/directory | What it does / where it runs |
| --- | --- |
| `.github/workflows/backend.yml` | GitHub CI, temporary pgvector PostgreSQL tests, GHCR publish, EC2/EKS job selection |
| `.github/workflows/frontend.yml` | Frontend CI; Vercel separately handles hosting |
| `backend/Dockerfile` | Multi-stage Rust release build; small runtime image, non-root UID 10001 |
| `backend/.dockerignore` | Keeps local build/cache/private files out of Docker build context |
| `backend/.env.example` | Documents runtime options; not actual deployed secrets |
| `backend/infra/postgres/init.sql` | Local/CI role provisioning; do not reuse its development password in Neon |
| `backend/migrations/` | Schema, pgvector and grants; executed by `platform-api --migrate` |
| `frontend/vercel.json` | Vite framework, npm installation/build and `dist` output |
| `frontend/src/transport.ts` | Frontend API transport and backend base URL |
| `deploy/aws/ec2/send-deploy.py` | Runs on GitHub; packages files, calls SSM, polls command status |
| `deploy/aws/ec2/bootstrap.sh` | Runs as root on EC2; installs missing Docker/Compose, AWS CLI, Python/flock; avoids restarting active SSM during deployment |
| `deploy/aws/ec2/remote-deploy.sh` | Runs on EC2; checks prerequisites, locks deployment, fetches/decrypts parameters to private files |
| `deploy/aws/ec2/deploy.sh` | Runs Compose on EC2: pull → stop backend → migrate → start backend/proxy |
| `deploy/aws/ec2/compose.yaml` | Backend/proxy/services, internal networking, volumes, restart policy and log rotation |
| `deploy/aws/ec2/Caddyfile` | HTTPS reverse proxy to `backend:8080`; immediate streaming flush |
| `deploy/aws/ec2/deployment.env.example` | Public settings/path example for the manual alternative |
| `deploy/aws/ec2/deploy-user-policy.json` | IAM policy template; replace old instance ID before reuse |
| `deploy/aws/ec2/README.md` | Focused EC2 deployment instructions |
| `deploy/aws/k8s/`, `deploy/aws/render.py`, `deploy/aws/deploy.sh` | Preserved EKS alternative; not used by current EC2 deployment |
| `deploy/aws/README.md` | EKS and Neon setup reference |
| `deploy/local/` | Separate Minikube learning setup; not used on EC2 |

## 6. What gets created on EC2

```text
/opt/interview-platform/
├── bootstrap.sh             # prerequisite installation
├── compose.yaml             # container configuration
├── Caddyfile                # HTTPS reverse proxy
├── deploy.sh                # Compose release steps
├── remote-deploy.sh         # configuration + deployment entrypoint
├── deploy.lock              # serializes remote deployments
└── config/                  # root-only directory
    ├── runtime.env          # decrypted runtime parameter; mode 600
    ├── migrations.env       # decrypted migration parameter; mode 600
    └── deployment.env       # placeholder; public deployment values passed as env
```

| Container/volume | Purpose |
| --- | --- |
| `interview-platform-backend-1` | Rust HTTP server and background workers |
| `interview-platform-proxy-1` | Caddy HTTP/HTTPS entrypoint |
| Temporary `migrate-run-*` container | Same image with `--migrate`; removed after execution |
| `interview-platform_media` | Evidence files mounted at `/data/media` |
| `interview-platform_caddy_data` | TLS certificates and Caddy persistent state |
| `interview-platform_caddy_config` | Caddy configuration state |

These are Docker Compose containers, **not Kubernetes Pods**. Docker volumes survive
container replacement, but can be lost with EC2 disk deletion. Back up evidence
alongside Neon. `docker compose down -v` deletes volumes; do not use it to pause.
Docker's `json-file` driver is its storage envelope; application messages are now
compact human-readable text. Log rotation limits each service to three 10 MB files.

## 7. Logs and useful checks

EC2 → instance → Connect → Session Manager opens a shell on the server.
It does not itself deploy anything.

```bash
sudo docker ps -a
sudo docker logs --tail 100 -f interview-platform-backend-1
sudo docker logs --tail 100 -f interview-platform-proxy-1
curl --fail https://3-25-167-34.sslip.io/health/live
curl --fail https://3-25-167-34.sslip.io/health/ready
```

Ctrl+C stops following logs, not the containers. `/health/live` checks process
liveness; `/health/ready` checks readiness including database connectivity.

For deployment failures: **Systems Manager → Run Command → Command history →
command ID → Targets and outputs → instance → Output/Error**. GitHub prints the
command ID but deliberately does not copy all remote output into public logs.
Never share populated env files, passwords, keys, or decrypted parameter output.

To verify parameter access without displaying values (on EC2):

```bash
aws ssm get-parameter \
  --name /interview-platform/production/runtime \
  --with-decryption --region ap-southeast-2 \
  --query 'Parameter.Name' --output text
```

## 8. Errors we encountered and their fixes

| Symptom | Cause / fix |
| --- | --- |
| OIDC token could not be validated | Previous role/provider setup failed. We switched EC2 authentication to dedicated IAM-user access keys. Provider inspection was SCP-denied; the precise OIDC root cause was not established. |
| SSM SendCommand fails | Check actual AWS error, policy attachment/current instance ID, region, Running state and SSM Online status. |
| SSM Agent cannot acquire credentials | Check instance profile attachment, EC2 role trust and `AmazonSSMManagedInstanceCore`; GitHub's policy does not replace the instance role. |
| `No such file or directory: aws` | Fresh EC2 lacked AWS CLI/PATH access. Pipeline now bootstraps missing prerequisites and sets a standard PATH. |
| `role "platform_app" does not exist` | Create role on the correct Neon branch before migrating; local roles are unrelated. |
| API rejects superuser/BYPASSRLS | Use restricted `platform_app` for runtime, owner only for migrations. Do not remove the security check. |
| Frontend `/api/auth/register` returns Vercel 404 | Frontend build had no backend URL. Set `VITE_API_BASE_URL` and redeploy Vercel. |
| Migration fails after stopping backend | Intentional fail-fast deployment; backend stays stopped. Correct the cause and rerun deployment. |
| API host stops working after EC2 restart | Public IP may change. Update GitHub `API_HOST`, Vercel API URL and redeploy both as needed. |

## 9. Stop, replace, update and roll back

- **Stop/start:** preserves EBS-backed installation/data, but public IP may change.
  Docker and `restart: unless-stopped` restart containers after a normal instance
  reboot, unless a container was deliberately stopped. Verify readiness afterward.
- **Terminate:** deletes the server; root-volume deletion settings determine disk
  retention. A replacement needs its role, SSM and bootstrap again. Update GitHub
  instance ID/API host, IAM policy instance ARN and Vercel URL.
- **Update application:** push relevant changes; CI/publish/deploy run automatically.
- **Update parameter:** save private settings in Parameter Store and trigger backend
  deployment; it fetches fresh values.
- **Pause automation:** set `BACKEND_DEPLOY_ENABLED=false`. CI/image publication
  can still run. This does not stop EC2 itself.
- **Rollback:** select an earlier image only if compatible with the current schema.
  There is no automatic database rollback or high-availability failover here.
- **Costs:** compute, disks, public IPs, other retained AWS resources, AI use and
  provider plan limits are separate. Stopping EC2 does not delete disks or other
  resources. Inspect leftovers from earlier EKS experiments in the AWS console.

## 10. Previous experiments and local Kubernetes

| Item | Status / distinction |
| --- | --- |
| Old EC2 `i-0c575eb4803c49d1c` | User terminated it; do not target it |
| `interview-platform-github-deploy` IAM role | Previous OIDC deployment role; not used by the current EC2 job |
| GitHub OIDC provider/trust configuration | Attempted earlier; not needed by current EC2 access-key authentication |
| `eks-learning` IAM user | Seen in console; not required by this pipeline; permissions/current use not audited |
| EKS roles/clusters/network resources | Created/attempted earlier; existence and billing status not audited; do not assume removed |
| SSH key `interview-platform-ec2-key.pem` | Used for earlier manual access; optional for automatic SSM deployment |
| Minikube profile `interview-platform` | Local learning cluster, separate from AWS |
| Minikube profile `minikube` | Another local cluster; starting it does not start the app profile |

Local manifests currently deploy **one backend Pod**, a migration Job, Service and
persistent evidence volume in namespace `interview-platform-local`. Frontend still
uses `npm run dev`; a frontend Pod was discussed but not implemented. PostgreSQL
is external to this cluster. The helper `deploy/local/connect-postgres.sh` added a
local Docker-host listener and restricted pg_hba rule; those changes concern the
laptop database, not Neon. See `deploy/local/README.md` for commands.

Remember: **IAM user authenticates GitHub; EC2 role authenticates the instance;
Neon role authenticates database access; Vercel variable connects the browser to
our API. These are four different concerns.**
