# Single EC2 deployment

Vercel serves the frontend, Neon hosts PostgreSQL, and one EC2 instance runs the
published backend image plus Caddy for HTTPS. No Kubernetes, ALB or NAT gateway is
needed for this initial public-subnet deployment. Choose the automatic GitHub Actions setup below, or the manual alternative.
The pipeline tests and publishes the image before deploying.

## 1. Launch the instance

Choose Ubuntu 24.04 LTS **x86-64** (the image is built for amd64), encrypted gp3
storage of about 20 GB, and a public subnet with internet access/public IPv4.
Start with a small instance with 2 GB RAM if your budget permits; 1 GB is a tighter
trial configuration. Check your account's free-tier/credit eligibility before launch.
Require IMDSv2. Use a security group allowing:

- TCP 22 from your own IP only for SSH.
- TCP 80 and 443 for public HTTP/HTTPS.
- No inbound 8080 or 5432.

Install Docker Engine and the Docker Compose plugin following
[Docker's Ubuntu instructions](https://docs.docker.com/engine/install/ubuntu/).
Enable Docker at boot. The commands below assume your user can run Docker.

Use a stable public IP and point an API subdomain's DNS A record to it. Public IPv4
and disks can remain billable when compute is stopped. Caddy obtains/renews TLS
certificates once DNS and ports 80/443 are reachable; no ACM setup is required.
An HTTPS API is necessary for the HTTPS Vercel frontend.

## 2. Copy code and prepare private settings

Clone the repository onto EC2. Keep deployment files and private configuration
outside the repository, for example under `/home/ubuntu/.config/interview-platform`.
Create the directory with mode 700 and the following files with mode 600:

- `deployment.env`: copy `deployment.env.example`; replace image tag and API host.
  Use the exact commit tag from the successful GitHub publish job.
- `runtime.env`: use the application keys from `backend/.env.example`, including
  `DATABASE_URL`, AI keys and a stable `INVITATION_ENCRYPTION_KEY`.
- `migrations.env`: only `DATABASE_URL`, using the Neon migration owner's direct
  TLS connection. The runtime file instead uses the restricted `platform_app` role.
  Each process uses the same key name; no additional database URL variable exists.

Prepare the Neon `platform_app` role as described in the [EKS guide](../README.md#2-prepare-neon).
Migrations enable pgvector. Never run development role credentials against Neon.
Use Docker env-file syntax `KEY=value`; avoid shell quotes. Do not commit secrets.
Set `ALLOW_REGISTRATION=false` after creating your initial interviewer account.
Keep the invitation key stable and backed up with the database and evidence.

The Compose file fixes the Vercel origin, bind address, media path and pool settings.
If your Vercel domain changes, update `FRONTEND_ORIGIN` in Compose.
Make the GHCR package public or authenticate Docker with a read-only package token
on EC2 before pulling. Public repository visibility does not imply a public image.

## 3. Deploy

For manual deployment, leave GitHub `BACKEND_DEPLOY_ENABLED` unset or false.
For automatic EC2 deployment, use the settings below.

From the repository root on EC2:

```bash
bash deploy/aws/ec2/deploy.sh /home/ubuntu/.config/interview-platform/deployment.env
curl --fail https://api.example.com/health/live
curl --fail https://api.example.com/health/ready
```

The script pulls images, stops backend workers, runs migrations, and starts the
backend and HTTPS proxy. Migration failure stops deployment and leaves the backend
stopped for investigation. A successful script does not guarantee DNS/TLS/readiness;
verify the public health endpoints. Deployment has brief downtime; run only one
release at a time. To update, change the image tag in `deployment.env` and rerun.

Set Vercel `VITE_API_BASE_URL=https://api.example.com` and redeploy the frontend.
Verify sign-in, candidate invitation, evidence upload and streaming. WebRTC may
need an external TURN service for viewers on different networks; this setup does
not provision TURN. Keep one backend instance while signaling is held in memory.

## Operations

From `deploy/aws/ec2` on EC2:

```bash
docker compose --env-file /home/ubuntu/.config/interview-platform/deployment.env logs --tail 100 -f backend
```

Named volumes preserve evidence and Caddy certificates across container updates.
Back up media and Neon together; do not use `docker compose down -v`, which deletes
volumes. Terminating EC2 can delete its root disk and these volumes. A single EC2
instance has no automatic failover. To roll back, select an older image tag and
redeploy only if compatible with the current database schema.

You can stop/start EC2 for development. The app is unavailable while stopped;
containers restart when Docker starts again. A non-elastic public IP can change,
requiring a DNS update. Check for resources left by EKS attempts (clusters, nodes,
load balancers, NAT gateways and disks); this setup does not remove them.

Next steps: automate this working deployment through GitHub Actions, then move the
same image to single-node Kubernetes or EKS when needed.

## Automatic deployment with GitHub Actions (SSM)

After this one-time setup, pushes affecting backend/deployment files on the default
branch run tests → publish image → deploy EC2 → check public HTTPS readiness.
There is no Git clone, SSH key or manual `.env` editing on EC2 in this mode.
The workflow sends its checked-out Compose/scripts to `/opt/interview-platform`.

### One-time AWS setup

1. The pipeline automatically installs missing Docker/Compose, AWS CLI v2,
   Python 3 and `flock` using `bootstrap.sh` before deployment. Ensure SSM Agent is installed/running and the instance appears **Online** in
   Systems Manager managed nodes. Ubuntu installations may use the snap service
   `snap.amazon-ssm-agent.amazon-ssm-agent.service`. The instance needs outbound
   HTTPS to Systems Manager, Parameter Store, image registries, Neon and AI APIs.
2. Attach an EC2 instance-profile role with `AmazonSSMManagedInstanceCore`.
   Add `ssm:GetParameter` permission for only these two parameter ARNs. If using
   a customer-managed KMS key, add `kms:Decrypt` for that key as well.
3. In Systems Manager → Parameter Store, create two **SecureString** parameters
   in the EC2 region (paste multiline `KEY=value` contents privately):
   - `/interview-platform/production/runtime`: application runtime settings from
     the manual setup above, including restricted-role `DATABASE_URL` and AI keys.
   - `/interview-platform/production/migrations`: only owner-role `DATABASE_URL`.
   Do not put credentials in workflow variables or Run Command parameters.
   The instance writes these settings to root-only files under
   `/opt/interview-platform/config`; Docker receives only the corresponding file.
4. Create a dedicated IAM user named `interview-platform-github-deploy-user`,
   without console access or administrator policies. Attach the inline policy in
   [deploy-user-policy.json](deploy-user-policy.json). It is scoped to the current
   EC2 instance/account/region; adjust those public identifiers if the instance
   changes. The user can send shell commands to that instance and poll results.
   It cannot directly read application parameters or create AWS infrastructure.
   SSM shell commands execute as root and can access instance-held secrets, so
   these deployment credentials grant control of that specific server.
   An organization SCP can still deny these actions; access keys do not override it.
   Create an access key for this user and store it directly in GitHub's
   **Settings → Environments → production → Environment secrets**:
   - `AWS_ACCESS_KEY_ID`
   - `AWS_SECRET_ACCESS_KEY`
   Do not paste keys into source, repository variables, chat, or EC2 env files.
   Restrict the production environment to the default branch (`master`) and
   protect repository writes. Rotate keys by replacing the GitHub secrets and
   deactivating the old key after a successful deployment.
   EC2 deployment uses these keys directly: no OIDC provider, role assumption,
   trust policy, or `AWS_DEPLOY_ROLE_ARN` is required for this target.
5. Make the GHCR backend package public, or configure root's Docker registry
   login on EC2 once with a read-only package credential. No registry credential
   is passed through the deployment command.
6. Configure API DNS and ports 80/443 as above. SSM needs no inbound SSH rule.

Add these **GitHub repository variables** (Settings → Secrets and variables →
Actions → Variables):

| Variable | Value |
| --- | --- |
| `BACKEND_DEPLOY_TARGET` | `ec2` |
| `BACKEND_DEPLOY_ENABLED` | `true` only after setup is complete |
| `AWS_REGION` | `ap-southeast-2` for the current instance |
| `EC2_INSTANCE_ID` | Your EC2 instance ID (`i-...`), not its public IP |
| `API_HOST` | API domain without scheme/path |
| `EC2_RUNTIME_PARAMETER` | `/interview-platform/production/runtime` |
| `EC2_MIGRATION_PARAMETER` | `/interview-platform/production/migrations` |

EKS variables are not required for this target. Leave deployment disabled until
the parameters, IAM permissions, SSM connectivity and domain are ready. Set both
`BACKEND_DEPLOY_TARGET=ec2` and the enable switch; without the target the existing
EKS path remains selected for backward compatibility.

The pipeline waits for the SSM command and fails if deployment or public readiness
fails. Detailed command output stays in Systems Manager rather than being copied
into GitHub logs. Parameter changes take effect on the next deployment; they do
not themselves trigger GitHub Actions. Keep changes within Parameter Store size
limits. Only one release should run at a time; the remote script also uses a lock.
Do not manually start competing deployments or cancel an active migration.

The manual instructions earlier remain an alternative, not prerequisites for the
automatic route. Avoid mixing manual and automated deployments: they use different
working directories. If migrating an existing manual installation, preserve its
named volumes (same Compose project name), stop its containers first, and verify
all media remains available.

## Automatic prerequisite installation

For a new Ubuntu x86-64 instance, paste the complete contents of `bootstrap.sh`
into **Launch instance → Advanced details → User data**. Attach the EC2 instance
profile during launch. The script installs Docker/Compose, AWS CLI, Python and
starts SSM Agent. It contains no application secrets and does not deploy the app.
For an existing instance, run the same script once using `sudo bash bootstrap.sh`
through SSH or EC2 Instance Connect. Updating User Data alone does not normally
rerun it on an already initialized instance. User Data execution logs are in
`/var/log/cloud-init-output.log`. Agent installation cannot repair missing IAM
permissions: the instance profile still needs `AmazonSSMManagedInstanceCore`
and the scoped Parameter Store permissions described above.

The automatic SSM deployment sends `bootstrap.sh` with the deployment files and
runs it before reading Parameter Store. It installs missing prerequisites and
skips restarting SSM Agent while a Run Command is active. SSM connectivity and
the instance role must already work; this step cannot bootstrap an offline agent.

Backend logs use compact single-line text by default, without terminal escape
codes. Each request has one completion record with method, safe route, request ID,
status and duration. Request-start records require debug logging. Request bodies, credentials and query strings are
not added to request logs.
