"""Request loop: one-shot jobs and sessions."""

from __future__ import annotations

import logging
import sys
from typing import IO

from ariad_docling import convert
from ariad_docling.describe import capabilities
from ariad_docling.environment import prepare_process, resolve_assets
from ariad_docling.protocol import (
    ConvertRequest,
    DescribeRequest,
    EngineError,
    Writer,
    parse_request,
    read_line,
)

MAX_ERROR_MESSAGE = 300


def main(protocol_fd: int, stdin: IO[bytes]) -> int:
    """Serves requests until the job is done or, in a session, until stdin closes.

    A first ``convert`` without ``session_id`` and every ``describe`` are one-shot. A first
    ``convert`` with a ``session_id`` opens a session: later requests must carry the same id
    and reuse the warm converters. Every request gets exactly one ``result``.
    """
    prepare_process()
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr)
    writer = Writer(protocol_fd)
    converters = convert.Converters()
    session: str | None = None
    while True:
        try:
            line = read_line(stdin)
            if line is None:
                return 0
            request = parse_request(line)
        except EngineError as error:
            writer.error(error)
            return 0

        if session is not None and (
            not isinstance(request, ConvertRequest) or request.session_id != session
        ):
            writer.error(EngineError("invalid_request", "request does not belong to this session"))
            return 0

        handle(request, writer, converters)
        if isinstance(request, DescribeRequest) or request.session_id is None:
            return 0
        session = request.session_id


def handle(
    request: ConvertRequest | DescribeRequest, writer: Writer, converters: convert.Converters
) -> None:
    """Runs one request and emits exactly one ``result``."""
    try:
        if isinstance(request, DescribeRequest):
            writer.emit(capabilities(resolve_assets({})))
            writer.ok()
        else:
            convert.run(request, writer, converters)
    except EngineError as error:
        writer.error(error)
    except Exception as error:  # every failure must still produce a result
        logging.getLogger("ariad_docling").exception("conversion failed")
        message = f"{type(error).__name__}: {error}"[:MAX_ERROR_MESSAGE]
        writer.error(EngineError("engine_failure", message))
