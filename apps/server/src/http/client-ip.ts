import { BlockList, isIP } from 'node:net';
import type { Context } from 'hono';

/**
 * Reverse proxies whose `X-Forwarded-For` the server believes, as parsed
 * from `TRUSTED_PROXIES`.
 */
export interface TrustedProxies {
  /** Whether the address is one of the proxies. */
  has(address: string): boolean;
  /** Whether any proxy is configured at all. */
  readonly any: boolean;
}

/**
 * Parses a comma-separated list of addresses and CIDR ranges
 * (`172.16.0.0/12, 127.0.0.1, fd00::/8`).
 *
 * @param list - The value of `TRUSTED_PROXIES`.
 * @returns The proxies.
 * @throws {Error} For an entry that is neither an address nor a range.
 */
export function parseTrustedProxies(list: string | undefined): TrustedProxies {
  const blocks = new BlockList();
  const entries = (list ?? '')
    .split(',')
    .map((entry) => entry.trim())
    .filter(Boolean);
  for (const entry of entries) {
    const [address = '', prefix, ...rest] = entry.split('/');
    const family = isIP(address);
    const bits = prefix === undefined ? (family === 6 ? 128 : 32) : Number(prefix);
    if (!family || rest.length > 0 || !Number.isInteger(bits) || bits < 0 || bits > (family === 6 ? 128 : 32) || prefix === '') {
      throw new Error(`TRUSTED_PROXIES: "${entry}" is neither an IP address nor a CIDR range (like 172.16.0.0/12)`);
    }
    blocks.addSubnet(address, bits, family === 6 ? 'ipv6' : 'ipv4');
  }
  return {
    any: entries.length > 0,
    has: (address) => {
      const family = isIP(address);
      return family !== 0 && blocks.check(address, family === 6 ? 'ipv6' : 'ipv4');
    },
  };
}

/**
 * An IPv4 address as written, without the `::ffff:` that dual-stack sockets
 * put in front.
 *
 * @param address - Address of a socket or from a header.
 * @returns The plain address.
 */
function plain(address: string): string {
  const mapped = /^::ffff:(\d+\.\d+\.\d+\.\d+)$/i.exec(address);
  return mapped?.[1] ?? address;
}

/**
 * The address a request came from. Only a trusted proxy may name the client:
 * `X-Forwarded-For` is read from the right, skipping proxies, up to the
 * first address that is not one. Anyone who reaches the server directly
 * cannot claim another address by sending the header.
 *
 * @param peer - Address of the TCP connection, null where there is none (tests).
 * @param forwardedFor - The `X-Forwarded-For` header.
 * @param proxies - The trusted proxies.
 * @returns The client's address, or null if unknown.
 */
export function resolveClientIp(peer: string | null, forwardedFor: string | undefined, proxies: TrustedProxies): string | null {
  if (peer === null) {
    return null;
  }
  const direct = plain(peer);
  if (!proxies.has(direct) || !forwardedFor) {
    return direct;
  }
  const hops = forwardedFor
    .split(',')
    .map((hop) => plain(hop.trim()))
    .filter(Boolean);
  for (let index = hops.length - 1; index >= 0; index -= 1) {
    const hop = hops[index] as string;
    if (!isIP(hop)) {
      // Garbage in the chain: the last address that is sure is the proxy's.
      return direct;
    }
    if (!proxies.has(hop)) {
      return hop;
    }
  }
  return direct;
}

/**
 * The address of the TCP connection behind a Hono request on Node.
 *
 * @param c - Request context.
 * @returns The remote address, or null without a socket (`app.request` in tests).
 */
export function peerAddress(c: Context): string | null {
  const env = c.env as { incoming?: { socket?: { remoteAddress?: string } } } | undefined;
  return env?.incoming?.socket?.remoteAddress ?? null;
}

/**
 * The key that rate limits count under: the address itself for IPv4, the
 * /64 network for IPv6, because one connection usually owns a whole /64.
 *
 * @param address - Client address.
 * @returns The key.
 */
export function rateKey(address: string | null): string {
  if (address === null) {
    return 'unknown';
  }
  if (isIP(address) !== 6) {
    return address;
  }
  const [head = '', tail = ''] = address.toLowerCase().split('::');
  const left = head ? head.split(':') : [];
  const right = tail ? tail.split(':') : [];
  const groups = address.includes('::') ? [...left, ...Array<string>(8 - left.length - right.length).fill('0'), ...right] : left;
  return `${groups.slice(0, 4).map((group) => group.replace(/^0+(?=.)/, '')).join(':')}::/64`;
}
