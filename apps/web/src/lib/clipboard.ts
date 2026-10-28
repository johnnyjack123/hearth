/**
 * Browsers offer `navigator.clipboard` and `ClipboardItem` only in secure
 * contexts (HTTPS or localhost). The server often runs on plain HTTP in the
 * local network (the user's Raspberry Pi), and there every copy button
 * failed: Streamdown's with "Clipboard API not available", ours silently.
 * Where the API is missing, this puts a small stand-in in its place that
 * copies through a hidden text field and `document.execCommand('copy')`,
 * deprecated but still supported. With the real API nothing changes.
 */
export function installClipboardFallback(): void {
  if (typeof navigator === 'undefined' || navigator.clipboard) {
    return;
  }
  const writeText = (text: string): Promise<void> =>
    copyWithTextField(text) ? Promise.resolve() : Promise.reject(new Error('Copying failed'));
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: {
      writeText,
      // Streamdown copies tables as plain text and HTML; plain text is what
      // a text field can carry.
      write: async (items: ClipboardItem[]): Promise<void> => {
        const item = items.find((candidate) => candidate.types.includes('text/plain'));
        if (!item) {
          throw new Error('Only plain text can be copied here');
        }
        await writeText(await (await item.getType('text/plain')).text());
      },
    },
  });
  if (typeof ClipboardItem === 'undefined') {
    Object.defineProperty(globalThis, 'ClipboardItem', { configurable: true, value: PlainClipboardItem });
  }
}

/** Enough of `ClipboardItem` for the stand-in above. */
class PlainClipboardItem {
  readonly #items: Record<string, string | Blob | PromiseLike<string | Blob>>;

  /**
   * @param items - Data per MIME type.
   */
  constructor(items: Record<string, string | Blob | PromiseLike<string | Blob>>) {
    this.#items = items;
  }

  /** The MIME types it carries. */
  get types(): string[] {
    return Object.keys(this.#items);
  }

  /**
   * @param type - A MIME type of {@link types}.
   * @returns Its data.
   */
  async getType(type: string): Promise<Blob> {
    const data = await this.#items[type];
    if (data === undefined) {
      throw new Error(`No ${type} data`);
    }
    return typeof data === 'string' ? new Blob([data], { type }) : data;
  }
}

/**
 * Copies `text` through a hidden text field. It goes into the dialog the
 * user is in, if any: a dialog keeps focus inside, and a field outside it
 * could not be selected.
 *
 * @param text - What to copy.
 * @returns Whether the browser copied it.
 */
function copyWithTextField(text: string): boolean {
  const active = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const host = active?.closest('[role=dialog], [role=alertdialog]') ?? document.body;
  const field = document.createElement('textarea');
  field.value = text;
  field.setAttribute('readonly', '');
  field.setAttribute('aria-hidden', 'true');
  // Out of sight and layout; not a design value, so no token.
  field.style.position = 'fixed';
  field.style.top = '0';
  field.style.left = '0';
  field.style.opacity = '0';
  field.style.pointerEvents = 'none';
  host.append(field);
  field.select();
  let copied = false;
  try {
    copied = document.execCommand('copy');
  } finally {
    field.remove();
    active?.focus({ preventScroll: true });
  }
  return copied;
}
