/**
 * How a part of a document was read, from the first line the server writes
 * (`apps/server/src/files/read.ts`): `From "<name>": sections 3, 7 of 261,
 * chosen by meaning and keywords`, `… 2 matching lines for "…"`, `… no line
 * contains "…".` or `… the whole document, it is short`. An agent's tool
 * result reaches the chat as this text only, a chat model's as well.
 */
export interface DocumentReadInfo {
  /** The document's name. */
  name: string;
  /** Sections (by focus or number), lines with words (grep), or all of it; null for older replies. */
  kind: 'sections' | 'grep' | 'whole' | null;
  /** Sections shown. */
  shown?: number;
  /** Sections of the document. */
  total?: number;
  /** How sections were picked for a focus; missing when asked for by number. */
  ranking?: 'meaning' | 'keywords';
  /** False when nothing matched the focus and the start stands in. */
  matched: boolean;
  /** Lines found by grep. */
  lines?: number;
}

/**
 * Reads {@link DocumentReadInfo} from a tool result.
 *
 * @param text - What the model got back.
 * @returns The info, or null if the text does not start like a document read.
 */
export function documentReadInfo(text: string): DocumentReadInfo | null {
  const first = text.split('\n', 1)[0] ?? '';
  if (!first.startsWith('From "')) {
    return null;
  }
  // The name is JSON; a quote followed by a colon may also stand inside it.
  for (let end = first.indexOf('":', 6); end >= 0; end = first.indexOf('":', end + 1)) {
    let name: unknown;
    try {
      name = JSON.parse(first.slice(5, end + 1));
    } catch {
      continue;
    }
    if (typeof name !== 'string') {
      return null;
    }
    const rest = first.slice(end + 2).trim();
    const sections = /^sections ([\d, ]+) of (\d+)/.exec(rest);
    if (sections) {
      const ranking = rest.includes('chosen by meaning') ? 'meaning' : rest.includes('chosen by keywords') ? 'keywords' : undefined;
      return {
        name,
        kind: 'sections',
        shown: (sections[1] ?? '').split(',').filter((n) => n.trim()).length,
        total: Number(sections[2]),
        ...(ranking ? { ranking } : {}),
        matched: !rest.includes('nothing matched'),
      };
    }
    const lines = /^(\d+) matching lines/.exec(rest);
    if (lines || rest.startsWith('no line contains')) {
      return { name, kind: 'grep', lines: lines ? Number(lines[1]) : 0, matched: lines !== null };
    }
    if (rest.startsWith('the whole document')) {
      return { name, kind: 'whole', matched: true };
    }
    return { name, kind: null, matched: true };
  }
  return null;
}
