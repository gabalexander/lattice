// Reading an answer's text/event-stream, however its bytes are split.

import { expect, test } from 'vitest';
import { EventParser, readEvents, type StreamEvent } from './sse';

test('events come out whole, whatever the chunks', () => {
  const stream = 'event: tool\ndata: {"name":"Read","path":"src/a.rs"}\n\nevent: delta\ndata: {"text":"Hi"}\n\n: a comment\n\nevent: done\ndata: {"conversation":"c1","cost_usd":0.03}\n\n';
  for (const size of [1, 3, 7, stream.length]) {
    const parser = new EventParser();
    const out: StreamEvent[] = [];
    for (let i = 0; i < stream.length; i += size) out.push(...parser.push(stream.slice(i, i + size)));
    out.push(...parser.end());
    expect(out).toEqual([
      { event: 'tool', data: { name: 'Read', path: 'src/a.rs' } },
      { event: 'delta', data: { text: 'Hi' } },
      { event: 'done', data: { conversation: 'c1', cost_usd: 0.03 } },
    ]);
  }
});

test('CRLF lines, several data lines, an unnamed event, and data that is not JSON', () => {
  const parser = new EventParser();
  expect(parser.push('data: {"a":\r\ndata: 1}\r\n\r\ndata:plain\n\n')).toEqual([
    { event: 'message', data: { a: 1 } },
    { event: 'message', data: { text: 'plain' } },
  ]);
});

test('a last event without its blank line still counts', () => {
  const parser = new EventParser();
  expect(parser.push('event: error\ndata: {"message":"no"}')).toEqual([]);
  expect(parser.end()).toEqual([{ event: 'error', data: { message: 'no' } }]);
});

test('a response body is read to its end, multi-byte characters split across chunks', async () => {
  const bytes = new TextEncoder().encode('event: delta\ndata: {"text":"héllo → ✓"}\n\n');
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      for (let i = 0; i < bytes.length; i += 5) controller.enqueue(bytes.slice(i, i + 5));
      controller.close();
    },
  });
  const seen: StreamEvent[] = [];
  await readEvents(body, (e) => seen.push(e));
  expect(seen).toEqual([{ event: 'delta', data: { text: 'héllo → ✓' } }]);
});
