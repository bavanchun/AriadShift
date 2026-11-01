#!/usr/bin/env python3
"""Regression tests for the rendered auth Turnstile site key check."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "check_turnstile_config", Path(__file__).with_name("check-turnstile-config.py")
)
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class TurnstileConfigTest(unittest.TestCase):
    def test_auth_receives_shared_site_key_and_rejects_missing_or_wrong_values(self):
        for configured in (False, True):
            with self.subTest(configured=configured):
                services, _, site_key = check.render_services(configured)
                check.check_auth_site_key(services, site_key)
                auth_env = services["auth"]["environment"]
                del auth_env["AUTH_TURNSTILE_SITE_KEY"]
                with self.assertRaises(AssertionError):
                    check.check_auth_site_key(services, site_key)
                auth_env["AUTH_TURNSTILE_SITE_KEY"] = "wrong-key"
                with self.assertRaises(AssertionError):
                    check.check_auth_site_key(services, site_key)


if __name__ == "__main__":
    unittest.main()
