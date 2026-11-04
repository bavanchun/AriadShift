#!/usr/bin/env python3
"""Unit tests for Cloudflare R2 cache pruning, reporting, and CI sccache configuration."""

from __future__ import annotations

import datetime
import importlib.util
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

# Dynamically import r2-cache-prune.py
script_path = os.path.join(os.path.dirname(__file__), "r2-cache-prune.py")
spec = importlib.util.spec_from_file_location("r2_cache_prune", script_path)
if spec is None or spec.loader is None:
    raise ImportError(f"Cannot load spec from {script_path}")
prune_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prune_mod)


class TestR2CachePrune(unittest.TestCase):
    def test_format_bytes(self):
        self.assertEqual(prune_mod.format_bytes(500), "500 B")
        self.assertEqual(prune_mod.format_bytes(1024), "1.00 KiB")
        self.assertEqual(prune_mod.format_bytes(10 * 1024 * 1024), "10.00 MiB")
        self.assertEqual(prune_mod.format_bytes(2 * 1024 * 1024 * 1024), "2.00 GiB")

    def test_get_s3_client_missing_env(self):
        with patch.dict(os.environ, {}, clear=True):
            res = prune_mod.get_s3_client()
            self.assertIsNone(res)

    def test_get_s3_client_prefix_normalization(self):
        env = {
            "SCCACHE_ENDPOINT": "https://fake.r2.cloudflarestorage.com",
            "SCCACHE_BUCKET": "fake-bucket",
            "AWS_ACCESS_KEY_ID": "fake-key",
            "AWS_SECRET_ACCESS_KEY": "fake-secret",
            "SCCACHE_S3_KEY_PREFIX": "custom-prefix",
        }
        with patch.dict(os.environ, env, clear=True):
            with patch("boto3.client") as mock_boto:
                res = prune_mod.get_s3_client()
                self.assertIsNotNone(res)
                client, bucket, prefix = res
                self.assertEqual(prefix, "custom-prefix/")

    def test_apply_lifecycle_rule_handles_access_denied(self):
        from botocore.exceptions import ClientError

        mock_client = MagicMock()
        mock_client.put_bucket_lifecycle_configuration.side_effect = ClientError(
            {"Error": {"Code": "AccessDenied", "Message": "Access Denied"}},
            "PutBucketLifecycleConfiguration",
        )
        # Should not raise exception
        prune_mod.apply_lifecycle_rule(mock_client, "test-bucket", "prefix/")
        mock_client.put_bucket_lifecycle_configuration.assert_called_once()

    def test_lifecycle_skipped_on_report_only(self):
        mock_client = MagicMock()
        mock_setup = (mock_client, "ariadshift-ci-cache", "ariadshift-sccache-v2/")
        with patch.object(prune_mod, "get_s3_client", return_value=mock_setup):
            with patch.object(prune_mod, "apply_lifecycle_rule") as mock_lifecycle:
                with patch.object(prune_mod, "list_objects", return_value=[]):
                    with patch.object(prune_mod, "write_summary"):
                        # In report-only mode, apply_lifecycle_rule must NOT be called
                        prune_mod.main(["--report-only"])
                        mock_lifecycle.assert_not_called()

                        # In prune mode, apply_lifecycle_rule should be called
                        prune_mod.main(["--prune"])
                        mock_lifecycle.assert_called_once_with(mock_client, "ariadshift-ci-cache", "ariadshift-sccache-v2/")

    def test_list_objects(self):
        mock_client = MagicMock()
        mock_paginator = MagicMock()
        mock_client.get_paginator.return_value = mock_paginator
        d1 = datetime.datetime(2026, 11, 1, tzinfo=datetime.timezone.utc)
        d2 = datetime.datetime(2026, 11, 2, tzinfo=datetime.timezone.utc)
        mock_paginator.paginate.return_value = [
            {"Contents": [{"Key": "k1", "Size": 100, "LastModified": d1}]},
            {"Contents": [{"Key": "k2", "Size": 200, "LastModified": d2}]},
        ]
        objs = prune_mod.list_objects(mock_client, "test-bucket", "prefix/")
        self.assertEqual(len(objs), 2)
        self.assertEqual(objs[0]["Key"], "k1")
        self.assertEqual(objs[1]["Size"], 200)

    def test_prune_guard_bucket(self):
        mock_client = MagicMock()
        objs = [{"Key": "k1", "Size": 100, "LastModified": datetime.datetime.now(datetime.timezone.utc)}]
        with self.assertRaises(ValueError) as ctx:
            prune_mod.prune_objects(
                client=mock_client,
                bucket="wrong-bucket",
                prefix="ariadshift-sccache-v2/",
                objects=objs,
                target_bytes=50,
            )
        self.assertIn("Hard guard", str(ctx.exception))
        self.assertIn("ariadshift-ci-cache", str(ctx.exception))

    def test_prune_guard_prefix(self):
        mock_client = MagicMock()
        objs = [{"Key": "k1", "Size": 100, "LastModified": datetime.datetime.now(datetime.timezone.utc)}]
        with self.assertRaises(ValueError) as ctx:
            prune_mod.prune_objects(
                client=mock_client,
                bucket="ariadshift-ci-cache",
                prefix="unauthorized-prefix/",
                objects=objs,
                target_bytes=50,
            )
        self.assertIn("Hard guard", str(ctx.exception))
        self.assertIn("ariadshift-sccache", str(ctx.exception))

    def test_prune_objects_under_budget(self):
        mock_client = MagicMock()
        d1 = datetime.datetime(2026, 11, 1, tzinfo=datetime.timezone.utc)
        objs = [{"Key": "k1", "Size": 100, "LastModified": d1}]
        count, freed = prune_mod.prune_objects(
            client=mock_client,
            bucket="ariadshift-ci-cache",
            prefix="ariadshift-sccache-v2/",
            objects=objs,
            target_bytes=200,
        )
        self.assertEqual(count, 0)
        self.assertEqual(freed, 0)
        mock_client.delete_objects.assert_not_called()

    def test_prune_objects_dry_run(self):
        mock_client = MagicMock()
        d1 = datetime.datetime(2026, 11, 1, tzinfo=datetime.timezone.utc)
        d2 = datetime.datetime(2026, 11, 2, tzinfo=datetime.timezone.utc)
        objs = [
            {"Key": "k1", "Size": 300, "LastModified": d1},
            {"Key": "k2", "Size": 400, "LastModified": d2},
        ]
        count, freed = prune_mod.prune_objects(
            client=mock_client,
            bucket="ariadshift-ci-cache",
            prefix="ariadshift-sccache-v2/",
            objects=objs,
            target_bytes=200,
            dry_run=True,
        )
        self.assertEqual(count, 2)
        self.assertEqual(freed, 700)
        # dry_run must not issue delete calls to S3 API
        mock_client.delete_objects.assert_not_called()

    def test_prune_objects_over_budget_oldest_written(self):
        mock_client = MagicMock()
        d1 = datetime.datetime(2026, 11, 1, tzinfo=datetime.timezone.utc)
        d2 = datetime.datetime(2026, 11, 2, tzinfo=datetime.timezone.utc)
        d3 = datetime.datetime(2026, 11, 3, tzinfo=datetime.timezone.utc)
        objs = [
            {"Key": "newest", "Size": 500, "LastModified": d3},
            {"Key": "oldest", "Size": 300, "LastModified": d1},
            {"Key": "middle", "Size": 400, "LastModified": d2},
        ]
        # Total = 1200. Target = 600. Needs to free 600.
        # Should delete oldest (300) then middle (400) = 700 freed >= 600.
        count, freed = prune_mod.prune_objects(
            client=mock_client,
            bucket="ariadshift-ci-cache",
            prefix="ariadshift-sccache-v2/",
            objects=objs,
            target_bytes=600,
        )
        self.assertEqual(count, 2)
        self.assertEqual(freed, 700)
        mock_client.delete_objects.assert_called_once_with(
            Bucket="ariadshift-ci-cache",
            Delete={"Objects": [{"Key": "oldest"}, {"Key": "middle"}], "Quiet": True},
        )

    def test_write_summary(self):
        with tempfile.NamedTemporaryFile("w+", delete=False) as tf:
            summary_path = tf.name

        try:
            with patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": summary_path}):
                prune_mod.write_summary(
                    bucket="ariadshift-ci-cache",
                    prefix="ariadshift-sccache-v2/",
                    count=42,
                    total_bytes=1024 * 1024 * 50,
                    max_bytes=8_000_000_000,
                    pruned_count=5,
                    pruned_bytes=1024 * 1024 * 10,
                )
            with open(summary_path, "r", encoding="utf-8") as f:
                content = f.read()
            self.assertIn("Cloudflare R2 Build Cache Storage Report", content)
            self.assertIn("ariadshift-ci-cache", content)
            self.assertIn("Pruned (oldest-written)", content)
            self.assertIn("50.00 MiB", content)
        finally:
            os.remove(summary_path)

    def test_ci_sccache_missing_secrets_skip_exit_zero(self):
        sh_path = os.path.join(os.path.dirname(__file__), "ci-sccache.sh")
        env = {
            "PATH": os.environ.get("PATH", ""),
            "SCCACHE_BUCKET": "ariadshift-ci-cache",
            "SCCACHE_ENDPOINT": "https://fake.r2.cloudflarestorage.com",
            # Missing or empty RO/RW keys:
            "AWS_ACCESS_KEY_ID": "",
            "AWS_SECRET_ACCESS_KEY": "",
            "R2_ACCESS_KEY_ID": "",
            "R2_SECRET_ACCESS_KEY": "",
        }
        res = subprocess.run(["bash", sh_path, "start"], env=env, capture_output=True, text=True)
        self.assertEqual(res.returncode, 0)
        self.assertIn("falling back to direct compilation", res.stdout)

    def test_ci_workflow_secret_isolation_and_ro_dispatch(self):
        workflow_path = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "ci.yml"
        content = workflow_path.read_text(encoding="utf-8")

        # Verify all sccache key prefixes use v2
        prefixes = re.findall(r"SCCACHE_S3_KEY_PREFIX:\s*([^\s]+)", content)
        self.assertTrue(len(prefixes) >= 5, "Expected at least 5 prefix references in ci.yml")
        for p in prefixes:
            self.assertEqual(p, "ariadshift-sccache-v2/", f"Prefix {p} must be ariadshift-sccache-v2/")

        # Verify step-level split for read-write vs read-only:
        # Every step providing R2_ACCESS_KEY_ID or R2_SECRET_ACCESS_KEY must have an if: condition
        # restricting execution to push or workflow_dispatch on dev or main.
        rw_cond_pattern = re.compile(
            r"if:.*github\.event_name == 'push' \|\| github\.event_name == 'workflow_dispatch'.*github\.ref_name == 'dev' \|\| github\.ref_name == 'main'"
        )

        steps = re.split(r"\n\s+-\s+name:\s+", content)
        rw_steps_count = 0
        ro_steps_count = 0

        for step in steps:
            has_rw_secret = "secrets.R2_ACCESS_KEY_ID" in step or "secrets.R2_SECRET_ACCESS_KEY" in step
            has_ro_secret = "secrets.R2_RO_ACCESS_KEY_ID" in step or "secrets.R2_RO_SECRET_ACCESS_KEY" in step

            if has_rw_secret:
                rw_steps_count += 1
                self.assertNotIn("secrets.R2_RO_ACCESS_KEY_ID", step)
                self.assertNotIn("secrets.R2_RO_SECRET_ACCESS_KEY", step)
                self.assertTrue(
                    rw_cond_pattern.search(step),
                    f"Read-write step missing required dev/main ref gate:\n{step[:300]}",
                )

            if has_ro_secret:
                ro_steps_count += 1
                self.assertNotIn("secrets.R2_ACCESS_KEY_ID", step)
                self.assertNotIn("secrets.R2_SECRET_ACCESS_KEY", step)
                # Ensure read-only steps are run on PRs and non-mainline refs
                self.assertTrue(
                    "!" in step or "pull_request" in step or "report-only" in step,
                    f"Read-only step should run on non-mainline refs:\n{step[:300]}",
                )

        self.assertGreaterEqual(rw_steps_count, 4, "Expected at least 4 read-write steps (verify, server, fuzz, prune)")
        self.assertGreaterEqual(ro_steps_count, 4, "Expected at least 4 read-only steps (verify, server, fuzz, report)")


if __name__ == "__main__":
    unittest.main()
