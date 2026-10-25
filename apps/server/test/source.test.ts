import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const TEXT_FILES = /\.(ts|tsx|js|mjs|cjs|rs|toml|ya?ml|json|md|sh|ps1|html|css|txt)$/;

/**
 * Characters that make source code read differently than it runs ("Trojan
 * Source"): controls other than tab and line breaks, invisible characters,
 * direction overrides and tag characters. Code that needs them writes
 * escapes such as `\u{200B}`.
 */
function isHidden(code: number): boolean {
  return (
    (code <= 0x08 || code === 0x0b || code === 0x0c || (code >= 0x0e && code <= 0x1f)) ||
    (code >= 0x7f && code <= 0x9f) ||
    code === 0xad ||
    (code >= 0x200b && code <= 0x200f) ||
    (code >= 0x202a && code <= 0x202e) ||
    (code >= 0x2060 && code <= 0x2064) ||
    (code >= 0x2066 && code <= 0x2069) ||
    code === 0xfeff ||
    (code >= 0xe0000 && code <= 0xe007f)
  );
}

describe('source files', () => {
  it('contain no raw invisible or control characters', () => {
    const files = execFileSync('git', ['ls-files', '-z'], { cwd: ROOT, encoding: 'utf8' })
      .split('\0')
      .filter((file) => TEXT_FILES.test(file));
    const offenders: string[] = [];
    for (const file of files) {
      let text: string;
      try {
        text = readFileSync(path.join(ROOT, file), 'utf8');
      } catch {
        // Deleted in the working tree but still in the index.
        continue;
      }
      let line = 1;
      for (const char of text) {
        const code = char.codePointAt(0) ?? 0;
        if (code === 0x0a) {
          line += 1;
        } else if (isHidden(code)) {
          offenders.push(`${file}:${line}: U+${code.toString(16).toUpperCase().padStart(4, '0')}`);
        }
      }
    }
    expect(offenders).toEqual([]);
  });

  it('have no Rust strings whose line continuation got lost', () => {
    // A lost `\` at the end of a string line leaves the next line's
    // indentation in the text (seen after edits through tools).
    const files = execFileSync('git', ['ls-files', '-z', '*.rs'], { cwd: ROOT, encoding: 'utf8' }).split('\0').filter(Boolean);
    const offenders: string[] = [];
    for (const file of files) {
      let text: string;
      try {
        text = readFileSync(path.join(ROOT, file), 'utf8');
      } catch {
        continue;
      }
      text.split('\n').forEach((line, index) => {
        if (/"[^"]*\S {10,}\S[^"]*"/.test(line)) {
          offenders.push(`${file}:${index + 1}`);
        }
      });
    }
    expect(offenders).toEqual([]);
  });
});
