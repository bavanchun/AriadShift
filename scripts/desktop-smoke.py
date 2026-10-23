#!/usr/bin/env python3
"""Exercise the real desktop UI through tauri-driver (Linux/WebKitGTK only)."""
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


def request(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(URL + path, data=data, method=method,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return json.load(response)["value"]
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        raise OSError(f"WebDriver {method} {path}: HTTP {error.code}: {detail}") from error


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

        def element(xpath):
            ident = request("POST", prefix + "/element", {"using": "xpath", "value": xpath})[
                "element-6066-11e4-a52e-4f735466cecf"]
            rect = request("GET", prefix + f"/element/{ident}/rect")
            return ident if rect["width"] > 0 and rect["height"] > 0 else None

        def click(xpath):
            def ready():
                ident = element(xpath)
                return ident if ident and request("GET", prefix + f"/element/{ident}/enabled") else None
            ident = until(ready)
            request("POST", prefix + f"/element/{ident}/click", {})

        with tempfile.TemporaryDirectory(prefix="ariadshift-desktop-smoke-") as directory:
            source = pathlib.Path(directory) / "smoke.md"
            source.write_text("# Desktop smoke\n\nA real local conversion.\n", encoding="utf-8")
            click("//summary[normalize-space()='Add a local file by path']")
            ident = until(lambda: element("//details[@open]//input[@id='file-path']"))
            request("POST", prefix + f"/element/{ident}/value", {"text": str(source)})
            click("//button[normalize-space()='Add file']")
            click("//button[@aria-label='Start conversion']")
            until(lambda: element("//*[@data-status='succeeded']"))
            output = source.with_suffix(".html")
            assert "Desktop smoke" in output.read_text(encoding="utf-8")
            assert "A real local conversion." in output.read_text(encoding="utf-8")
            print("Desktop WebDriver smoke passed: Markdown to HTML through the real UI.")
    except Exception:
        if session:
            try:
                details = request("POST", prefix + "/execute/sync", {
                    "script": "return document.querySelector('details')?.outerHTML", "args": []})
                print(f"Path input state: {details}", flush=True)
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
