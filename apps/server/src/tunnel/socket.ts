import type { IncomingMessage } from 'node:http';
import type { Duplex } from 'node:stream';
import { DEFAULT_IDLE_MINUTES, TUNNEL_PATH } from '@hearth/shared';
import type { Kysely } from 'kysely';
import { type WebSocket, WebSocketServer } from 'ws';
import type { DB } from '../db/schema.ts';
import { authenticateDevice } from '../services/devices.ts';
import type { DeviceHub } from './hub.ts';

/** Largest frame a device may send; tool outputs are capped well below. */
const MAX_FRAME = 1 << 20;

/** A device that misses this many pings in a row is considered gone. */
const PING_INTERVAL_MS = 30_000;

/** What `serve` from `@hono/node-server` returns, as far as tunnels need it. */
export interface UpgradeServer {
  /** Subscribes to HTTP upgrade requests. */
  on(event: 'upgrade', listener: (request: IncomingMessage, socket: Duplex, head: Buffer) => void): unknown;
  /** Subscribes to the server's end. */
  on(event: 'close', listener: () => void): unknown;
}

function reject(socket: Duplex, status: 401 | 404): void {
  const text = status === 401 ? 'Unauthorized' : 'Not Found';
  socket.end(`HTTP/1.1 ${status} ${text}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n`);
}

/**
 * Accepts device tunnels (`hearth connect`) on {@link TUNNEL_PATH}. Devices
 * authenticate with their token as bearer, like on `/api/llm`; the session
 * cookie of the web app does not count here.
 *
 * @param server - The HTTP server.
 * @param db - Open database, for the token check.
 * @param hub - Where connected devices are registered.
 */
export function attachTunnel(server: UpgradeServer, db: Kysely<DB>, hub: DeviceHub): void {
  const wss = new WebSocketServer({ noServer: true, maxPayload: MAX_FRAME });
  const alive = new WeakMap<WebSocket, boolean>();

  server.on('upgrade', (request, socket, head) => {
    const path = new URL(request.url ?? '/', 'http://localhost').pathname;
    if (path !== TUNNEL_PATH) {
      reject(socket, 404);
      return;
    }
    const header = request.headers.authorization ?? '';
    const token = header.startsWith('Bearer ') ? header.slice('Bearer '.length).trim() : '';
    void (token ? authenticateDevice(db, token) : Promise.resolve(undefined))
      .then((found) => {
        if (!found) {
          reject(socket, 401);
          return;
        }
        wss.handleUpgrade(request, socket, head, (ws) => {
          alive.set(ws, true);
          const connection = hub.attach(found.device.id, found.owner.id, ws, {
            idleMinutes: found.device.idleMinutes ?? DEFAULT_IDLE_MINUTES,
          });
          ws.on('pong', () => alive.set(ws, true));
          ws.on('message', (data, isBinary) => {
            if (isBinary) {
              ws.close(1003, 'text frames only');
              return;
            }
            connection.receive(data.toString());
          });
          ws.on('close', () => connection.closed());
          ws.on('error', () => ws.terminate());
        });
      })
      .catch((error: unknown) => {
        console.error('[tunnel] upgrade failed', error);
        socket.destroy();
      });
  });

  // WebSocket pings detect devices that vanished without closing (sleep,
  // network loss); the device answers them automatically.
  const timer = setInterval(() => {
    for (const ws of wss.clients) {
      if (!alive.get(ws)) {
        ws.terminate();
        continue;
      }
      alive.set(ws, false);
      ws.ping();
    }
  }, PING_INTERVAL_MS);
  timer.unref();
  server.on('close', () => {
    clearInterval(timer);
    wss.close();
  });
}
