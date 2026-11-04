#!/usr/bin/env python3
"""Unit tests for Cloudflare R2 cache pruning, reporting, and CI sccache configuration."""

from __future__ import annotations

import datetime
import importlib.util
import os
import re
import shutil
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

EXPECTED_PREFIX = "ariadshift-sccache-v2/"


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

    def test_apply_lifecycle_rule_handles_get_access_denied(self):
        from botocore.exceptions import ClientError

        mock_client = MagicMock()
        mock_client.get_bucket_lifecycle_configuration.side_effect = ClientError(
            {"Error": {"Code": "AccessDenied", "Message": "Access Denied"}},
            "GetBucketLifecycleConfiguration",
        )
        # Should not raise exception
        prune_mod.apply_lifecycle_rule(mock_client, "ariadshift-ci-cache", EXPECTED_PREFIX)
        mock_client.get_bucket_lifecycle_configuration.assert_called_once()
        mock_client.put_bucket_lifecycle_configuration.assert_not_called()

    def test_apply_lifecycle_rule_handles_put_access_denied(self):
        from botocore.exceptions import ClientError

        mock_client = MagicMock()
        mock_client.get_bucket_lifecycle_configuration.return_value = {"Rules": []}
        mock_client.put_bucket_lifecycle_configuration.side_effect = ClientError(
            {"Error": {"Code": "AccessDenied", "Message": "Access Denied"}},
            "PutBucketLifecycleConfiguration",
        )
        # Should not raise exception
        prune_mod.apply_lifecycle_rule(mock_client, "ariadshift-ci-cache", EXPECTED_PREFIX)
        mock_client.get_bucket_lifecycle_configuration.assert_called_once()
        mock_client.put_bucket_lifecycle_configuration.assert_called_once()

    def test_lifecycle_guard_bucket(self):
        mock_client = MagicMock()
        with self.assertRaises(ValueError) as ctx:
            prune_mod.apply_lifecycle_rule(
                client=mock_client,
                bucket="wrong-bucket",
                prefix=EXPECTED_PREFIX,
            )
        self.assertIn("Hard guard", str(ctx.exception))
        self.assertIn("ariadshift-ci-cache", str(ctx.exception))

    def test_lifecycle_guard_prefix(self):
        mock_client = MagicMock()
        with self.assertRaises(ValueError) as ctx:
            prune_mod.apply_lifecycle_rule(
                client=mock_client,
                bucket="ariadshift-ci-cache",
                prefix="unauthorized-prefix/",
            )
        self.assertIn("Hard guard", str(ctx.exception))
        self.assertIn("ariadshift-sccache", str(ctx.exception))

    def test_lifecycle_skipped_on_report_only(self):
        mock_client = MagicMock()
        mock_setup = (mock_client, "ariadshift-ci-cache", EXPECTED_PREFIX)
        with patch.object(prune_mod, "get_s3_client", return_value=mock_setup):
            with patch.object(prune_mod, "apply_lifecycle_rule") as mock_lifecycle:
                with patch.object(prune_mod, "list_objects", return_value=[]):
                    with patch.object(prune_mod, "write_summary"):
                        # In report-only mode, apply_lifecycle_rule must NOT be called
                        prune_mod.main(["--report-only"])
                        mock_lifecycle.assert_not_called()

                        # In prune mode, apply_lifecycle_rule should be called
                        prune_mod.main(["--prune"])
                        mock_lifecycle.assert_called_once_with(
                            mock_client, "ariadshift-ci-cache", EXPECTED_PREFIX, dry_run=False
                        )

    def test_lifecycle_skipped_on_dry_run(self):
        mock_client = MagicMock()
        mock_setup = (mock_client, "ariadshift-ci-cache", EXPECTED_PREFIX)
        with patch.object(prune_mod, "get_s3_client", return_value=mock_setup):
            with patch.object(prune_mod, "apply_lifecycle_rule") as mock_lifecycle:
                with patch.object(prune_mod, "list_objects", return_value=[]):
                    with patch.object(prune_mod, "write_summary"):
                        prune_mod.main(["--dry-run"])
                        mock_lifecycle.assert_not_called()

                        prune_mod.main(["--prune", "--dry-run"])
                        mock_lifecycle.assert_not_called()

        # Direct dry_run call must not issue S3 get/put lifecycle requests
        prune_mod.apply_lifecycle_rule(mock_client, "ariadshift-ci-cache", EXPECTED_PREFIX, dry_run=True)
        mock_client.get_bucket_lifecycle_configuration.assert_not_called()
        mock_client.put_bucket_lifecycle_configuration.assert_not_called()

    def test_lifecycle_read_modify_write_preserves_unrelated_rules(self):
        mock_client = MagicMock()
        unrelated_rule = {
            "ID": "other-app-retention",
            "Status": "Enabled",
            "Filter": {"Prefix": "other-app/"},
            "Expiration": {"Days": 30},
        }
        old_sccache_rule = {
            "ID": "ariadshift-sccache-retention",
            "Status": "Enabled",
            "Filter": {"Prefix": "ariadshift-sccache-old/"},
            "Expiration": {"Days": 7},
        }
        mock_client.get_bucket_lifecycle_configuration.return_value = {
            "Rules": [unrelated_rule, old_sccache_rule]
        }

        prune_mod.apply_lifecycle_rule(mock_client, "ariadshift-ci-cache", EXPECTED_PREFIX)

        mock_client.put_bucket_lifecycle_configuration.assert_called_once()
        call_kwargs = mock_client.put_bucket_lifecycle_configuration.call_args[1]
        self.assertEqual(call_kwargs["Bucket"], "ariadshift-ci-cache")
        rules = call_kwargs["LifecycleConfiguration"]["Rules"]
        self.assertEqual(len(rules), 2)
        self.assertIn(unrelated_rule, rules)
        updated_rule = [r for r in rules if r.get("ID") == "ariadshift-sccache-retention"][0]
        self.assertEqual(updated_rule["Filter"]["Prefix"], EXPECTED_PREFIX)
        self.assertEqual(updated_rule["Expiration"]["Days"], 14)

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
                prefix=EXPECTED_PREFIX,
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
            prefix=EXPECTED_PREFIX,
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
            prefix=EXPECTED_PREFIX,
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
            prefix=EXPECTED_PREFIX,
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
                    prefix=EXPECTED_PREFIX,
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
        with tempfile.TemporaryDirectory() as tmpdir:
            stub_sccache = os.path.join(tmpdir, "sccache")
            with open(stub_sccache, "w", encoding="utf-8") as f:
                f.write("#!/bin/sh\nexit 0\n")
            os.chmod(stub_sccache, 0o755)

            env = {
                "PATH": f"{tmpdir}:{os.environ.get('PATH', '')}",
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
            self.assertIn("R2 credentials or configuration missing", res.stdout)
            self.assertIn("falling back to direct compilation", res.stdout)

    def test_ci_sccache_binary_missing_skip_exit_zero(self):
        sh_path = os.path.join(os.path.dirname(__file__), "ci-sccache.sh")
        with tempfile.TemporaryDirectory() as empty_dir:
            bash_path = shutil.which("bash") or "/bin/bash"
            os.symlink(bash_path, os.path.join(empty_dir, "bash"))
            env = {
                "PATH": empty_dir,
                "SCCACHE_BUCKET": "ariadshift-ci-cache",
                "SCCACHE_ENDPOINT": "https://fake.r2.cloudflarestorage.com",
            }
            res = subprocess.run([bash_path, sh_path, "start"], env=env, capture_output=True, text=True)
            self.assertEqual(res.returncode, 0)
            self.assertIn("binary not found in PATH", res.stdout)

    def test_ci_workflow_secret_isolation_and_ro_dispatch(self):
        workflow_path = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "ci.yml"
        content = workflow_path.read_text(encoding="utf-8")

        # Verify all sccache key prefixes use EXPECTED_PREFIX
        prefixes = re.findall(r"SCCACHE_S3_KEY_PREFIX:\s*([^\s]+)", content)
        self.assertTrue(len(prefixes) >= 5, "Expected at least 5 prefix references in ci.yml")
        for p in prefixes:
            self.assertEqual(p, EXPECTED_PREFIX, f"Prefix {p} must be {EXPECTED_PREFIX}")

        # Verify step-level split for read-write vs read-only:
        # Every step providing R2_ACCESS_KEY_ID or R2_SECRET_ACCESS_KEY must have an if: condition
        # restricting execution to push or workflow_dispatch on dev or main branch refs,
        # and must configure SCCACHE_S3_RW_MODE: READ_WRITE.
        # Every step providing R2_RO_ACCESS_KEY_ID or R2_RO_SECRET_ACCESS_KEY must configure
        # SCCACHE_S3_RW_MODE: READ_ONLY and never receive RW secrets.
        rw_cond_pattern = re.compile(
            r"if:.*github\.event_name == 'push' \|\| github\.event_name == 'workflow_dispatch'.*github\.ref == 'refs/heads/dev' \|\| github\.ref == 'refs/heads/main'"
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
                self.assertIn(
                    "SCCACHE_S3_RW_MODE: READ_WRITE",
                    step,
                    f"Read-write step missing SCCACHE_S3_RW_MODE: READ_WRITE:\n{step[:300]}",
                )
                self.assertNotIn("READ_ONLY", step, f"Read-write step must not contain READ_ONLY:\n{step[:300]}")
                self.assertTrue(
                    rw_cond_pattern.search(step),
                    f"Read-write step missing required dev/main ref gate:\n{step[:300]}",
                )

            if has_ro_secret:
                ro_steps_count += 1
                self.assertNotIn("secrets.R2_ACCESS_KEY_ID", step)
                self.assertNotIn("secrets.R2_SECRET_ACCESS_KEY", step)
                self.assertIn(
                    "SCCACHE_S3_RW_MODE: READ_ONLY",
                    step,
                    f"Read-only step missing SCCACHE_S3_RW_MODE: READ_ONLY:\n{step[:300]}",
                )
                self.assertNotIn("READ_WRITE", step, f"Read-only step must not contain READ_WRITE:\n{step[:300]}")
                # Ensure read-only steps are run on PRs and non-mainline refs
                self.assertTrue(
                    "!" in step or "pull_request" in step or "report-only" in step,
                    f"Read-only step should run on non-mainline refs:\n{step[:300]}",
                )

        self.assertEqual(rw_steps_count, 4, "Expected exactly 4 read-write steps (verify, server, prune, fuzz)")
        self.assertEqual(ro_steps_count, 4, "Expected exactly 4 read-only steps (verify, server, report, fuzz)")

    def test_default_prefix_matches_expected(self):
        self.assertEqual(prune_mod.DEFAULT_PREFIX, EXPECTED_PREFIX)


if __name__ == "__main__":
    unittest.main()
