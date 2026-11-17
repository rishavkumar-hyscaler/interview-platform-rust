#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# Read deployment settings from the caller's environment or Compose --env-file.
settings_file="${1:?Usage: bash deploy.sh /absolute/path/deployment.env}"
compose=(docker compose --env-file "$settings_file" -f compose.yaml)

"${compose[@]}" config --quiet
"${compose[@]}" --profile tools pull

# Stop workers before changing the schema. A failed migration leaves the old
# backend stopped so it cannot run against a partially changed schema.
"${compose[@]}" stop backend
"${compose[@]}" --profile tools run --rm --no-deps migrate
"${compose[@]}" up -d backend proxy
"${compose[@]}" ps

echo 'Containers started. Verify the public /health/ready endpoint before using the app.'
