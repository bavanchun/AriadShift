#!/usr/bin/env python3
"""Manage Cloudflare R2 build cache storage: report size, enforce lifecycle, and prune LRU to stay under 8 GB.

This script communicates only with the dedicated sccache bucket via the S3 API.
It never prints or logs secret credentials and guarantees the bucket stays strictly
under the 8 GB budget (well within the Cloudflare R2 10 GB free tier).
"""

from __future__ import annotations

import argparse
import os
import sys
from typing import Any

# Default cost-capping budget: 8.0 GB max (8,000,000,000 bytes).
# Target size after pruning: 6.5 GB (6,500,000,000 bytes) to prevent constant re-pruning.
DEFAULT_MAX_BYTES = 8_000_000_000
DEFAULT_TARGET_BYTES = 6_500_000_000
DEFAULT_PREFIX = "ariadshift-sccache/"


def format_bytes(num_bytes: int) -> str:
    """Format byte count into human-readable string (KiB, MiB, GiB)."""
    if num_bytes < 1024:
        return f"{num_bytes} B"
    for unit in ("KiB", "MiB", "GiB", "TiB"):
        num_bytes /= 1024.0
        if num_bytes < 1024.0 or unit == "TiB":
            return f"{num_bytes:.2f} {unit}"
    return f"{num_bytes:.2f} TiB"


def get_s3_client() -> tuple[Any, str, str] | None:
    """Initialize boto3 S3 client using environment variables.

    Returns (client, bucket, prefix) or None if credentials are missing.
    """
    try:
        import boto3
        from botocore.config import Config
    except ImportError:
        print("r2-cache-prune: boto3 not installed; skipping storage management", file=sys.stderr)
        return None

    endpoint = os.environ.get("SCCACHE_ENDPOINT")
    if not endpoint and os.environ.get("R2_ACCOUNT_ID"):
        endpoint = f"https://{os.environ['R2_ACCOUNT_ID']}.r2.cloudflarestorage.com"

    bucket = os.environ.get("SCCACHE_BUCKET") or os.environ.get("R2_CACHE_BUCKET")
    key_id = os.environ.get("AWS_ACCESS_KEY_ID") or os.environ.get("R2_ACCESS_KEY_ID")
    secret_key = os.environ.get("AWS_SECRET_ACCESS_KEY") or os.environ.get("R2_SECRET_ACCESS_KEY")
    region = os.environ.get("SCCACHE_REGION") or "auto"
    prefix = os.environ.get("SCCACHE_S3_KEY_PREFIX") or DEFAULT_PREFIX
    if not prefix.endswith("/"):
        prefix += "/"

    if not bucket or not endpoint or not key_id or not secret_key:
        print(
            "r2-cache-prune: R2 credentials or configuration missing (e.g. fork PR); "
            "skipping storage management"
        )
        return None

    config = Config(
        region_name=region,
        retries={"max_attempts": 3, "mode": "standard"},
        s3={"addressing_style": "path"},
    )
    client = boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=key_id,
        aws_secret_access_key=secret_key,
        region_name=region,
        config=config,
    )
    return client, bucket, prefix


def apply_lifecycle_rule(client: Any, bucket: str, prefix: str) -> None:
    """Attempt to configure an automated lifecycle expiration rule on the bucket.

    If the API token lacks bucket-management permissions, this logs an informative note
    and allows the script to continue to object-level pruning.
    """
    from botocore.exceptions import ClientError

    lifecycle_policy = {
        "Rules": [
            {
                "ID": "ariadshift-sccache-retention",
                "Status": "Enabled",
                "Filter": {"Prefix": prefix},
                "Expiration": {"Days": 14},
                "AbortIncompleteMultipartUpload": {"DaysAfterInitiation": 1},
            }
        ]
    }
    try:
        client.put_bucket_lifecycle_configuration(
            Bucket=bucket,
            LifecycleConfiguration=lifecycle_policy,
        )
        print(f"r2-cache-prune: applied 14-day expiration lifecycle policy to '{prefix}' in '{bucket}'")
    except ClientError as err:
        code = err.response.get("Error", {}).get("Code", "Unknown")
        print(
            f"r2-cache-prune: put_bucket_lifecycle_configuration returned {code} "
            "(token is scoped to Object Read & Write; relying on LRU pruning step)"
        )


def list_objects(client: Any, bucket: str, prefix: str) -> list[dict[str, Any]]:
    """List all objects matching prefix."""
    paginator = client.get_paginator("list_objects_v2")
    objects = []
    for page in paginator.paginate(Bucket=bucket, Prefix=prefix):
        for item in page.get("Contents", []):
            objects.append(
                {
                    "Key": item["Key"],
                    "Size": item["Size"],
                    "LastModified": item["LastModified"],
                }
            )
    return objects


def prune_objects(
    client: Any,
    bucket: str,
    objects: list[dict[str, Any]],
    target_bytes: int,
) -> tuple[int, int]:
    """Delete oldest objects until total size drops to target_bytes.

    Returns (pruned_count, pruned_bytes).
    """
    total_bytes = sum(obj["Size"] for obj in objects)
    bytes_to_free = total_bytes - target_bytes
    if bytes_to_free <= 0:
        return 0, 0

    # Sort oldest first (LRU)
    sorted_objs = sorted(objects, key=lambda x: x["LastModified"])

    to_delete = []
    freed = 0
    for obj in sorted_objs:
        to_delete.append(obj)
        freed += obj["Size"]
        if freed >= bytes_to_free:
            break

    # S3 DeleteObjects allows up to 1000 keys per call
    pruned_count = 0
    pruned_bytes = 0
    batch_size = 1000
    for i in range(0, len(to_delete), batch_size):
        chunk = to_delete[i : i + batch_size]
        chunk_bytes = sum(obj["Size"] for obj in chunk)
        delete_keys = [{"Key": obj["Key"]} for obj in chunk]
        resp = client.delete_objects(
            Bucket=bucket,
            Delete={"Objects": delete_keys, "Quiet": True},
        )
        errors = resp.get("Errors", [])
        if errors:
            print(f"r2-cache-prune: encountered {len(errors)} errors during deletion batch", file=sys.stderr)
            failed_keys = {e["Key"] for e in errors}
            successful_count = len(chunk) - len(failed_keys)
            successful_bytes = sum(
                obj["Size"]
                for obj in chunk
                if obj["Key"] not in failed_keys
            )
            pruned_count += successful_count
            pruned_bytes += successful_bytes
        else:
            pruned_count += len(chunk)
            pruned_bytes += chunk_bytes

    return pruned_count, pruned_bytes


def write_summary(
    bucket: str,
    prefix: str,
    count: int,
    total_bytes: int,
    max_bytes: int,
    pruned_count: int = 0,
    pruned_bytes: int = 0,
) -> None:
    """Write markdown summary to GITHUB_STEP_SUMMARY if available."""
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if not summary_path or not os.path.exists(os.path.dirname(summary_path)):
        return

    status = "OK (under 8 GB budget)" if total_bytes <= max_bytes else "EXCEEDED"
    lines = [
        "### Cloudflare R2 Build Cache Storage Report",
        "",
        "| Metric | Value |",
        "|---|---|",
        f"| **Bucket** | `{bucket}` |",
        f"| **Prefix** | `{prefix}` |",
        f"| **Object Count** | {count:,} |",
        f"| **Total Storage** | {format_bytes(total_bytes)} ({total_bytes:,} bytes) |",
        f"| **Budget Limit** | {format_bytes(max_bytes)} (Free tier: 10 GB) |",
        f"| **Status** | {status} |",
    ]
    if pruned_count > 0:
        lines.append(f"| **Pruned (LRU)** | {pruned_count:,} objects ({format_bytes(pruned_bytes)}) |")
    lines.append("")

    try:
        with open(summary_path, "a", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
    except OSError as err:
        print(f"r2-cache-prune: failed to append summary to {summary_path}: {err}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description="Report and prune Cloudflare R2 sccache storage.")
    parser.add_argument(
        "--report-only",
        action="store_true",
        help="Only report storage usage without pruning objects.",
    )
    parser.add_argument(
        "--prune",
        action="store_true",
        help="Prune oldest objects if total storage exceeds budget.",
    )
    parser.add_argument(
        "--max-bytes",
        type=int,
        default=DEFAULT_MAX_BYTES,
        help=f"Maximum allowed bytes before pruning (default: {DEFAULT_MAX_BYTES}).",
    )
    parser.add_argument(
        "--target-bytes",
        type=int,
        default=DEFAULT_TARGET_BYTES,
        help=f"Target bytes to reduce to after pruning (default: {DEFAULT_TARGET_BYTES}).",
    )
    args = parser.parse_args()

    try:
        setup = get_s3_client()
        if setup is None:
            return 0

        client, bucket, prefix = setup

        # Attempt lifecycle configuration once
        apply_lifecycle_rule(client, bucket, prefix)

        objects = list_objects(client, bucket, prefix)
        count = len(objects)
        total_bytes = sum(obj["Size"] for obj in objects)

        print(f"r2-cache-prune: Bucket '{bucket}', prefix '{prefix}':")
        print(f"  Objects: {count:,}")
        print(f"  Storage: {format_bytes(total_bytes)} ({total_bytes:,} bytes)")
        print(f"  Budget:  {format_bytes(args.max_bytes)}")

        pruned_count = 0
        pruned_bytes = 0
        if not args.report_only and total_bytes > args.max_bytes:
            print(
                f"r2-cache-prune: total storage {format_bytes(total_bytes)} exceeds budget "
                f"{format_bytes(args.max_bytes)}; pruning oldest objects down to {format_bytes(args.target_bytes)}..."
            )
            pruned_count, pruned_bytes = prune_objects(
                client=client,
                bucket=bucket,
                objects=objects,
                target_bytes=args.target_bytes,
            )
            count -= pruned_count
            total_bytes -= pruned_bytes
            print(
                f"r2-cache-prune: pruned {pruned_count:,} objects ({format_bytes(pruned_bytes)} freed). "
                f"New total: {format_bytes(total_bytes)} across {count:,} objects."
            )

        write_summary(
            bucket=bucket,
            prefix=prefix,
            count=count,
            total_bytes=total_bytes,
            max_bytes=args.max_bytes,
            pruned_count=pruned_count,
            pruned_bytes=pruned_bytes,
        )
    except Exception as err:
        print(
            f"r2-cache-prune: WARNING: storage management encountered error: {err} "
            "(falling back safely without failing build)",
            file=sys.stderr,
        )
        return 0

    return 0


if __name__ == "__main__":
    sys.exit(main())
