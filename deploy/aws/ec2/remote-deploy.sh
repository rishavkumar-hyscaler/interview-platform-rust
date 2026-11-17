#!/usr/bin/env bash
set -euo pipefail
umask 077
cd /opt/interview-platform

# SSM commands run as root. Serialize deployments even if a runner disconnects.
exec 9>/opt/interview-platform/deploy.lock
flock -w 1200 9

# Values are fetched on EC2, never sent through GitHub or Run Command parameters.
python3 - <<'PY'
import json
import os
from pathlib import Path
import subprocess

config = Path('/opt/interview-platform/config')
config.mkdir(mode=0o700, exist_ok=True)
for variable, filename in [('RUNTIME_PARAMETER', 'runtime.env'),
                           ('MIGRATION_PARAMETER', 'migrations.env')]:
    result = subprocess.run(
        ['aws', 'ssm', 'get-parameter', '--region', os.environ['AWS_REGION'],
         '--name', os.environ[variable], '--with-decryption', '--output', 'json'],
        check=True, capture_output=True, text=True,
    )
    value = json.loads(result.stdout)['Parameter']['Value']
    if not value.strip() or '\x00' in value:
        raise ValueError('Invalid application configuration parameter')
    target = config / filename
    target.write_text(value.rstrip() + '\n')
    target.chmod(0o600)
PY

export RUNTIME_ENV_FILE=/opt/interview-platform/config/runtime.env
export MIGRATION_ENV_FILE=/opt/interview-platform/config/migrations.env
printf '# Settings supplied by the deployment environment\n' > config/deployment.env
bash deploy.sh /opt/interview-platform/config/deployment.env
