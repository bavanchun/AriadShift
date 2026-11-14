#!/usr/bin/env python3
"""Exercise the real desktop UI through tauri-driver (Linux/WebKitGTK only)."""
import contextvars
import http.client
import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

URL = "http://127.0.0.1:4444"
TIMEOUT = 30
POLL_INTERVAL = 0.15
DISCONNECT_GRACE_SECONDS = 1.5
MAX_CLICK_DISCONNECT_RETRIES = 2
DISCONNECT_ERRORS = (http.client.RemoteDisconnected, ConnectionResetError, urllib.error.URLError)
_TIMED_OUT = object()
_ACTIVE_DEADLINE = contextvars.ContextVar("desktop_smoke_deadline", default=None)


def _scoped_deadline(deadline):
    current = _ACTIVE_DEADLINE.get()
    effective = deadline if current is None else min(current, deadline)
    return effective, _ACTIVE_DEADLINE.set(effective)


def disconnected(error):
    if isinstance(error, urllib.error.URLError):
        error = error.reason
    return isinstance(error, (http.client.RemoteDisconnected, ConnectionResetError))


def request(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(URL + path, data=data, method=method,
                                 headers={"Content-Type": "application/json", "Connection": "close"})
    # Element lookup is a read despite using POST. Never replay session creation,
    # clicks or key events inside request(); click() checks state before retrying.
    retryable = method == "GET" or (method == "POST" and path.endswith("/element"))
    active_deadline = _ACTIVE_DEADLINE.get()
    deadline = time.monotonic() + TIMEOUT
    if active_deadline is not None:
        deadline = min(deadline, active_deadline)
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req, timeout=max(0.001, deadline - time.monotonic())) as response:
                return json.load(response)["value"]
        except urllib.error.HTTPError as error:
            with error:
                detail = error.read().decode("utf-8", errors="replace")
            raise OSError(f"WebDriver {method} {path}: HTTP {error.code}: {detail}") from error
        except DISCONNECT_ERRORS as error:
            if not retryable or not disconnected(error) or attempt == 2 or time.monotonic() >= deadline:
                raise
            time.sleep(min(POLL_INTERVAL * (attempt + 1), max(0, deadline - time.monotonic())))


def _timeout_error(last_error):
    return RuntimeError(f"Timed out waiting for desktop UI: {last_error}")


def _poll_until(action, deadline, last_error=None):
    """Poll ``action`` until truthy or ``deadline`` expires, returning ``(_TIMED_OUT, last_error)`` on timeout."""
    while True:
        try:
            value = action()
            if value:
                return value, last_error
        except (OSError, KeyError) as error:
            last_error = error
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return _TIMED_OUT, last_error
        time.sleep(min(POLL_INTERVAL, remaining))


def _wait_until_deadline(action, deadline, last_error=None):
    """Wait until ``action`` returns truthy before ``deadline`` or raise RuntimeError."""
    value, last_error = _poll_until(action, deadline, last_error=last_error)
    if value is _TIMED_OUT:
        raise _timeout_error(last_error)
    return value


def until(action, timeout=TIMEOUT, last_error=None):
    deadline, token = _scoped_deadline(time.monotonic() + timeout)
    try:
        return _wait_until_deadline(action, deadline, last_error=last_error)
    finally:
        _ACTIVE_DEADLINE.reset(token)


def element(prefix, xpath):
    ident = request("POST", prefix + "/element", {"using": "xpath", "value": xpath})[
        "element-6066-11e4-a52e-4f735466cecf"]
    rect = request("GET", prefix + f"/element/{ident}/rect")
    return ident if rect["width"] > 0 and rect["height"] > 0 else None


def click(prefix, xpath, completed, timeout=TIMEOUT):
    """Click an element and confirm its post-condition, retrying idempotently on transient disconnects."""
    deadline, token = _scoped_deadline(time.monotonic() + timeout)
    try:
        return _click_until(prefix, xpath, completed, deadline)
    finally:
        _ACTIVE_DEADLINE.reset(token)


def _click_until(prefix, xpath, completed, deadline):
    def ready():
        ident = element(prefix, xpath)
        return ident if ident and request("GET", prefix + f"/element/{ident}/enabled") else None

    def retry_step():
        completed_error = None
        try:
            done = completed()
            if done:
                return ("completed", done)
        except (OSError, KeyError) as exc:
            completed_error = exc
        ident = ready()
        if ident:
            return ("ready", ident)
        if completed_error is not None:
            raise completed_error
        return None

    last_error = None
    for attempt in range(1 + MAX_CLICK_DISCONNECT_RETRIES):
        if attempt == 0:
            ident = _wait_until_deadline(ready, deadline, last_error=last_error)
        else:
            kind, payload = _wait_until_deadline(retry_step, deadline, last_error=last_error)
            if kind == "completed":
                return payload
            ident = payload

        if time.monotonic() >= deadline:
            raise _timeout_error(last_error)
        try:
            request("POST", prefix + f"/element/{ident}/click", {})
            break
        except DISCONNECT_ERRORS as error:
            if not disconnected(error):
                raise
            last_error = error
            if attempt >= MAX_CLICK_DISCONNECT_RETRIES:
                break
            grace_deadline = min(deadline, time.monotonic() + DISCONNECT_GRACE_SECONDS)
            observed, last_error = _poll_until(completed, grace_deadline, last_error=last_error)
            if observed is not _TIMED_OUT:
                return observed
            if time.monotonic() >= deadline:
                raise _timeout_error(last_error) from error

    return _wait_until_deadline(completed, deadline, last_error=last_error)


def main():
    app = str(pathlib.Path(sys.argv[1]).resolve(strict=True))
    driver = subprocess.Popen(["tauri-driver", "--port", "4444"], start_new_session=True)
    session = None
    try:
        until(lambda: request("GET", "/status"))
        session = request("POST", "/session", {"capabilities": {"alwaysMatch": {
            "tauri:options": {"application": app}
        }}})["sessionId"]
        prefix = f"/session/{session}"

        def type_path(path):
            # A lost response may follow a successful key event. Read back the
            # value before retrying so a transport retry cannot duplicate it.
            def entered():
                ident = element(prefix, "//details[@open]//input[@id='file-path']")
                if not ident:
                    return False
                endpoint = prefix + f"/element/{ident}"
                value = request("GET", endpoint + "/property/value")
                if value == str(path):
                    return True
                if value:
                    request("POST", endpoint + "/clear", {})
                request("POST", endpoint + "/value", {"text": str(path)})
                return request("GET", endpoint + "/property/value") == str(path)
            until(entered)

        with tempfile.TemporaryDirectory(prefix="ariadshift-desktop-smoke-") as directory:
            source = pathlib.Path(directory) / "smoke.md"
            source.write_text("# Desktop smoke\n\nA real local conversion.\n", encoding="utf-8")
            docx = pathlib.Path(directory) / "roundtrip.docx"
            subprocess.run([os.environ.get("ASHIFT_PANDOC", "pandoc"), str(source),
                            "--to", "docx", "--output", str(docx)], check=True)
            for input_path in (source, docx):
                if input_path == docx:
                    click(prefix,
                          "//nav[@aria-label='Main navigation']//button[span[normalize-space()='Convert']]",
                          lambda: element(prefix, "//h1[normalize-space()='Convert']"))
                click(prefix, "//details[not(@open)]/summary[normalize-space()='Add a local file by path']",
                      lambda: element(prefix, "//details[@open]//input[@id='file-path']"))
                type_path(input_path)
                click(prefix, "//button[normalize-space()='Add file']",
                      lambda name=input_path.name: element(prefix, f"//ul[@class='file-list']//span[normalize-space()='{name}']"))
                click(prefix, "//button[@aria-label='Start conversion']",
                      lambda: element(prefix, "//h1[normalize-space()='Queue']"))
                until(lambda name=input_path.name: element(
                    prefix,
                    f"//div[strong[normalize-space()='{name}']]//*[@data-status='succeeded']"))
                output = input_path.with_suffix(".html")
                assert "Desktop smoke" in output.read_text(encoding="utf-8")
                assert "A real local conversion." in output.read_text(encoding="utf-8")
            print("Desktop WebDriver smoke passed: Markdown and Pandoc DOCX to HTML through the packaged app.")
    except Exception:
        if session:
            try:
                details = request("POST", prefix + "/execute/sync", {
                    "script": "return document.body.innerText.slice(0, 12000)", "args": []})
                print(f"Desktop UI state: {details}", flush=True)
            except OSError:
                pass
        # Preserve process/socket evidence when WebKit cannot establish automation.
        subprocess.run(["ps", "-eo", "pid,ppid,stat,comm,wchan:32"], check=False)
        subprocess.run(["ss", "-ltnp"], check=False)
        raise
    finally:
        if session:
            try:
                request("DELETE", f"/session/{session}")
            except OSError:
                pass
        os.killpg(driver.pid, signal.SIGTERM)
        try:
            driver.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(driver.pid, signal.SIGKILL)
            driver.wait()


if __name__ == "__main__":
    main()
