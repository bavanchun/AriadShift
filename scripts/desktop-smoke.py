#!/usr/bin/env python3
"""Exercise the real desktop UI through tauri-driver (Linux/WebKitGTK only)."""
import http.client
import json
import os
import signal
import pathlib
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

URL = "http://127.0.0.1:4444"
DISCONNECT_ERRORS = (http.client.RemoteDisconnected, ConnectionResetError, urllib.error.URLError)


def disconnected(error):
    if isinstance(error, urllib.error.URLError):
        error = error.reason
    return isinstance(error, (http.client.RemoteDisconnected, ConnectionResetError))


def request(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(URL + path, data=data, method=method,
                                 headers={"Content-Type": "application/json", "Connection": "close"})
    # Element lookup is a read despite using POST. Never replay session creation,
    # clicks or key events: the command may have succeeded before its response was lost.
    retryable = method == "GET" or (method == "POST" and path.endswith("/element"))
    deadline = time.monotonic() + 30
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
            time.sleep(min(0.15 * (attempt + 1), max(0, deadline - time.monotonic())))


def until(action, timeout=30):
    started = time.monotonic()
    last_error = None
    while True:
        try:
            value = action()
            if value:
                return value
        except (OSError, KeyError) as error:
            last_error = error
        if time.monotonic() - started > timeout:
            raise RuntimeError(f"Timed out waiting for desktop UI: {last_error}")
        time.sleep(0.15)


def element(prefix, xpath):
    ident = request("POST", prefix + "/element", {"using": "xpath", "value": xpath})[
        "element-6066-11e4-a52e-4f735466cecf"]
    rect = request("GET", prefix + f"/element/{ident}/rect")
    return ident if rect["width"] > 0 and rect["height"] > 0 else None


def click(prefix, xpath, completed):
    def ready():
        ident = element(prefix, xpath)
        return ident if ident and request("GET", prefix + f"/element/{ident}/enabled") else None
    ident = until(ready)
    try:
        request("POST", prefix + f"/element/{ident}/click", {})
    except DISCONNECT_ERRORS as error:
        if not disconnected(error):
            raise
    # Confirm the effect even when only the response was lost; a second click could
    # close the path disclosure or enqueue the same conversion twice.
    until(completed)


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
                click(prefix, "//summary[normalize-space()='Add a local file by path']",
                      lambda: element(prefix, "//details[@open]//input[@id='file-path']"))
                type_path(input_path)
                click(prefix, "//button[normalize-space()='Add file']",
                      lambda: element(prefix, f"//ul[@class='file-list']//span[normalize-space()='{input_path.name}']"))
                click(prefix, "//button[@aria-label='Start conversion']",
                      lambda: element(prefix, "//h1[normalize-space()='Queue']"))
                until(lambda: element(prefix,
                    f"//div[strong[normalize-space()='{input_path.name}']]//*[@data-status='succeeded']"))
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
