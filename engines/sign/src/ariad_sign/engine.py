"""Request loop for the sign engine (ariad-engine/2)."""

from __future__ import annotations

import logging
import sys
from typing import IO

from ariad_sign import describe, remove, sign, verify
from ariad_sign.protocol import (
    DescribeRequest,
    EngineError,
    RunToolRequest,
    Writer,
    parse_request,
    read_line,
)

MAX_ERROR_MESSAGE = 300
logger = logging.getLogger("ariad_sign")


def main(protocol_fd: int, stdin: IO[bytes]) -> int:
    """Serves one-shot requests in ariad-engine/2."""
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr)
    writer = Writer(protocol_fd)

    try:
        line = read_line(stdin)
        if line is None:
            return 0
        request = parse_request(line)
    except EngineError as error:
        writer.error(error)
        return 0

    handle(request, writer)
    return 0


def handle(request: RunToolRequest | DescribeRequest, writer: Writer) -> None:
    """Dispatches one request and emits protocol events ending with a result."""
    try:
        if isinstance(request, DescribeRequest):
            writer.emit(describe.capabilities())
            writer.ok()
            return

        if request.tool == "pdf.sign":
            sign.run_sign(request, writer)
        elif request.tool == "pdf.sign_verify":
            verify.run_verify(request, writer)
        elif request.tool == "pdf.sign_remove":
            remove.run_remove(request, writer)
        else:
            raise EngineError("unsupported_route", f"unsupported tool: '{request.tool}'")
    except EngineError as error:
        writer.error(error)
    except Exception as error:
        logger.exception("execution failed")
        message = f"{type(error).__name__}: {error}"[:MAX_ERROR_MESSAGE]
        writer.error(EngineError("engine_failure", message))
