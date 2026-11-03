// Cases for documentReadInfo (src/features/chat/document-info.ts): how the
// chat reads back what the server says about a document read. The web app
// has no unit test runner; Node runs the TypeScript file directly:
//   node --test apps/web/e2e/document-info.test.mjs
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { documentReadInfo } from '../src/features/chat/document-info.ts';

test('sections by meaning, by keywords, by number and without a match', () => {
  assert.deepEqual(documentReadInfo('From "bohnen.md": sections 3, 241 of 261, chosen by meaning and keywords\n[§3]\n…'), {
    name: 'bohnen.md',
    kind: 'sections',
    shown: 2,
    total: 261,
    ranking: 'meaning',
    matched: true,
  });
  assert.equal(documentReadInfo('From "a.pdf": sections 1 of 9, chosen by keywords').ranking, 'keywords');
  assert.equal(documentReadInfo('From "a.pdf": sections 4, 5 of 9').ranking, undefined);
  assert.equal(documentReadInfo('From "a.pdf": sections 1, 2 of 9, chosen by keywords (nothing matched the focus; this is the start)').matched, false);
});

test('exact words, the whole document, and names with quotes', () => {
  assert.deepEqual(documentReadInfo('From "a.pdf": 3 matching lines for "Minuten", more than fit\n…'), {
    name: 'a.pdf',
    kind: 'grep',
    lines: 3,
    matched: true,
  });
  assert.equal(documentReadInfo('From "a.pdf": no line contains "x".').lines, 0);
  assert.equal(documentReadInfo('From "notiz.txt": the whole document, it is short\nBohnen').kind, 'whole');
  assert.equal(documentReadInfo(`From ${JSON.stringify('Plan "B": neu.md')}: the whole document, it is short`).name, 'Plan "B": neu.md');
});

test('older replies keep their name, other text is no read', () => {
  assert.deepEqual(documentReadInfo('From "alt.md":\nText'), { name: 'alt.md', kind: null, matched: true });
  assert.equal(documentReadInfo('README.md'), null);
  assert.equal(documentReadInfo('From somewhere: x'), null);
});
