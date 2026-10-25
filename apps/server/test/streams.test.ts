import { describe, expect, it } from 'vitest';
import { ActiveStreams } from '../src/streams.ts';

/** A stream the test writes to by hand. */
function source() {
  let controller!: ReadableStreamDefaultController<string>;
  const stream = new ReadableStream<string>({
    start(c) {
      controller = c;
    },
  });
  return { stream, write: (chunk: string) => controller.enqueue(chunk), end: () => controller.close() };
}

async function readAll(stream: ReadableStream<string>): Promise<string> {
  let text = '';
  for await (const chunk of stream) {
    text += chunk;
  }
  return text;
}

/** Lets the registry read what was written. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('active streams', () => {
  it('replays a running reply from its start and follows it to the end', async () => {
    const streams = new ActiveStreams();
    const reply = source();
    streams.add('c1', reply.stream, () => undefined);
    reply.write('a');
    reply.write('b');
    await settle();
    const first = streams.resume('c1');
    const second = streams.resume('c1');
    expect(first).not.toBeNull();
    reply.write('c');
    reply.end();
    expect(await readAll(first as ReadableStream<string>)).toBe('abc');
    expect(await readAll(second as ReadableStream<string>)).toBe('abc');
    expect(streams.has('c1')).toBe(false);
    expect(streams.resume('c1')).toBeNull();
    expect(streams.resume('other')).toBeNull();
  });

  it('stops the work behind a reply, and a new reply replaces an old one', async () => {
    const streams = new ActiveStreams();
    const stopped: string[] = [];
    const old = source();
    streams.add('c1', old.stream, () => stopped.push('old'));
    const fresh = source();
    streams.add('c1', fresh.stream, () => stopped.push('fresh'));
    expect(stopped).toEqual(['old']);
    // The old reply ending does not remove the new one.
    old.end();
    await settle();
    expect(streams.has('c1')).toBe(true);
    expect(streams.stop('c1')).toBe(true);
    expect(stopped).toEqual(['old', 'fresh']);
    fresh.end();
    await settle();
    expect(streams.stop('c1')).toBe(false);
  });

  it('ends a resumed stream when the reply fails', async () => {
    const streams = new ActiveStreams();
    let controller!: ReadableStreamDefaultController<string>;
    streams.add(
      'c1',
      new ReadableStream<string>({
        start(c) {
          controller = c;
        },
      }),
      () => undefined,
    );
    controller.enqueue('x');
    await settle();
    const resumed = streams.resume('c1') as ReadableStream<string>;
    controller.error(new Error('boom'));
    expect(await readAll(resumed)).toBe('x');
  });
});
