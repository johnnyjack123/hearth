/** Opening line of a fenced code block: up to three spaces, then ``` or ~~~. */
const FENCE = /^ {0,3}(`{3,}|~{3,})/;

/** A display formula alone on its lines: `\[ … \]`, possibly over several lines. */
const DISPLAY_BLOCK = /^([ \t]*)\\\[([\s\S]+?)\\\][ \t]*$/gm;

/** `\[ … \]` inside running text. */
const DISPLAY_INLINE = /\\\[([\s\S]+?)\\\]/g;

/** `\( … \)`. */
const INLINE = /\\\(([\s\S]+?)\\\)/g;

/**
 * Turns LaTeX's `\( … \)` and `\[ … \]` into the `$$ … $$` form the Markdown
 * renderer understands (remark-math). Many models write math that way
 * (DeepSeek does by default). A single `$` stays text on purpose, so amounts
 * like $5 are never read as formulas. Code blocks and inline code are left
 * as they are; an unclosed delimiter (still streaming) stays text until its
 * end arrives.
 *
 * @param markdown - Markdown of a reply.
 * @returns The same Markdown with those formulas in `$$` form.
 */
export function normalizeMath(markdown: string): string {
  if (!markdown.includes('\\(') && !markdown.includes('\\[')) {
    return markdown;
  }
  const out: string[] = [];
  let prose: string[] = [];
  let fence: string | null = null;
  const flush = () => {
    if (prose.length > 0) {
      out.push(convert(prose.join('\n')));
      prose = [];
    }
  };
  for (const line of markdown.split('\n')) {
    const marker = FENCE.exec(line)?.[1];
    if (fence !== null) {
      out.push(line);
      // A fence closes with the same character, at least as long, alone.
      if (marker && marker[0] === fence[0] && marker.length >= fence.length && line.trim() === marker) {
        fence = null;
      }
      continue;
    }
    if (marker) {
      flush();
      fence = marker;
      out.push(line);
      continue;
    }
    prose.push(line);
  }
  flush();
  return out.join('\n');
}

/**
 * Converts the formulas of text without code blocks; inline code spans
 * (odd parts of the split) stay untouched.
 */
function convert(text: string): string {
  return text
    .split(/(`+[^`\n]*`+)/)
    .map((part, index) =>
      index % 2 === 1
        ? part
        : part
            .replace(DISPLAY_BLOCK, (_match, indent: string, body: string) => `${indent}$$\n${indent}${body.trim()}\n${indent}$$`)
            .replace(DISPLAY_INLINE, (_match, body: string) => `$$${body.trim()}$$`)
            .replace(INLINE, (_match, body: string) => `$$${body.trim()}$$`),
    )
    .join('');
}
