/**
 * The colour of the browser's address bar and of the installed app's title
 * and status bar (`<meta name="theme-color">`). `index.html` sets it per
 * system scheme; a theme chosen in the app can differ from the system's,
 * so the app sets it to the background token of the theme it shows.
 */

/**
 * A CSS colour as `#rrggbb`. Browsers serialize the token as `oklch(…)`,
 * which not every reader of `theme-color` understands; a canvas converts it.
 *
 * @param color - Any CSS colour.
 * @returns The colour in hex, or null without a 2D canvas.
 */
function toHex(color: string): string | null {
  const canvas = document.createElement('canvas');
  canvas.width = 1;
  canvas.height = 1;
  const context = canvas.getContext('2d');
  if (!context) {
    return null;
  }
  context.fillStyle = color;
  context.fillRect(0, 0, 1, 1);
  const [r = 0, g = 0, b = 0] = context.getImageData(0, 0, 1, 1).data;
  return `#${[r, g, b].map((value) => value.toString(16).padStart(2, '0')).join('')}`;
}

/**
 * Points `theme-color` at the page's background as it is drawn now; call it
 * after the theme class changed.
 */
export function syncThemeColor(): void {
  const color = toHex(getComputedStyle(document.body).backgroundColor);
  if (!color) {
    return;
  }
  for (const meta of document.head.querySelectorAll('meta[name="theme-color"]')) {
    meta.remove();
  }
  const meta = document.createElement('meta');
  meta.name = 'theme-color';
  meta.content = color;
  document.head.append(meta);
}
