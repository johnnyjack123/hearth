/**
 * What the user typed but did not send, per chat, in this browser's
 * `localStorage`: it survives reloading the page and switching chats. Saved
 * on every change (a few bytes, synchronous), so nothing is lost when the
 * page goes away mid-word. Signing out removes all of them.
 */
import { useCallback, useState } from 'react';

const PREFIX = 'hearth.draft.';

/** Drafts kept at most; the oldest go first. */
const MAX_DRAFTS = 50;

/** The key of the start page, whose chat does not exist yet. */
export const NEW_CHAT_DRAFT = 'new';

interface Stored {
  text: string;
  /** When it was last changed, epoch milliseconds. */
  at: number;
}

function read(key: string): Stored | null {
  try {
    const raw = localStorage.getItem(PREFIX + key);
    if (raw === null) {
      return null;
    }
    const parsed = JSON.parse(raw) as Partial<Stored>;
    return typeof parsed.text === 'string' ? { text: parsed.text, at: typeof parsed.at === 'number' ? parsed.at : 0 } : null;
  } catch {
    return null;
  }
}

/** Keys of all drafts in this browser. */
function draftKeys(): string[] {
  const keys: string[] = [];
  for (let index = 0; index < localStorage.length; index += 1) {
    const key = localStorage.key(index);
    if (key?.startsWith(PREFIX)) {
      keys.push(key);
    }
  }
  return keys;
}

/** Drops the oldest drafts beyond {@link MAX_DRAFTS}. */
function prune(): void {
  const keys = draftKeys();
  if (keys.length <= MAX_DRAFTS) {
    return;
  }
  const aged = keys.map((key) => ({ key, at: read(key.slice(PREFIX.length))?.at ?? 0 })).sort((a, b) => a.at - b.at);
  for (const { key } of aged.slice(0, keys.length - MAX_DRAFTS)) {
    localStorage.removeItem(key);
  }
}

/**
 * The draft of a chat.
 *
 * @param key - The conversation id, or {@link NEW_CHAT_DRAFT}.
 * @returns The text; empty without a draft or without storage.
 */
export function loadDraft(key: string): string {
  return read(key)?.text ?? '';
}

/**
 * Keeps or, when empty, forgets the draft of a chat.
 *
 * @param key - The conversation id, or {@link NEW_CHAT_DRAFT}.
 * @param text - What the message box holds.
 */
export function saveDraft(key: string, text: string): void {
  try {
    if (text.trim() === '') {
      localStorage.removeItem(PREFIX + key);
      return;
    }
    const fresh = localStorage.getItem(PREFIX + key) === null;
    localStorage.setItem(PREFIX + key, JSON.stringify({ text, at: Date.now() } satisfies Stored));
    // Only a new draft can push the count over the limit.
    if (fresh) {
      prune();
    }
  } catch {
    // Storage blocked or full: the draft just lives as long as the page.
  }
}

/** Removes every draft of this browser, e.g. when signing out. */
export function clearDrafts(): void {
  try {
    for (const key of draftKeys()) {
      localStorage.removeItem(key);
    }
  } catch {
    // Storage blocked: there is nothing to remove.
  }
}

/**
 * The message box's text for a chat, kept in this browser as the user types.
 * Follows `key` when the page switches to another chat.
 *
 * @param key - The conversation id, or {@link NEW_CHAT_DRAFT}.
 * @returns The text and a setter that also stores it; an empty text removes the draft.
 */
export function useDraft(key: string): [string, (text: string) => void] {
  const [state, setState] = useState(() => ({ key, text: loadDraft(key) }));
  // Another chat on the same page: its own draft, read during render so the
  // box never shows the old chat's text for a frame.
  let current = state;
  if (state.key !== key) {
    current = { key, text: loadDraft(key) };
    setState(current);
  }
  const setText = useCallback(
    (text: string) => {
      saveDraft(key, text);
      setState({ key, text });
    },
    [key],
  );
  return [current.text, setText];
}
