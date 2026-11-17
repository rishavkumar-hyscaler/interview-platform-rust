"""Send the checked-out deployment files to one EC2 instance through SSM."""
import base64
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import time


def aws(*arguments):
    result = subprocess.run(
        ["aws", *arguments, "--output", "json"],
        capture_output=True, text=True, check=True,
    )
    return json.loads(result.stdout)


def required(name, pattern):
    value = os.environ.get(name, "")
    if not re.fullmatch(pattern, value):
        raise ValueError(f"Missing or invalid {name}")
    return value


def main():
    instance = required("EC2_INSTANCE_ID", r"i-[0-9a-f]{8,17}")
    settings = {
        "BACKEND_IMAGE": required("IMAGE", r"ghcr\.io/[a-z0-9_./-]+@sha256:[0-9a-f]{64}"),
        "API_HOST": required("API_HOST", r"[a-zA-Z0-9](?:[a-zA-Z0-9.-]*[a-zA-Z0-9])?"),
        "AWS_REGION": required("AWS_REGION", r"[a-z0-9-]+"),
        "RUNTIME_PARAMETER": required("EC2_RUNTIME_PARAMETER", r"/[a-zA-Z0-9_./-]+"),
        "MIGRATION_PARAMETER": required("EC2_MIGRATION_PARAMETER", r"/[a-zA-Z0-9_./-]+"),
    }
    folder = Path(__file__).resolve().parent
    commands = ["set -eu", "umask 077", "mkdir -p /opt/interview-platform"]
    for name in ["compose.yaml", "Caddyfile", "deploy.sh", "remote-deploy.sh"]:
        encoded = base64.b64encode((folder / name).read_bytes()).decode()
        commands.append(
            f"printf %s {shlex.quote(encoded)} | base64 -d > /opt/interview-platform/{name}"
        )
    exports = " ".join(f"{key}={shlex.quote(value)}" for key, value in settings.items())
    commands.append(f"env {exports} bash /opt/interview-platform/remote-deploy.sh")
    result = aws(
        "ssm", "send-command", "--instance-ids", instance,
        "--document-name", "AWS-RunShellScript",
        "--parameters", json.dumps({"commands": ["\n".join(commands)], "executionTimeout": ["1200"]}),
        "--timeout-seconds", "120", "--comment", "Interview platform release",
    )
    command = result["Command"]["CommandId"]
    print(f"Deployment command: {command}", flush=True)
    deadline = time.monotonic() + 1320
    while time.monotonic() < deadline:
        try:
            invocation = aws(
                "ssm", "get-command-invocation", "--command-id", command,
                "--instance-id", instance,
            )
        except subprocess.CalledProcessError as error:
            if "InvocationDoesNotExist" not in error.stderr:
                raise
            time.sleep(5)
            continue
        status = invocation["Status"]
        if status == "Success":
            print("EC2 deployment command succeeded.")
            return
        if status not in {"Pending", "InProgress", "Delayed", "Cancelling"}:
            # Avoid copying runtime output (potential secrets) into GitHub logs.
            raise RuntimeError(f"EC2 deployment {status}; inspect command {command} in AWS Systems Manager")
        time.sleep(5)
    raise TimeoutError(f"Deployment wait timed out; inspect command {command} before starting another release")


if __name__ == "__main__":
    main()
