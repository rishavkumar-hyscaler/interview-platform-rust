# AWS EKS backend deployment

Target: **standard EKS with an x86-64 managed node group**, AWS Load Balancer
Controller, EBS CSI driver, Neon and Vercel. These manifests do not target EKS Auto
Mode or Fargate. No cloud resources are created by these setup instructions alone.
EKS, worker compute, ALB, EBS and networking can incur AWS charges.

## 1. Prepare EKS (administrator)

- Create an EKS cluster with access entries enabled, VPC CNI networking, and an
  x86-64 managed node group. AWS provisions worker machines; no manual EC2/SSH
  deployment is needed. Use a supported Kubernetes version.
- Install the AWS Load Balancer Controller and Amazon EBS CSI add-on with their
  required IAM permissions (Pod Identity or IRSA). Confirm IngressClass `alb`.
- ALB needs suitable tagged public subnets in at least two Availability Zones.
  Nodes need outbound connectivity to Neon, GHCR and the AI provider. Choose the
  network design deliberately: private subnets commonly require billed NAT egress.
- The GitHub runner must reach the EKS API. For a private-only endpoint, use a
  runner inside the VPC; authentication through OIDC does not provide connectivity.
  For a public endpoint, restrict access using an appropriate runner network design.
- Create the namespace and StorageClass below using administrator credentials.
  The deployment role is scoped to the application namespace and does not create
  cluster-wide resources.
- The encrypted gp3 volume uses `Retain`: deleting its PVC does not automatically
  delete the disk. Back it up and track retained disks to avoid orphaned charges.
  EBS is Availability Zone-bound; replacement nodes must have capacity in that AZ.
- Keep one backend replica while WebRTC signaling is in memory. Persistent media
  survives pod/node replacement when the existing EBS volume can be reattached.

## 2. Prepare Neon

Use a new database or Neon branch for this app. In the Neon SQL editor, provision
the runtime role before migrations (choose a new password privately):

```sql
CREATE ROLE platform_app
    LOGIN
    PASSWORD '<your-private-password>'
    NOSUPERUSER
    NOCREATEDB
    NOCREATEROLE
    NOBYPASSRLS;
```

Do not run the local `backend/infra/postgres/init.sql` against production: it
contains development credentials. The migrations enable pgvector and grant the
runtime permissions. They run using the migration owner's **direct, non-pooled**
Neon URL. Use a TLS URL for `platform_app` as the runtime URL; a direct runtime
connection is the simplest initial choice. Never use Neon's owner role for the API.

The ConfigMap reduces the SQLx pool to five connections and allows 30 seconds for
acquisition, accommodating Neon cold starts. Existing worker polling and readiness
checks regularly access PostgreSQL, so Neon may remain awake and consume compute
allowance. This deployment does not implement idle worker suspension.

## 3. Add secrets on EC2

As a cluster administrator, create the namespace and StorageClass first:

```bash
kubectl apply -f deploy/aws/k8s/namespace.yaml
kubectl apply -f deploy/aws/k8s/storage-class.yaml
```

Create private files **outside the checkout**, with permissions `600`. Do not
commit them, upload them as CI artifacts, or print their contents. Runtime file keys:

```text
DATABASE_URL
INVITATION_ENCRYPTION_KEY
LLM_PROVIDER
LLM_BASE_URL
LLM_MODEL
LLM_API_KEY
EMBEDDING_MODEL
EMBEDDING_API_KEY
WEBRTC_ICE_SERVERS
```

Each line must be `KEY=value`, without shell quotes. Typical non-secret values
are `LLM_PROVIDER=openai`, `LLM_BASE_URL=https://api.openai.com/v1`,
`LLM_MODEL=gpt-5-mini` and `EMBEDDING_MODEL=text-embedding-3-small`.
Keep the invitation key stable: changing it also affects evidence signature
verification. Back it up alongside the database and media. Supply STUN/TURN JSON
under `WEBRTC_ICE_SERVERS` for remote live viewing; TURN service is not provisioned
by this deployment. Values returned to the browser must not be long-lived privileged
TURN credentials.

Both processes use the single setting `DATABASE_URL`. The separate migration
Secret sets it to the owner connection; the runtime Secret sets it to the
restricted `platform_app` connection. The owner credential is never injected into
the API pod. The migration file therefore contains only `DATABASE_URL`. Then run:

```bash
kubectl -n interview-platform create secret generic backend-runtime \
  --from-env-file="$HOME/.config/interview-platform/runtime.env"
kubectl -n interview-platform create secret generic backend-migrations \
  --from-env-file="$HOME/.config/interview-platform/migrations.env"
```

Kubernetes Secrets are not encryption by themselves. Restrict cluster/kubeconfig
access and configure EKS secret encryption and restrict secret access. The app pod receives runtime
secrets only; migration credentials are available only to the migration Job.

Registration defaults to false. For the initial interviewer account, temporarily
set `ALLOW_REGISTRATION: "true"` in the ConfigMap, deploy, register, then restore
false and redeploy. Never ship the local test users/passwords.

## 4. HTTPS and DNS

Request and DNS-validate an ACM certificate for the API hostname **in the same AWS
region as the ALB**. Set its ARN in GitHub as described below. The ALB terminates
TLS using ACM; no `backend-tls` Kubernetes Secret or cert-manager is needed.
After deployment, point the API DNS name to the ALB hostname (Route 53 alias or
CNAME for a subdomain). Ingress creation is asynchronous: a successful pod rollout
does not guarantee the ALB, certificate and DNS are ready. Check public HTTPS.

The ingress redirects HTTP to HTTPS, uses pod IP targets, checks `/health/ready`,
and sets a 300-second idle timeout for streaming. Verify SSE and media uploads.

## 5. GitHub Actions and AWS OIDC

The existing Rust CI runs first. On `main`, a release container is published to
GHCR, then deployment runs only if `BACKEND_DEPLOY_ENABLED=true`.

Create a GitHub environment named `production` restricted to `main`. Add the
GitHub OIDC provider to AWS IAM and a deployment role with `eks:DescribeCluster`
scoped to this cluster. Its trust policy must require audience `sts.amazonaws.com`
and this repository's exact production-environment subject. GitHub's legacy
subject is `repo:OWNER/REPO:environment:production`; newer repositories can use
immutable owner/repository IDs in the subject. Match the repository's actual OIDC
format; do not use a broad wildcard trust condition.

Create an EKS access entry for the deployment role and authorize namespace-scoped
application deployment, e.g. `AmazonEKSEditPolicy` scoped to `interview-platform`.
That role can deploy pods and read application secrets, so protect the environment
and branch. It does not need cluster administrator or AWS administrator permissions.

Configure repository variables:

| Name | Value |
| --- | --- |
| `BACKEND_DEPLOY_ENABLED` | Leave unset until prerequisites are ready, then `true` |
| `AWS_REGION` | Your EKS region |
| `EKS_CLUSTER_NAME` | Cluster name |
| `AWS_DEPLOY_ROLE_ARN` | IAM deployment role trusted through GitHub OIDC |
| `KUBECTL_VERSION` | Pin a kubectl version compatible with the cluster |
| `API_HOST` | API hostname, without scheme/path |
| `ACM_CERTIFICATE_ARN` | Validated ACM certificate ARN in the cluster region |
| `FRONTEND_ORIGIN` | `https://interview-platform-rust-two.vercel.app` |

No AWS access keys, SSH keys, EC2 hostname or stored kubeconfig are required by
this pipeline. AWS credentials and kubeconfig are generated for each run.

Images target `linux/amd64` and Kubernetes uses immutable digest references.
GHCR images are private initially: either deliberately make the image package
public or add `imagePullSecrets` to **both** the backend and migration pod specs
with a namespace-local read-only GHCR pull credential. The default templates
assume public image pull access. Source repository visibility is independent.

## 6. Deploy and verify

Deployment checks Secrets, applies configuration/PVC, stops the old backend and
workers, runs a unique migration Job, and starts one new replica. Brief downtime
is intentional. Migration failure leaves the backend stopped for inspection;
there is no destructive database reset. Releases are serialized.

```bash
kubectl -n interview-platform get pods,jobs,pvc,ingress
kubectl -n interview-platform rollout status deployment/backend
curl --fail https://api.example.com/health/live
curl --fail https://api.example.com/health/ready
```

Set Vercel's `VITE_API_BASE_URL` to the API HTTPS origin and redeploy the frontend.
Verify login, invitations, answers, media, scoring, chat streaming and WebRTC from
separate networks. Runtime AI activity uses your provider account.

## Rollback and costs

```bash
kubectl -n interview-platform rollout history deployment/backend
kubectl -n interview-platform rollout undo deployment/backend
kubectl -n interview-platform rollout status deployment/backend
```

Rollback changes the binary, not database schema, ConfigMap or Secret values.
Check compatibility before rolling back; otherwise fix forward or restore a
coordinated backup. Preserve the invitation/signing key with database/media backups.
In-memory live viewing reconnects after restarts. Completed migration Jobs expire
after one day. Retained EBS disks need deliberate lifecycle management.

Monitor EKS, nodes, ALB, EBS, public IPv4, NAT (if used), data transfer, Neon and AI
usage. GitHub Actions free allowance does not make AWS infrastructure free. There
is no autoscaling or TURN deployment here. Shared signaling and storage changes
are required before increasing backend replicas.
