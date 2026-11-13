/**
 * This page's name for the server, sent with every request
 * (`CLIENT_ID_HEADER`). Events the server announces come back with it, so
 * the page skips what it caused itself. New with every page load;
 * `crypto.randomUUID` is missing over plain `http://` in the LAN.
 */
export const CLIENT_ID = Array.from(crypto.getRandomValues(new Uint8Array(12)), (byte) => byte.toString(16).padStart(2, '0')).join('');
