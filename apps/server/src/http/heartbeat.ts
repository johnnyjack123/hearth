/**
 * Keeps event streams alive behind reverse proxies. nginx (and Nginx Proxy
 * Manager) closes a response after `proxy_read_timeout` without data, 60 s
 * by default; an agent that runs a long build sends nothing for that long.
 * A comment line every few seconds of silence keeps the connection open;
 * SSE parsers skip comments, the AI SDK's included.
 */

/** Silence after which a comment line goes out. */
export const HEARTBEAT_MS = 25_000;

const BEAT = new TextEncoder().encode(': keepalive\n\n');

/**
 * Passes an event stream through, with a comment line after every
 * `interval` of silence. Only between chunks: the streams this wraps send
 * whole events per chunk, so a comment never lands inside one.
 *
 * @param body - The event stream.
 * @param interval - Silence before a comment line, in ms.
 * @returns The same events, with comment lines in the pauses.
 */
export function withHeartbeat(body: ReadableStream<Uint8Array>, interval = HEARTBEAT_MS): ReadableStream<Uint8Array> {
  const reader = body.getReader();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let open = true;
  return new ReadableStream<Uint8Array>({
    start(controller) {
      const arm = () => {
        clearTimeout(timer);
        timer = setTimeout(() => {
          if (open) {
            controller.enqueue(BEAT);
            arm();
          }
        }, interval);
      };
      arm();
      void (async () => {
        try {
          for (;;) {
            const { done, value } = await reader.read();
            if (done) {
              break;
            }
            controller.enqueue(value);
            arm();
          }
          open = false;
          controller.close();
        } catch (error) {
          if (open) {
            open = false;
            controller.error(error);
          }
        } finally {
          open = false;
          clearTimeout(timer);
        }
      })();
    },
    cancel(reason) {
      open = false;
      clearTimeout(timer);
      return reader.cancel(reason);
    },
  });
}
