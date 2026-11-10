#!/usr/bin/env python3
"""Exercise WebDriver transport recovery with real dropped HTTP connections."""
import contextlib
import http.client
import http.server
import importlib.util
import json
import pathlib
import socket
import threading
import unittest


spec = importlib.util.spec_from_file_location(
    "desktop_smoke", pathlib.Path(__file__).with_name("desktop-smoke.py"))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


@contextlib.contextmanager
def driver(dispatch):
    class Handler(http.server.BaseHTTPRequestHandler):
        def handle_request(self):
            size = int(self.headers.get("Content-Length", "0"))
            self.rfile.read(size)
            status, value = dispatch(self.command, self.path, self.headers)
            if status is None:
                self.connection.shutdown(socket.SHUT_RDWR)
                self.connection.close()
                return
            body = json.dumps({"value": value}).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        do_GET = handle_request
        do_POST = handle_request

        def log_message(self, *_args):
            pass

    with http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        previous = smoke.URL
        smoke.URL = f"http://127.0.0.1:{server.server_port}"
        try:
            yield
        finally:
            smoke.URL = previous
            server.shutdown()
            thread.join()


class WebDriverTransportTests(unittest.TestCase):
    def test_reads_and_element_lookups_retry_a_dropped_connection(self):
        for method, path in (("GET", "/status"), ("POST", "/session/test/element")):
            with self.subTest(method=method):
                calls = []

                def dispatch(command, endpoint, headers):
                    calls.append((command, endpoint, headers.get("Connection")))
                    return (None, None) if len(calls) == 1 else (200, {"ready": True})

                with driver(dispatch):
                    self.assertEqual(smoke.request(method, path, {}), {"ready": True})
                self.assertEqual(len(calls), 2)
                self.assertTrue(all(call[2] == "close" for call in calls))

    def test_disconnect_retries_are_bounded(self):
        calls = []

        def dispatch(*_args):
            calls.append(True)
            return None, None

        with driver(dispatch):
            with self.assertRaises(http.client.RemoteDisconnected):
                smoke.request("GET", "/status")
        self.assertEqual(len(calls), 3)

    def test_mutating_requests_are_not_replayed(self):
        for path in ("/session", "/session/test/element/button/click",
                     "/session/test/element/input/value"):
            with self.subTest(path=path):
                calls = []

                def dispatch(*_args):
                    calls.append(True)
                    return None, None

                with driver(dispatch):
                    with self.assertRaises(http.client.RemoteDisconnected):
                        smoke.request("POST", path, {})
                self.assertEqual(len(calls), 1)

    def test_http_errors_fail_without_transport_retries(self):
        calls = []

        def dispatch(*_args):
            calls.append(True)
            return 500, {"error": "invalid session id"}

        with driver(dispatch):
            with self.assertRaisesRegex(OSError, "HTTP 500.*invalid session id"):
                smoke.request("GET", "/status")
        self.assertEqual(len(calls), 1)

    def test_lost_click_response_uses_observed_state_without_a_second_click(self):
        clicks = []

        def dispatch(method, path, _headers):
            if path == "/session/test/element":
                return 200, {"element-6066-11e4-a52e-4f735466cecf": "button"}
            if path.endswith("/rect"):
                return 200, {"width": 50, "height": 20}
            if path.endswith("/enabled"):
                return 200, True
            if method == "POST" and path.endswith("/click"):
                clicks.append(True)
                return None, None
            if path == "/completed":
                return 200, bool(clicks)
            return 404, {"error": "unknown command"}

        with driver(dispatch):
            smoke.click("/session/test", "//button", lambda: smoke.request("GET", "/completed"))
        self.assertEqual(len(clicks), 1)


if __name__ == "__main__":
    unittest.main()
