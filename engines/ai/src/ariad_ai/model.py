"""Optional local llama.cpp execution; URLs and cloud execution are rejected."""

from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path
from typing import Any

from ariad_ai.protocol import EngineError


class LocalModel:
    def __init__(self, options: dict[str, Any], limits: dict[str, Any]) -> None:
        self.executable = Path(options.get("llama_cli", ""))
        self.model = Path(options.get("model_path", options.get("gguf_model", "")))
        if not self.executable.is_file() or not self.model.is_file():
            raise EngineError("tool_missing", "configure local llama_cli and model_path files")
        if self.model.suffix.lower() != ".gguf" or self.model.open("rb").read(4) != b"GGUF":
            raise EngineError("invalid_request", "model_path must be a local GGUF model")
        self.timeout = min(float(limits.get("timeout_s") or 120), 300)

    def generate(self, prompt: str) -> str:
        with tempfile.TemporaryFile() as output:
            # A real file works on all pack targets, including macOS and Windows.
            with tempfile.NamedTemporaryFile(
                mode="w", encoding="utf-8", suffix=".txt", delete_on_close=False
            ) as prompt_file:
                prompt_file.write(prompt)
                prompt_file.close()
                command = [
                    str(self.executable.resolve()),
                    "-m",
                    str(self.model.resolve()),
                    "-f",
                    prompt_file.name,
                    "-n",
                    "1024",
                    "--seed",
                    "0",
                    "--no-display-prompt",
                    "--simple-io",
                    "--single-turn",
                ]
                try:
                    process = subprocess.run(
                        command,
                        stdin=subprocess.DEVNULL,
                        stdout=output,
                        stderr=subprocess.DEVNULL,
                        timeout=self.timeout,
                        check=False,
                    )
                except subprocess.TimeoutExpired as exc:
                    raise EngineError("limit_exceeded", "local model timed out") from exc
            if process.returncode:
                raise EngineError("engine_failure", "local llama-cli failed")
            output.seek(0, 2)
            if output.tell() > 2_000_000:
                raise EngineError("limit_exceeded", "local model output exceeds 2 MB")
            output.seek(0)
            result = output.read().decode("utf-8", errors="replace").strip()
        if not result:
            raise EngineError("engine_failure", "local model returned no text")
        return result
