#!/usr/bin/env python3
"""Unit tests for Cloudflare R2 cache pruning and reporting script."""

from __future__ import annotations

import datetime
import importlib.util
import os
import sys
import tempfile
import unittest
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

    def test_prune_objects_under_budget(self):
        mock_client = MagicMock()
        d1 = datetime.datetime(2026, 11, 1, tzinfo=datetime.timezone.utc)
        objs = [{"Key": "k1", "Size": 100, "LastModified": d1}]
        count, freed = prune_mod.prune_objects(mock_client, "b", objs, target_bytes=200)
        self.assertEqual(count, 0)
        self.assertEqual(freed, 0)
        mock_client.delete_objects.assert_not_called()

    def test_prune_objects_over_budget_lru(self):
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
        count, freed = prune_mod.prune_objects(mock_client, "b", objs, target_bytes=600)
        self.assertEqual(count, 2)
        self.assertEqual(freed, 700)
        mock_client.delete_objects.assert_called_once_with(
            Bucket="b",
            Delete={"Objects": [{"Key": "oldest"}, {"Key": "middle"}], "Quiet": True},
        )

    def test_write_summary(self):
        with tempfile.NamedTemporaryFile("w+", delete=False) as tf:
            summary_path = tf.name

        try:
            with patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": summary_path}):
                prune_mod.write_summary(
                    bucket="test-bucket",
                    prefix="sccache/",
                    count=42,
                    total_bytes=1024 * 1024 * 50,
                    max_bytes=8_000_000_000,
                    pruned_count=5,
                    pruned_bytes=1024 * 1024 * 10,
                )
            with open(summary_path, "r", encoding="utf-8") as f:
                content = f.read()
            self.assertIn("Cloudflare R2 Build Cache Storage Report", content)
            self.assertIn("test-bucket", content)
            self.assertIn("Pruned (LRU)", content)
            self.assertIn("50.00 MiB", content)
        finally:
            os.remove(summary_path)


if __name__ == "__main__":
    unittest.main()
