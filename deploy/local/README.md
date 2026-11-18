# Local Kubernetes with Minikube

Runs one Rust backend replica; React and PostgreSQL remain outside the cluster.
AWS manifests and GitHub deployment workflows are preserved in `deploy/aws` and
`.github/workflows`. These local commands use the dedicated `interview-platform`
Minikube profile, never an EKS context. They do not trigger GitHub deployments.
If AWS deployment should stay paused, set GitHub's `BACKEND_DEPLOY_ENABLED=false`.

## 1. Install and start

Requires working Docker access as your normal user, and enough free memory for a
4 GiB cluster plus the Rust image build. Do not run the script with sudo.
Install Minikube using the [official instructions](https://minikube.sigs.k8s.io/docs/start/).
The script uses Minikube's bundled kubectl; a separate kubectl install is optional.

## 2. Configure privately

From the repository root:

```bash
cp -n deploy/local/runtime.env.example deploy/local/runtime.env
cp -n deploy/local/migrations.env.example deploy/local/migrations.env
chmod 600 deploy/local/runtime.env deploy/local/migrations.env
```

Edit both files privately. Use plain `NAME=value` lines without shell quotes.
They are ignored by Git. Kubernetes Secrets are not a substitute for encryption
at rest; treat access to your local cluster as access to these credentials.

- Runtime: existing restricted `platform_app` database URL, stable invitation
  encryption key, and AI settings. Copy values yourself; the script never reads
  `backend/.env` or changes it.
- Migrations: owner URL for the **same existing database**, using `DATABASE_URL`.
  The migration creates schema/extensions, not the database or runtime role.
- Laptop PostgreSQL: replace `localhost` with `host.minikube.internal` in both
  URLs. Configure PostgreSQL's listening address and narrowly scoped pg_hba.conf
  access for Minikube. A loopback-only PostgreSQL listener cannot serve pods.
- Neon: existing URLs can be used with TLS; migration URLs should use the direct
  endpoint. Migrations modify that database, so select your development database.
- Keep a stable encryption key to retain access to existing invitation links.

See [Minikube host access](https://minikube.sigs.k8s.io/docs/handbook/host-access/).
Stop a locally running Rust backend before deploying against the same database,
to avoid running its workers alongside Kubernetes workers.

## 3. Deploy

```bash
bash deploy/local/deploy.sh
```

This starts the Docker-driver cluster, builds the existing backend Dockerfile
using local Docker and loads the image into Minikube, creates Secrets/configuration/storage, runs the migration Job,
then starts the backend and waits for readiness. No registry or AWS is needed.
The first Rust release build can take several minutes. Re-run after backend
changes; it rebuilds the local image and restarts the pod. Deployments briefly
stop the backend before migrations; migration failure leaves it stopped.

## 4. Connect the frontend

Keep this command running in a separate terminal (local port 8080 must be free):

```bash
minikube -p interview-platform kubectl -- -n interview-platform-local port-forward service/backend 8080:80
```

Then run React normally:

```bash
cd frontend
npm run dev
```

Leave `VITE_API_BASE_URL` empty and use the existing Vite proxy to
`http://127.0.0.1:8080`. Open `http://localhost:5173`.
Check `http://127.0.0.1:8080/health/ready`. Restart port-forward after a pod restart.
The local ConfigMap enables registration; use only for local development.

## Inspect and stop

```bash
minikube -p interview-platform kubectl -- -n interview-platform-local get pods,services,jobs,pvc
minikube -p interview-platform kubectl -- -n interview-platform-local logs deployment/backend --tail=100 -f
minikube -p interview-platform kubectl -- -n interview-platform-local logs job/backend-migrate
minikube -p interview-platform kubectl -- -n interview-platform-local describe pod -l app=backend
minikube stop -p interview-platform
```

Stopping keeps cluster data. Start again with `minikube start -p interview-platform`.
Deleting the profile or its PVC can permanently delete local evidence; PostgreSQL
outside the cluster is independent. Do not delete these resources to pause.

## Files

| File | Purpose |
|---|---|
| `namespace.yaml` | Separates local application resources |
| `config.yaml` | Public server/frontend settings |
| `backend.yaml` | One backend pod, probes, resource limits and ClusterIP Service |
| `storage.yaml` | Minikube default storage, 5 GiB evidence PVC |
| `migration.yaml` | One-shot owner-credential migration Job |
| `deploy.sh` | Explicit-profile build and deployment sequence |
| `*.env.example` | Private Secret input templates |

One replica is intentional: WebRTC signaling is in memory and media uses a local
volume. HTTP and background workers run in the same backend process. This setup
keeps the application's existing behavior and adds no new database migrations.
