/** One dispatched Server-Sent Events message. */
export interface SseMessage {
  /** The `event` field, or `message` when absent. */
  event: string;
  /** The `data` lines joined with newlines. */
  data: string;
  /** The last event id seen on the stream, if any. */
  id?: string;
  /** A reconnection delay in milliseconds sent with this message. */
  retry?: number;
}

/**
 * Parses a `text/event-stream` body as the HTML standard describes: CR, LF and
 * CRLF line endings, `:` comments (keep-alives), one optional space after the
 * colon, and dispatch on a blank line. Messages without data are not dispatched,
 * but their `id` still updates the last event id.
 */
export async function* parseSse(body: ReadableStream<Uint8Array>): AsyncGenerator<SseMessage> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let pendingCr = false;
  let data: string[] = [];
  let event = "";
  let lastId: string | undefined;
  let retry: number | undefined;

  function* processLine(line: string): Generator<SseMessage> {
    if (line === "") {
      if (data.length > 0) {
        const message: SseMessage = { event: event || "message", data: data.join("\n") };
        if (lastId !== undefined) message.id = lastId;
        if (retry !== undefined) message.retry = retry;
        yield message;
      }
      data = [];
      event = "";
      retry = undefined;
      return;
    }
    if (line.startsWith(":")) {
      return;
    }
    const colon = line.indexOf(":");
    const field = colon === -1 ? line : line.slice(0, colon);
    let value = colon === -1 ? "" : line.slice(colon + 1);
    if (value.startsWith(" ")) {
      value = value.slice(1);
    }
    switch (field) {
      case "data":
        data.push(value);
        break;
      case "event":
        event = value;
        break;
      case "id":
        if (!value.includes("\0")) {
          lastId = value;
        }
        break;
      case "retry":
        if (/^\d+$/.test(value)) {
          retry = Number(value);
        }
        break;
    }
  }

  function* drain(): Generator<SseMessage> {
    for (;;) {
      const match = /\r\n|\r|\n/.exec(buffer);
      if (match === null) {
        return;
      }
      // A CR at the end of the buffer may be the first half of a CRLF split
      // across chunks; wait for the next chunk before deciding.
      if (match[0] === "\r" && match.index === buffer.length - 1) {
        pendingCr = true;
        return;
      }
      const line = buffer.slice(0, match.index);
      buffer = buffer.slice(match.index + match[0].length);
      yield* processLine(line);
    }
  }

  let finished = false;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) {
        finished = true;
        break;
      }
      let text = decoder.decode(value, { stream: true });
      if (pendingCr) {
        pendingCr = false;
        if (text.startsWith("\n")) {
          text = text.slice(1);
        }
        const line = buffer.slice(0, -1);
        buffer = "";
        yield* processLine(line);
      }
      buffer += text;
      yield* drain();
    }
    buffer += decoder.decode();
    if (pendingCr) {
      pendingCr = false;
      yield* processLine(buffer.slice(0, -1));
      buffer = "";
    }
    yield* drain();
    // An incomplete final message (no blank line) is discarded, as the
    // standard requires.
  } finally {
    // Stopping early (the consumer returned or threw) closes the connection.
    if (!finished) {
      await reader.cancel().catch(() => {});
    }
    reader.releaseLock();
  }
}
