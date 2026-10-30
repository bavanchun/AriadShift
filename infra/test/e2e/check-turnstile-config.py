#!/usr/bin/env python3
"""Render the shared signup/upload challenge configuration without starting services."""
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]
# Use synthetic settings and ignore host challenge variables and .env files.
env = {key: value for key, value in os.environ.items()
       if not key.startswith(("ARIAD_", "AUTH_", "BETTER_AUTH_", "POSTGRES_", "AWS_"))}
env.update({
    "POSTGRES_PASSWORD": "compose-test-postgres",
    "AUTH_DB_PASSWORD": "compose-test-auth",
    "ARIAD_API_DB_PASSWORD": "compose-test-api",
    "ARIAD_WORKER_DB_PASSWORD": "compose-test-worker",
    "S3_ACCESS_KEY_ID": "compose-test-access",
    "S3_SECRET_ACCESS_KEY": "compose-test-s3-secret",
    "BETTER_AUTH_SECRET": "compose-test-auth-secret-at-least-32-bytes",
    "ARIAD_SECRET": "compose-test-api-secret-at-least-32-bytes",
})
for configured in (False, True):
    secret = "compose-test-turnstile-secret" if configured else ""
    site_key = "compose-test-turnstile-site-key" if configured else ""
    settings = {**env, "ARIAD_TURNSTILE_SECRET": secret,
                "ARIAD_TURNSTILE_SITE_KEY": site_key,
                "AUTH_TURNSTILE_SECRET": "must-not-override-shared-secret"}
    result = subprocess.run(
        ["docker", "compose", "--env-file", "/dev/null", "-f",
         str(ROOT / "infra/compose/docker-compose.yml"), "--profile", "core",
         "config", "--format", "json"],
        env=settings, check=True, capture_output=True, text=True,
    )
    services = json.loads(result.stdout)["services"]
    assert services["api"]["environment"]["ARIAD_TURNSTILE_SECRET"] == secret
    assert services["web"]["build"]["args"]["NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY"] == site_key
    assert services["auth"]["environment"].get("AUTH_TURNSTILE_SECRET") == secret, (
        "auth must receive the shared Turnstile secret, including the empty local default"
    )
print("Rendered Compose Turnstile configuration passed (configured and local defaults)")
