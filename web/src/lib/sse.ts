// Reading text/event-stream: a job's events come through EventSource, but an answer's come back from a POST,
// which EventSource can't send, so its body is read here, an event at a time.

export interface StreamEvent {
  event: string;
  data: unknown;
}

/** Feeds `chunk`s of a stream in and gets whole events out, each event's data parsed as JSON when it is. */
export class EventParser {
  private buffer = '';

  push(chunk: string): StreamEvent[] {
    this.buffer += chunk;
    const out: StreamEvent[] = [];
    let match: RegExpExecArray | null;
    while ((match = /\r\n\r\n|\n\n|\r\r/.exec(this.buffer))) {
      const event = parseBlock(this.buffer.slice(0, match.index));
      if (event) out.push(event);
      this.buffer = this.buffer.slice(match.index + match[0].length);
    }
    return out;
  }

  /** What's left when the stream ends: a last event not followed by a blank line. */
  end(): StreamEvent[] {
    const rest = this.buffer;
    this.buffer = '';
    const event = rest.trim() ? parseBlock(rest) : null;
    return event ? [event] : [];
  }
}

function parseBlock(block: string): StreamEvent | null {
  let event = 'message';
  const data: string[] = [];
  for (const line of block.split(/\r\n|\r|\n/)) {
    if (!line || line.startsWith(':')) continue;
    const colon = line.indexOf(':');
    const field = colon < 0 ? line : line.slice(0, colon);
    let value = colon < 0 ? '' : line.slice(colon + 1);
    if (value.startsWith(' ')) value = value.slice(1);
    if (field === 'event') event = value;
    else if (field === 'data') data.push(value);
  }
  if (!data.length) return null;
  const text = data.join('\n');
  try {
    return { event, data: JSON.parse(text) };
  } catch {
    return { event, data: { text } };
  }
}

/** Reads a response body to its end, handing each event to `on`. */
export async function readEvents(body: ReadableStream<Uint8Array>, on: (event: StreamEvent) => void): Promise<void> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  const parser = new EventParser();
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    for (const event of parser.push(decoder.decode(value, { stream: true }))) on(event);
  }
  for (const event of parser.push(decoder.decode())) on(event);
  for (const event of parser.end()) on(event);
}
