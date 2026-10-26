#!/usr/bin/env python3
"""Regression checks for credential selection and polluted CLI output."""

import contextlib
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "github_token", Path(__file__).with_name("with-github-token.py")
)
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


class TokenTests(unittest.TestCase):
    def environment(self, env, stdout=b"fixture_token\n", stderr=b"", rc=0):
        result = subprocess.CompletedProcess(["gh"], rc, stdout, stderr)
        with (
            patch.dict(os.environ, env, clear=True),
            patch.object(helper.subprocess, "run", return_value=result) as run,
        ):
            captured = io.StringIO()
            with contextlib.redirect_stdout(captured), contextlib.redirect_stderr(captured):
                actual = helper.token_environment()
            self.assertNotIn("fixture_token", captured.getvalue())
            return actual, run

    def test_environment_precedence(self):
        env, run = self.environment(
            {"GH_TOKEN": "fixture_token", "GITHUB_TOKEN": "other_token"}
        )
        self.assertEqual(env["GH_TOKEN"], "fixture_token")
        self.assertNotIn("GITHUB_TOKEN", env)
        run.assert_not_called()

    def test_secondary_environment(self):
        env, run = self.environment(
            {"GH_TOKEN": "bad header", "GITHUB_TOKEN": "fixture_token"}
        )
        self.assertEqual(env["GH_TOKEN"], "fixture_token")
        run.assert_not_called()

    def test_cli_stderr_is_separate_and_environment_is_clean(self):
        env, run = self.environment(
            {"GH_TOKEN": "bad\nheader", "GITHUB_TOKEN": "bad\theader"},
            stderr=b"mise status\n",
        )
        self.assertEqual(env["GH_TOKEN"], "fixture_token")
        self.assertNotIn("GH_TOKEN", run.call_args.kwargs["env"])
        self.assertNotIn("GITHUB_TOKEN", run.call_args.kwargs["env"])

    def test_rejects_polluted_or_empty_cli_output(self):
        for output in (
            b"mise status\nfixture_token\n", b" fixture_token\n",
            b"fixture_token\n\n", b"fixture_token\x00\n",
            b"fixture_token\t\n", b"fixture_token\r", b"", b"\xff\n",
        ):
            with self.subTest(output=output), self.assertRaises(RuntimeError):
                self.environment({}, stdout=output)

    def test_cli_failure_does_not_expose_output(self):
        with self.assertRaises(RuntimeError) as error:
            self.environment({}, rc=1, stderr=b"fixture_token")
        self.assertNotIn("fixture_token", str(error.exception))

    def test_normal_cli_line_endings(self):
        for output in (b"fixture_token", b"fixture_token\n", b"fixture_token\r\n"):
            with self.subTest(output=output):
                env, _ = self.environment({}, stdout=output)
                self.assertEqual(env["GH_TOKEN"], "fixture_token")

    def test_missing_cli_has_clear_error(self):
        with (
            patch.dict(os.environ, {}, clear=True),
            patch.object(helper.subprocess, "run", side_effect=FileNotFoundError),
            self.assertRaisesRegex(RuntimeError, "Cannot run gh"),
        ):
            helper.token_environment()

    def test_launcher_rejects_pollution_without_leaking_it(self):
        result = subprocess.CompletedProcess([], 0, b"mise status\nfixture_token\n", b"")
        captured = io.StringIO()
        with (
            patch.dict(os.environ, {}, clear=True),
            patch.object(helper.subprocess, "run", return_value=result),
            patch.object(helper.os, "execvpe") as execute,
            patch.object(helper.sys, "argv", ["helper", "uvx"]),
            contextlib.redirect_stdout(captured),
            contextlib.redirect_stderr(captured),
        ):
            self.assertEqual(helper.main(), 1)
        execute.assert_not_called()
        self.assertIn("malformed output", captured.getvalue())
        self.assertNotIn("fixture_token", captured.getvalue())

    def test_launcher_forwards_command_and_clean_environment(self):
        with (
            patch.object(helper, "token_environment", return_value={"GH_TOKEN": "fixture_token"}),
            patch.object(helper.os, "execvpe") as execute,
            patch.object(helper.sys, "argv", ["helper", "uvx", "zizmor@1.30.1", ".github"]),
        ):
            helper.main()
        execute.assert_called_once_with(
            "uvx", ["uvx", "zizmor@1.30.1", ".github"], {"GH_TOKEN": "fixture_token"}
        )


if __name__ == "__main__":
    unittest.main()
