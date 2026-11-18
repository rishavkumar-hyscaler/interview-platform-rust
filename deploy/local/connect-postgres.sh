#!/usr/bin/env bash
# Run on your Ubuntu laptop as your normal user; sudo prompts locally.
set -euo pipefail

profile=interview-platform
host_address="$(minikube -p "$profile" ssh -- 'getent hosts host.minikube.internal' | awk 'NR == 1 {print $1}' | tr -d '\r')"
node_address="$(minikube -p "$profile" ip | tr -d '\r')"
for address in "$host_address" "$node_address"; do
  if [[ ! "$address" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]]; then
    echo "Could not resolve Minikube IPv4 addresses." >&2
    exit 1
  fi
done

config_file="$(sudo -u postgres psql -XAtc 'SHOW config_file')"
hba_file="$(sudo -u postgres psql -XAtc 'SHOW hba_file')"
listen_addresses="$(sudo -u postgres psql -XAtc 'SHOW listen_addresses')"
backup_suffix="minikube-backup-$(date +%Y%m%d%H%M%S)"
sudo cp -- "$config_file" "$config_file.$backup_suffix"
sudo cp -- "$hba_file" "$hba_file.$backup_suffix"

# Preserve existing listeners and add only the Docker host address.
if [[ "$listen_addresses" != '*' && ",$listen_addresses," != *",$host_address,"* ]]; then
  sudo -u postgres psql -X -v ON_ERROR_STOP=1 \
    -c "ALTER SYSTEM SET listen_addresses = '$listen_addresses,$host_address';"
fi

# Only the Minikube node, this database and the two application roles.
rule="host interview-platform postgres,platform_app $node_address/32 scram-sha-256"
if ! sudo grep -Fxq "$rule" "$hba_file"; then
  printf '\n# Local Minikube application access\n%s\n' "$rule" | sudo tee -a "$hba_file" >/dev/null
fi

# Restart just the PostgreSQL cluster serving the default local connection.
cluster_dir="$(dirname "$config_file")"
cluster_name="$(basename "$cluster_dir")"
cluster_version="$(basename "$(dirname "$cluster_dir")")"
sudo pg_ctlcluster "$cluster_version" "$cluster_name" restart
printf '\nPostgreSQL configured for Minikube node %s via %s:5432.\n' "$node_address" "$host_address"
printf 'Configuration backups: %s.%s and %s.%s\n' "$config_file" "$backup_suffix" "$hba_file" "$backup_suffix"
printf 'Existing localhost access is retained. Rerun deploy/local/deploy.sh.\n'
