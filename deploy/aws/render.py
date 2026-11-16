"""Render non-secret deployment settings without loading environment files."""

import argparse
import re
from pathlib import Path
from urllib.parse import urlsplit


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--image", required=True)
    parser.add_argument("--api-host", required=True)
    parser.add_argument("--certificate-arn", required=True)
    parser.add_argument("--frontend-origin", required=True)
    parser.add_argument("--release", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    if not re.fullmatch(r"ghcr\.io/[a-z0-9/_.-]+@sha256:[a-f0-9]{64}", args.image):
        parser.error("image must be a GHCR image pinned by SHA-256 digest")
    if not re.fullmatch(r"[a-z0-9]+(?:[.-][a-z0-9]+)*", args.api_host):
        parser.error("api-host must be a lowercase DNS hostname, without scheme or path")
    if not re.fullmatch(r"arn:aws:acm:[a-z0-9-]+:[0-9]{12}:certificate/[a-f0-9-]+", args.certificate_arn):
        parser.error("certificate-arn must be an ACM certificate ARN in the cluster region")
    origin = urlsplit(args.frontend_origin)
    if (
        origin.scheme != "https"
        or not origin.hostname
        or origin.netloc != origin.hostname
        or origin.path
        or origin.query
        or origin.fragment
        or not re.fullmatch(r"[a-z0-9]+(?:[.-][a-z0-9]+)*", origin.hostname)
    ):
        parser.error("frontend-origin must be an HTTPS origin without a trailing slash")
    if not re.fullmatch(r"[a-z0-9-]{1,40}", args.release):
        parser.error("release must contain 1–40 lowercase letters, numbers or hyphens")

    replacements = {
        "__IMAGE__": args.image,
        "__CERTIFICATE_ARN__": args.certificate_arn,
        "__API_HOST__": args.api_host,
        "__FRONTEND_ORIGIN__": args.frontend_origin,
        "__RELEASE__": args.release,
    }
    args.output.mkdir(parents=True, exist_ok=True)
    for source in (Path(__file__).parent / "k8s").glob("*.yaml"):
        content = source.read_text()
        for placeholder, value in replacements.items():
            content = content.replace(placeholder, value)
        if re.search(r"__[A-Z_]+__", content):
            parser.error(f"unresolved placeholder in {source.name}")
        (args.output / source.name).write_text(content)


if __name__ == "__main__":
    main()
