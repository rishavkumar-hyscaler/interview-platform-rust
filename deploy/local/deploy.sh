#!/usr/bin/env bash
set -euo pipefail

local_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "$local_dir/../.." && pwd)"
profile="interview-platform"
namespace="interview-platform-local"

command -v docker >/dev/null || { echo "Install Docker first." >&2; exit 1; }
command -v minikube >/dev/null || { echo "Install Minikube first; see deploy/local/README.md." >&2; exit 1; }
for env_file in runtime.env migrations.env; do
  if [[ ! -f "$local_dir/$env_file" ]]; then
    echo "Create deploy/local/$env_file from its .example before deploying." >&2
    exit 1
  fi
done

# Explicit profile/context keeps this script away from an AWS Kubernetes cluster.
minikube start -p "$profile" --driver=docker --cpus=2 --memory=4096
kube=(minikube -p "$profile" kubectl --)
# Build on the host with an explicit Dockerfile, then import into Minikube.
# Some Minikube/containerd builds can report failure but return success.
if ! docker build \
  --file "$repo_dir/backend/Dockerfile" \
  --tag interview-platform-backend:local \
  "$repo_dir/backend"; then
  echo "Backend image build failed; deployment stopped." >&2
  exit 1
fi
if ! minikube -p "$profile" image load interview-platform-backend:local; then
  echo "Could not load the backend image; deployment stopped." >&2
  exit 1
fi
"${kube[@]}" apply -f "$local_dir/namespace.yaml"
for kind in runtime migrations; do
  "${kube[@]}" -n "$namespace" create secret generic "backend-$kind" \
    --from-env-file="$local_dir/$kind.env" --dry-run=client -o yaml |
    "${kube[@]}" apply -f - >/dev/null
done
"${kube[@]}" apply -f "$local_dir/config.yaml" -f "$local_dir/storage.yaml"

# Stop background workers before changing the shared schema.
if "${kube[@]}" -n "$namespace" get deployment backend >/dev/null 2>&1; then
  "${kube[@]}" -n "$namespace" scale deployment backend --replicas=0
  "${kube[@]}" -n "$namespace" wait --for=delete pod -l app=backend --timeout=120s
fi
"${kube[@]}" -n "$namespace" delete job backend-migrate --ignore-not-found --wait=true
"${kube[@]}" apply -f "$local_dir/migration.yaml"
# Poll for Complete or Failed; `wait --for=condition=complete` alone blocks
# for the full timeout when the Job fails fast (e.g. database unreachable).
migration_state=""
for ((i = 0; i < 300; i++)); do
  conditions="$("${kube[@]}" -n "$namespace" get job backend-migrate \
    -o jsonpath='{range .status.conditions[?(@.status=="True")]}{.type} {end}')"
  case " $conditions " in
    *" Complete "*) migration_state=complete; break ;;
    *" Failed "*) migration_state=failed; break ;;
  esac
  sleep 2
done
if [[ "$migration_state" != complete ]]; then
  "${kube[@]}" -n "$namespace" logs job/backend-migrate
  echo "Migration failed; backend remains stopped. Correct the error and rerun." >&2
  exit 1
fi
"${kube[@]}" apply -f "$local_dir/backend.yaml"
"${kube[@]}" -n "$namespace" rollout restart deployment/backend
"${kube[@]}" -n "$namespace" rollout status deployment/backend --timeout=240s
printf '\nBackend ready. Run the port-forward command in deploy/local/README.md.\n'
