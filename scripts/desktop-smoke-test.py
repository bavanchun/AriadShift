#!/usr/bin/env python3
"""Unit tests for transient disconnect recovery in scripts/desktop-smoke.py."""

import contextlib
import http.client
import importlib.util
import json
import pathlib
import unittest
import urllib.error
from unittest import mock


def _load_smoke_module():
    path = pathlib.Path(__file__).with_name("desktop-smoke.py")
    spec = importlib.util.spec_from_file_location("desktop_smoke", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


smoke = _load_smoke_module()


class FakeClock:
    """Deterministic monotonic clock and sleep stub for bounded timeout tests."""

    def __init__(self, initial=100.0):
        self.start = initial
        self.now = initial
        self.sleeps = []

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.now += max(0.0, seconds)

    @property
    def elapsed(self):
        return self.now - self.start


def _element_response(ident):
    return {"element-6066-11e4-a52e-4f735466cecf": ident}


class FakeWebDriver:
    """Minimal request router for element lookup, enabled checks, and click handling."""

    def __init__(self, on_click, on_find=None, on_enabled=None):
        self.on_click = on_click
        self.on_find = on_find
        self.on_enabled = on_enabled
        self.counter = 0
        self.clicked_ids = []

    def __call__(self, method, path, body=None):
        if method == "POST" and path == "/session/s1/element":
            if self.on_find is not None:
                found = self.on_find(body["value"])
                if found is not None:
                    return found
            self.counter += 1
            return _element_response(f"elem-{self.counter}")
        if method == "GET" and path.endswith("/rect"):
            return {"width": 80, "height": 32}
        if method == "GET" and path.endswith("/enabled"):
            return True if self.on_enabled is None else self.on_enabled()
        if method == "POST" and path.endswith("/click"):
            ident = path.split("/")[-2]
            self.clicked_ids.append(ident)
            return self.on_click(ident, len(self.clicked_ids))
        raise AssertionError(f"Unexpected request: {method} {path}")


class DesktopSmokeClickRetryTests(unittest.TestCase):
    def _patch_clock(self, clock):
        stack = contextlib.ExitStack()
        stack.enter_context(mock.patch.object(smoke.time, "monotonic", side_effect=clock.monotonic))
        stack.enter_context(mock.patch.object(smoke.time, "sleep", side_effect=clock.sleep))
        return stack

    def _run_click(self, driver, completed, xpath="//button[normalize-space()='Add file']"):
        clock = FakeClock()
        with self._patch_clock(clock), mock.patch.object(smoke, "request", side_effect=driver):
            result = smoke.click("/session/s1", xpath, completed)
        return result, clock

    def test_timeout_constant_remains_30_seconds(self):
        self.assertEqual(smoke.TIMEOUT, 30)

    def test_request_does_not_replay_mutating_click(self):
        calls = 0

        def fake_urlopen(_req, timeout=None):
            nonlocal calls
            calls += 1
            raise http.client.RemoteDisconnected("peer closed")

        clock = FakeClock()
        with self._patch_clock(clock), mock.patch.object(
            smoke.urllib.request, "urlopen", side_effect=fake_urlopen
        ), self.assertRaises(http.client.RemoteDisconnected):
            smoke.request("POST", "/session/s1/element/elem-1/click", {})

        self.assertEqual(calls, 1)
        self.assertEqual(clock.sleeps, [])

    def test_dropped_click_waits_grace_window_and_retries_with_fresh_element_id(self):
        is_completed = False

        def on_find(xpath):
            if xpath != "//done":
                return None
            if not is_completed:
                raise OSError("WebDriver POST /session/s1/element: HTTP 404: no such element")
            return _element_response("done-1")

        def on_click(_ident, attempt):
            nonlocal is_completed
            if attempt == 1:
                raise urllib.error.URLError(ConnectionResetError(104, "Connection reset by peer"))
            is_completed = True

        driver = FakeWebDriver(on_click=on_click, on_find=on_find)
        result, clock = self._run_click(driver, lambda: smoke.element("/session/s1", "//done"))
        self.assertEqual(result, "done-1")
        self.assertEqual(driver.clicked_ids, ["elem-1", "elem-2"])
        self.assertGreaterEqual(clock.elapsed, smoke.DISCONNECT_GRACE_SECONDS - 1e-9)
        self.assertLess(clock.elapsed, 2.5)

    def test_details_summary_uses_closed_predicate_so_open_details_is_never_toggled_closed(self):
        details_open = False
        file_input_laid_out = False

        def on_find(xpath):
            if xpath == "//details[not(@open)]/summary[normalize-space()='Add a local file by path']":
                if details_open:
                    raise OSError("WebDriver POST /session/s1/element: HTTP 404: already open")
                return None
            if xpath == "//details[@open]//input[@id='file-path']":
                if not details_open:
                    raise OSError("WebDriver POST /session/s1/element: HTTP 404: still closed")
                return _element_response("file-input-1")
            return None

        def on_click(_ident, _attempt):
            nonlocal details_open
            details_open = True
            raise http.client.RemoteDisconnected("peer closed")

        driver = FakeWebDriver(on_click=on_click, on_find=on_find)
        clock = FakeClock()

        def completed():
            nonlocal file_input_laid_out
            if clock.elapsed >= 1.8:
                file_input_laid_out = True
            if not file_input_laid_out:
                return None
            return smoke.element("/session/s1", "//details[@open]//input[@id='file-path']")

        with self._patch_clock(clock), mock.patch.object(smoke, "request", side_effect=driver):
            result = smoke.click(
                "/session/s1",
                "//details[not(@open)]/summary[normalize-space()='Add a local file by path']",
                completed,
            )

        self.assertEqual(result, "file-input-1")
        self.assertEqual(driver.clicked_ids, ["elem-1"])

    def test_click_applied_before_disconnect_confirms_during_grace_without_reclicking(self):
        completed_polls = 0

        def on_click(_ident, _attempt):
            raise http.client.RemoteDisconnected("Remote end closed connection without response")

        def completed():
            nonlocal completed_polls
            completed_polls += 1
            return "file-input-1" if completed_polls >= 3 else None

        driver = FakeWebDriver(on_click=on_click)
        result, clock = self._run_click(
            driver,
            completed,
            xpath="//details[not(@open)]/summary[normalize-space()='Add a local file by path']",
        )
        self.assertEqual(result, "file-input-1")
        self.assertEqual(driver.clicked_ids, ["elem-1"])
        self.assertLess(clock.elapsed, smoke.DISCONNECT_GRACE_SECONDS)

    def test_slow_async_transition_confirms_without_second_click_while_button_disabled(self):
        clock = FakeClock()
        driver = FakeWebDriver(
            on_click=lambda _id, _n: (_ for _ in ()).throw(
                ConnectionResetError(104, "Connection reset by peer")
            ),
            on_enabled=lambda: len(driver.clicked_ids) == 0,
        )

        def completed():
            if clock.elapsed < 2.1:
                raise OSError("WebDriver POST /session/s1/element: HTTP 404: no such element")
            return "queue-h1"

        with self._patch_clock(clock), mock.patch.object(smoke, "request", side_effect=driver):
            result = smoke.click("/session/s1", "//button[@aria-label='Start conversion']", completed)

        self.assertEqual(result, "queue-h1")
        self.assertEqual(driver.clicked_ids, ["elem-1"])

    def test_exhausted_disconnect_retries_time_out_at_30s_budget(self):
        driver = FakeWebDriver(
            on_click=lambda _id, _n: (_ for _ in ()).throw(
                http.client.RemoteDisconnected("peer reset")
            )
        )
        clock = FakeClock()

        def completed():
            raise OSError("WebDriver POST /session/s1/element: HTTP 404: no such element")

        with self._patch_clock(clock), mock.patch.object(
            smoke, "request", side_effect=driver
        ), self.assertRaises(RuntimeError) as ctx:
            smoke.click("/session/s1", "//button", completed)

        self.assertIn("Timed out waiting for desktop UI", str(ctx.exception))
        expected_clicks = 1 + smoke.MAX_CLICK_DISCONNECT_RETRIES
        self.assertEqual(driver.clicked_ids, [f"elem-{i}" for i in range(1, expected_clicks + 1)])
        self.assertAlmostEqual(clock.elapsed, smoke.TIMEOUT, places=6)

    def test_non_disconnect_errors_propagate_immediately_without_retry(self):
        for exc in (
            urllib.error.URLError(ConnectionRefusedError(111, "Connection refused")),
            OSError("WebDriver POST /session/s1/element/elem-1/click: HTTP 500: internal error"),
        ):
            with self.subTest(exc=type(exc).__name__):
                def on_click(_ident, _attempt, error=exc):
                    raise error

                driver = FakeWebDriver(on_click=on_click)
                clock = FakeClock()
                with self._patch_clock(clock), mock.patch.object(
                    smoke, "request", side_effect=driver
                ), self.assertRaises(type(exc)):
                    smoke.click("/session/s1", "//button", lambda: None)

                self.assertEqual(driver.clicked_ids, ["elem-1"])
                self.assertEqual(clock.elapsed, 0.0)

    def test_budget_nearly_exhausted_clamps_grace_and_does_not_click_past_deadline(self):
        clock = FakeClock()

        def on_find(_xpath):
            if clock.elapsed < 29.2:
                raise OSError("WebDriver POST /session/s1/element: HTTP 404: not ready")
            return _element_response("elem-late")

        driver = FakeWebDriver(
            on_find=on_find,
            on_click=lambda _id, _n: (_ for _ in ()).throw(
                http.client.RemoteDisconnected("peer reset")
            ),
        )
        with self._patch_clock(clock), mock.patch.object(
            smoke, "request", side_effect=driver
        ), self.assertRaises(RuntimeError) as ctx:
            smoke.click("/session/s1", "//button", lambda: None)

        self.assertIn("peer reset", str(ctx.exception))
        self.assertEqual(driver.clicked_ids, ["elem-late"])
        self.assertAlmostEqual(clock.elapsed, smoke.TIMEOUT, places=6)

    def test_click_does_not_fire_after_slow_ready_crosses_deadline_and_request_inherits_deadline(self):
        clock = FakeClock()
        seen_timeouts = []
        clicked = 0

        class _FakeResp:
            def __init__(self, payload):
                self.payload = payload

            def read(self):
                return json.dumps({"value": self.payload}).encode()

            def __enter__(self):
                return self

            def __exit__(self, *args):
                return False

        def fake_urlopen(req, timeout=None):
            nonlocal clicked
            seen_timeouts.append(timeout)
            if req.full_url.endswith("/element"):
                clock.now += 15.0
                return _FakeResp(_element_response("elem-slow"))
            if req.full_url.endswith("/rect"):
                clock.now += 10.0
                return _FakeResp({"width": 10, "height": 10})
            if req.full_url.endswith("/enabled"):
                clock.now += 5.0
                return _FakeResp(True)
            if req.full_url.endswith("/click"):
                clicked += 1
            raise AssertionError(f"Unexpected call {req.full_url}")

        with self._patch_clock(clock), mock.patch.object(
            smoke.urllib.request, "urlopen", side_effect=fake_urlopen
        ), self.assertRaises(RuntimeError):
            smoke.click("/session/s1", "//button", lambda: None)

        self.assertEqual(clicked, 0)
        self.assertLessEqual(seen_timeouts[1], 15.001)
        self.assertLessEqual(seen_timeouts[2], 5.001)


if __name__ == "__main__":
    unittest.main()
