/**
 * Which addresses the web fetch may connect to. A port of the gateway's
 * `address.rs`: the server may run on a Raspberry Pi in the user's home
 * network, where the router, the LAN, a tailnet and cloud metadata services
 * are reachable. Only public unicast addresses are therefore allowed.
 */
import { isIPv4, isIPv6 } from 'node:net';

/**
 * Blocked IPv4 ranges (IANA special-purpose registry plus multicast and
 * reserved space), as network and prefix length.
 */
const BLOCKED_V4: readonly [network: string, prefix: number][] = [
  ['0.0.0.0', 8], // "this network"
  ['10.0.0.0', 8], // private
  ['100.64.0.0', 10], // CGNAT, Tailscale
  ['127.0.0.0', 8], // loopback
  ['169.254.0.0', 16], // link-local, cloud metadata
  ['172.16.0.0', 12], // private
  ['192.0.0.0', 24], // IETF protocol assignments
  ['192.0.2.0', 24], // documentation
  ['192.88.99.0', 24], // 6to4 relay
  ['192.168.0.0', 16], // private
  ['198.18.0.0', 15], // benchmarking
  ['198.51.100.0', 24], // documentation
  ['203.0.113.0', 24], // documentation
  ['224.0.0.0', 4], // multicast
  ['240.0.0.0', 4], // reserved, broadcast
];

function v4Number(text: string): number {
  return text.split('.').reduce((value, octet) => value * 256 + Number(octet), 0);
}

const BLOCKED_V4_MASKS = BLOCKED_V4.map(([network, prefix]) => {
  // Shifting by 32 is a no-op in JavaScript, hence the explicit zero mask.
  const mask = prefix === 0 ? 0 : (0xffffffff << (32 - prefix)) >>> 0;
  return { mask, network: (v4Number(network) & mask) >>> 0 };
});

function isPublicV4(value: number): boolean {
  return !BLOCKED_V4_MASKS.some(({ mask, network }) => ((value & mask) >>> 0) === network);
}

/**
 * Parses an IPv6 address into its eight 16-bit groups.
 *
 * @param text - Address without brackets, optionally with a zone (`%eth0`)
 *   and an embedded IPv4 address at the end.
 * @returns The groups, or null if the text is not an IPv6 address.
 */
export function ipv6Groups(text: string): number[] | null {
  const address = text.split('%')[0] ?? '';
  if (!isIPv6(address)) {
    return null;
  }
  let body = address;
  const tail: number[] = [];
  if (address.includes('.')) {
    // An embedded IPv4 address fills the last two groups.
    const lastColon = address.lastIndexOf(':');
    const embedded = v4Number(address.slice(lastColon + 1));
    tail.push(Math.floor(embedded / 0x10000), embedded % 0x10000);
    body = address.slice(0, lastColon + 1);
    // Drop the separator before the IPv4 part, but keep a `::`.
    if (!body.endsWith('::')) {
      body = body.slice(0, -1);
    }
  }
  const parse = (part: string): number[] => (part ? part.split(':').map((group) => Number.parseInt(group, 16)) : []);
  const [head, rest] = body.includes('::') ? body.split('::') : [body, undefined];
  const headGroups = parse(head ?? '');
  const restGroups = rest === undefined ? [] : parse(rest);
  const fill = 8 - tail.length - headGroups.length - restGroups.length;
  const groups =
    rest === undefined ? [...headGroups, ...tail] : [...headGroups, ...new Array<number>(fill).fill(0), ...restGroups, ...tail];
  return groups.length === 8 ? groups : null;
}

function isPublicV6(groups: number[]): boolean {
  const [g0 = 0, g1 = 0, g2 = 0, g3 = 0, g4 = 0, g5 = 0, g6 = 0, g7 = 0] = groups;
  // IPv4-mapped (::ffff:a.b.c.d) and NAT64 (64:ff9b::a.b.c.d) reach the
  // embedded IPv4 address, so that address decides.
  if (g0 === 0 && g1 === 0 && g2 === 0 && g3 === 0 && g4 === 0 && g5 === 0xffff) {
    return isPublicV4(g6 * 0x10000 + g7);
  }
  if (g0 === 0x64 && g1 === 0xff9b && g2 === 0 && g3 === 0 && g4 === 0 && g5 === 0) {
    return isPublicV4(g6 * 0x10000 + g7);
  }
  // Only global unicast (2000::/3) is public; that excludes loopback,
  // unique-local, link-local, multicast and the deprecated IPv4-compatible
  // form in one go.
  if ((g0 & 0xe000) !== 0x2000) {
    return false;
  }
  const blocked = [
    // IETF protocol assignments incl. Teredo (2001::/23).
    g0 === 0x2001 && g1 < 0x0200,
    // Documentation (2001:db8::/32).
    g0 === 0x2001 && g1 === 0x0db8,
    // 6to4 (2002::/16) can tunnel to any IPv4 address, private ones too.
    g0 === 0x2002,
  ];
  return !blocked.includes(true);
}

/**
 * Whether the web fetch may connect to `ip`: a public unicast address.
 *
 * @param ip - IPv4 or IPv6 address, IPv6 without brackets.
 * @returns False for private, loopback, link-local, CGNAT, multicast,
 *   documentation and reserved ranges, and for anything that is no address.
 */
export function isPublicAddress(ip: string): boolean {
  if (isIPv4(ip)) {
    return isPublicV4(v4Number(ip));
  }
  const groups = ipv6Groups(ip);
  return groups !== null && isPublicV6(groups);
}
