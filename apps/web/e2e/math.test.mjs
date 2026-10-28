// Cases for normalizeMath (src/features/chat/math.ts). The web app has no
// unit test runner; Node runs the TypeScript file directly:
//   node --test apps/web/e2e/math.test.mjs
// String.raw keeps the backslashes LaTeX needs.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { normalizeMath } from '../src/features/chat/math.ts';

const r = String.raw;

test('LaTeX delimiters become $$ outside code', () => {
  const cases = [
    [r`Die Formel \(E = mc^2\) gilt.`, r`Die Formel $$E = mc^2$$ gilt.`],
    ['Lösung:\n' + r`\[` + '\n x = ' + r`\frac{-b}{2a}` + '\n' + r`\]` + '\nfertig', 'Lösung:\n$$\nx = ' + r`\frac{-b}{2a}` + '\n$$\nfertig'],
    ['  ' + r`\[ a^2 + b^2 = c^2 \]`, '  $$\n  a^2 + b^2 = c^2\n  $$'],
    [r`mitten \[x\] im Satz`, 'mitten $$x$$ im Satz'],
    ['- Punkt ' + r`\(a\)` + '\n- zweiter', '- Punkt $$a$$\n- zweiter'],
  ];
  for (const [input, expected] of cases) {
    assert.equal(normalizeMath(input), expected);
  }
});

test('amounts, code and unfinished formulas stay as they are', () => {
  for (const input of [
    'Kosten $5 und $10',
    '`' + r`\(nicht\)` + '`',
    '```js\nconst a = "' + r`\\(x\\)` + '";\n```',
    '~~~~\n' + r`\(x\)` + '\n~~~~',
    r`offen \(x + `,
  ]) {
    assert.equal(normalizeMath(input), input);
  }
});
