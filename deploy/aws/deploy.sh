#!/usr/bin/env bash
# Run with an EKS kubectl context authorized for this namespace.
set -euo pipefail

manifest_dir="${1:?Usage: deploy.sh RENDERED_MANIFEST_DIRECTORY}"
namespace="interview-platform"

# Verify prerequisites before stopping a running release. Never print secrets.
for secret in backend-runtime backend-migrations; do
  kubectl --namespace "$namespace" get secret "$secret" -o name >/dev/null
done
kubectl --namespace "$namespace" apply -f "$manifest_dir/config.yaml"
kubectl --namespace "$namespace" apply -f "$manifest_dir/storage.yaml"

# Stop HTTP and workers before modifying the schema; one replica uses EBS media
# and in-memory signaling. This deliberately trades availability for simplicity.
if kubectl --namespace "$namespace" get deployment backend >/dev/null 2>&1; then
  kubectl --namespace "$namespace" scale deployment backend --replicas=0
  while read -r pod; do
    if [[ -n "$pod" ]]; then
      kubectl --namespace "$namespace" wait --for=delete "$pod" --timeout=90s
    fi
  done < <(kubectl --namespace "$namespace" get pods -l app=backend -o name)
fi

kubectl --namespace "$namespace" create -f "$manifest_dir/migration.yaml"
migration_job="$(kubectl create --dry-run=client -f "$manifest_dir/migration.yaml" -o jsonpath='{.metadata.name}')"
if ! kubectl --namespace "$namespace" wait --for=condition=complete \
  "job/$migration_job" --timeout=660s; then
  echo "Migration did not complete. Backend remains stopped; inspect the migration Job before retrying."
  exit 1
fi

kubectl --namespace "$namespace" apply -f "$manifest_dir/backend.yaml"
kubectl --namespace "$namespace" rollout status deployment/backend --timeout=300s
kubectl --namespace "$namespace" apply -f "$manifest_dir/ingress.yaml"
echo "Backend rollout completed. Verify the public HTTPS health endpoint."
