import type { InjectionFinding, WebFetchWarning } from '@harness/shared';
import type { ReactNode } from 'react';
import { useT } from '@/lib/preferences.tsx';

/** A piece of found text: plain, an invisible character, or text hidden in tag characters. */
export type RevealedPart = { type: 'text'; text: string } | { type: 'char'; code: string } | { type: 'tags'; text: string };

// Control, zero-width, direction and tag characters. Shown as they are,
// they would hide text or reorder what the user reads.
const HIDDEN =
  /([\u{E0000}-\u{E007F}]+)|([\u0000-\u001F\u007F-\u009F​-‏‪-‮⁠-⁤⁦-⁩﻿])/gu;

/**
 * Splits text so invisible characters can be shown: each one as its code
 * point, a run of tag characters as the ASCII text it spells.
 *
 * @param text - Text from a finding.
 * @returns The parts in order.
 */
export function revealHidden(text: string): RevealedPart[] {
  const parts: RevealedPart[] = [];
  let last = 0;
  for (const match of text.matchAll(HIDDEN)) {
    if (match.index > last) {
      parts.push({ type: 'text', text: text.slice(last, match.index) });
    }
    if (match[1]) {
      const hidden = Array.from(match[1], (char) => (char.codePointAt(0) ?? 0) - 0xe0000)
        .filter((code) => code >= 0x20 && code < 0x7f)
        .map((code) => String.fromCharCode(code))
        .join('');
      parts.push({ type: 'tags', text: hidden });
    } else {
      const code = (match[2] ?? '').charCodeAt(0);
      parts.push({ type: 'char', code: `U+${code.toString(16).toUpperCase().padStart(4, '0')}` });
    }
    last = match.index + match[0].length;
  }
  if (last < text.length) {
    parts.push({ type: 'text', text: text.slice(last) });
  }
  return parts;
}

function Revealed({ text }: { text: string }) {
  const t = useT();
  return revealHidden(text).map((part, index): ReactNode => {
    if (part.type === 'text') {
      return part.text;
    }
    const label = part.type === 'tags' ? t('webFetch.hiddenText', { text: part.text }) : t('webFetch.hiddenChar', { code: part.code });
    return (
      <span key={index} className="mx-0.5 rounded-sm bg-destructive-subtle px-1 font-sans text-destructive">
        {part.type === 'tags' ? label : part.code}
        {part.type === 'char' ? <span className="sr-only">{label}</span> : null}
      </span>
    );
  });
}

/**
 * Where content looked like a prompt injection: per kind its name, a
 * sentence on why it is dangerous, and the places with their surroundings,
 * the match highlighted and invisible characters made visible.
 *
 * @param props - The kinds of warnings, and the findings if they were stored.
 * @returns The list.
 */
export function InjectionFindings({ warnings, findings }: { warnings: WebFetchWarning[]; findings: InjectionFinding[] | undefined }) {
  const t = useT();
  return (
    <div className="flex flex-col gap-3">
      <ul className="flex flex-col gap-3">
        {warnings.map((kind) => (
          <li key={kind} className="flex flex-col gap-1.5">
            <p className="break-words">
              <span className="font-medium">{t(`webFetch.warnings.${kind}`)}</span>
              {' · '}
              {t(`webFetch.why.${kind}`)}
            </p>
            {(findings ?? [])
              .filter((finding) => finding.kind === kind)
              .map((finding, index) => (
                <p
                  key={index}
                  className="wrap-anywhere rounded-md bg-card px-2 py-1.5 font-mono text-xs text-card-foreground"
                >
                  <Revealed text={finding.before} />
                  <mark className="rounded-sm bg-warning px-0.5 font-medium text-warning-foreground underline decoration-2 underline-offset-2">
                    <Revealed text={finding.match} />
                  </mark>
                  <Revealed text={finding.after} />
                </p>
              ))}
          </li>
        ))}
      </ul>
      {findings === undefined ? <p className="text-xs">{t('webFetch.findingsMissing')}</p> : null}
    </div>
  );
}
