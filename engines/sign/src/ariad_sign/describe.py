"""Capabilities advertisement for the sign engine."""

from typing import Any

from ariad_sign import __version__


def capabilities() -> dict[str, Any]:
    """Returns the engine capabilities event for describe requests."""
    return {
        "type": "capabilities",
        "protocol": "ariad-engine/2",
        "engine": "sign",
        "version": __version__,
        "tool": {
            "name": "sign",
            "version": __version__,
            "status": "found",
        },
        "license": "MIT",
        "routes": [],
        "tools": [
            {"id": "pdf.sign", "version": "1"},
            {"id": "pdf.sign_token", "version": "1"},
            {"id": "pdf.timestamp", "version": "1"},
            {"id": "pdf.sign_verify", "version": "1"},
            {"id": "pdf.sign_remove", "version": "1"},
        ],
        "enforces_memory_limit": False,
        "supports_session": False,
    }
