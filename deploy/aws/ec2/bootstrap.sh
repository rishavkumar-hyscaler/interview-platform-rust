#!/usr/bin/env bash
# EC2 User Data, or run once with: sudo bash bootstrap.sh
set -euo pipefail
export PATH="/usr/local/bin:/usr/bin:/bin:/snap/bin:${PATH:-}"
if [[ "$(id -u)" != 0 ]]; then
  echo 'Run with sudo, or paste this script into EC2 User Data.' >&2
  exit 1
fi
. /etc/os-release
if [[ "$ID" != ubuntu || "$(uname -m)" != x86_64 ]]; then
  echo 'This bootstrap targets Ubuntu x86-64.' >&2
  exit 1
fi
export DEBIAN_FRONTEND=noninteractive
# Install only missing tools; later deployments reuse the installed packages.
if ! command -v curl >/dev/null || ! command -v unzip >/dev/null || \
   ! command -v python3 >/dev/null || ! command -v flock >/dev/null; then
  apt-get -o DPkg::Lock::Timeout=180 update
  apt-get -o DPkg::Lock::Timeout=180 install -y ca-certificates curl unzip python3 util-linux
fi
if ! command -v docker >/dev/null || ! docker compose version >/dev/null 2>&1; then
  install -m 0755 -d /etc/apt/keyrings
  curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
  chmod a+r /etc/apt/keyrings/docker.asc
  cat > /etc/apt/sources.list.d/docker.sources <<DOCKER
Types: deb
URIs: https://download.docker.com/linux/ubuntu
Suites: ${UBUNTU_CODENAME:-$VERSION_CODENAME}
Components: stable
Architectures: amd64
Signed-By: /etc/apt/keyrings/docker.asc
DOCKER
  apt-get -o DPkg::Lock::Timeout=180 update
  apt-get -o DPkg::Lock::Timeout=180 install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
fi
systemctl enable --now docker
if ! command -v aws >/dev/null; then
  bootstrap_tmp="$(mktemp -d)"
  trap 'rm -rf "$bootstrap_tmp"' EXIT
  curl -fsSL https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip -o "$bootstrap_tmp/awscliv2.zip"
  unzip -q "$bootstrap_tmp/awscliv2.zip" -d "$bootstrap_tmp"
  "$bootstrap_tmp/aws/install"
fi
# Never restart the agent executing this Run Command.
if [[ "${1:-}" != --from-ssm ]]; then
# Avoid installing a second SSM Agent when the deb service is already present.
if systemctl list-unit-files amazon-ssm-agent.service --no-legend | grep -q '^amazon-ssm-agent.service'; then
  systemctl enable --now amazon-ssm-agent
  systemctl restart amazon-ssm-agent
else
  apt-get install -y snapd
  if ! snap list amazon-ssm-agent >/dev/null 2>&1; then
    snap install amazon-ssm-agent --classic
  fi
  snap start --enable amazon-ssm-agent
  snap restart amazon-ssm-agent
fi
fi
docker --version
docker compose version
aws --version
echo 'Prerequisites installed. Confirm the EC2 instance role and SSM Online status before deploying.'
