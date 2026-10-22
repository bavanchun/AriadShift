"""Process and job environment: offline settings, workspace confinement, memory limits."""

from __future__ import annotations

import os
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from ariad_docling.protocol import EngineError

# Third-party code reads these at import time, so they are set before Docling is imported.
_OFFLINE = {
    "HF_HUB_OFFLINE": "1",
    "TRANSFORMERS_OFFLINE": "1",
    "HF_HUB_DISABLE_TELEMETRY": "1",
    "HF_HUB_DISABLE_PROGRESS_BARS": "1",
    "TOKENIZERS_PARALLELISM": "false",
    "DOCLING_SECURITY_REFUSE_UNPINNED_REMOTE_CODE": "1",
}

MAX_THREADS = 4


def thread_count() -> int:
    return max(1, min(MAX_THREADS, os.cpu_count() or 1))


def prepare_process() -> None:
    """Applies settings that must be in place before any third-party import."""
    os.environ.update(_OFFLINE)
    threads = str(thread_count())
    for name in ("OMP_NUM_THREADS", "MKL_NUM_THREADS", "OPENBLAS_NUM_THREADS"):
        os.environ[name] = threads
    # getpass and some caches need a user name; the host clears the environment.
    for name in ("USER", "USERNAME", "LOGNAME"):
        os.environ.setdefault(name, "ariadshift")


@dataclass(frozen=True)
class Assets:
    """Model and OCR data locations for one job."""

    artifacts: Path | None
    tessdata: Path | None


def install_root() -> Path:
    """The directory that holds the runtime, ``models`` and ``tessdata``.

    In a pack the interpreter lives in ``<root>/runtime``; in the development environment it is
    ``.tools/docling/python-env``. Both put the assets next to the interpreter prefix.
    """
    return Path(sys.prefix).resolve().parent


def resolve_assets(options: dict[str, Any]) -> Assets:
    """Request options name the asset directories; the install root supplies defaults."""
    root = install_root()
    return Assets(
        artifacts=_directory(options, "artifacts_path", root / "models"),
        tessdata=_directory(options, "tessdata_path", root / "tessdata"),
    )


def _directory(options: dict[str, Any], key: str, default: Path) -> Path | None:
    value = options.get(key)
    if value is None:
        return default if default.is_dir() else None
    if not isinstance(value, str) or not value:
        raise EngineError("invalid_request", f"options.{key} must be a non-empty string")
    path = Path(value)
    return path if path.is_dir() else None


def prepare_job(work_dir: str, assets: Assets) -> None:
    """Keeps temporary files and library caches inside the job workspace."""
    work = Path(work_dir)
    if not work.is_dir():
        raise EngineError("io", "work_dir does not exist")
    for name in ("TMPDIR", "TMP", "TEMP", "HOME", "USERPROFILE"):
        os.environ[name] = str(work)
    os.environ["XDG_CACHE_HOME"] = str(work / "cache")
    os.environ["HF_HOME"] = str(work / "cache" / "huggingface")
    os.environ["TORCH_HOME"] = str(work / "cache" / "torch")
    os.environ["TORCHINDUCTOR_CACHE_DIR"] = str(work / "cache" / "torchinductor")
    tempfile.tempdir = str(work)
    if assets.tessdata is not None:
        os.environ["TESSDATA_PREFIX"] = str(assets.tessdata)


def enforces_memory_limit() -> bool:
    """``RLIMIT_DATA`` bounds heap and anonymous mappings only on Linux."""
    return sys.platform.startswith("linux")


# Below this much room above the memory already in use, a job cannot convert anything.
MEMORY_HEADROOM_MB = 256


def data_usage_mb() -> int | None:
    """The process data size (``VmData``) in MiB, which ``RLIMIT_DATA`` is compared with."""
    try:
        with open("/proc/self/status", encoding="ascii") as status:
            for line in status:
                if line.startswith("VmData:"):
                    return int(line.split()[1]) // 1024
    except OSError:
        return None
    return None


def apply_memory_limit(max_memory_mb: int | None) -> None:
    """Sets the soft ``RLIMIT_DATA`` for this job; ``None`` restores the hard limit.

    The limit is applied after the runtime and models are loaded, because C extensions that
    fail to allocate while loading crash instead of raising. A limit that leaves too little room
    above the memory already in use fails cleanly here.
    """
    if not enforces_memory_limit():
        return
    import resource

    _, hard = resource.getrlimit(resource.RLIMIT_DATA)
    if max_memory_mb is None:
        resource.setrlimit(resource.RLIMIT_DATA, (hard, hard))
        return
    used = data_usage_mb()
    if used is not None and max_memory_mb < used + MEMORY_HEADROOM_MB:
        raise EngineError(
            "limit_exceeded",
            f"the memory limit of {max_memory_mb} MB is below the {used} MB the engine needs",
        )
    soft = max_memory_mb * 1024 * 1024
    if hard != resource.RLIM_INFINITY:
        soft = min(soft, hard)
    resource.setrlimit(resource.RLIMIT_DATA, (soft, hard))
