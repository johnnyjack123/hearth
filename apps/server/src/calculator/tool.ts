import { type CalculatorOutput, MAX_CALCULATIONS, MAX_EXPRESSION_LENGTH } from '@hearth/shared';
import { tool } from 'ai';
import { z } from 'zod';
import type { Calculator } from './engine.ts';

const DESCRIPTION = [
  'Evaluates math expressions exactly with math.js and returns each result.',
  'Use it for every calculation that is not trivial instead of computing in your head, and report its results.',
  'Syntax: arithmetic and powers (2^64, 7 mod 3), functions (sqrt, log, log10, exp, round(x, 2); trigonometry in radians or with units such as sin(30 deg)),',
  'units and conversion (5 km + 300 m to mi, 100 km/h to m/s, 3 kWh * 0.32 / kWh),',
  'exact numbers (bignumber("0.1") + bignumber("0.2"), fraction(1, 3) + fraction(1, 6)), matrices ([1, 2; 3, 4], det, inv, multiply, transpose),',
  'statistics (mean, median, std, variance, quantileSeq), combinatorics (combinations(49, 6), factorial), complex numbers (sqrt(-4)),',
  'equations through lusolve, and symbolic derivative("x^2 + 3x", "x") and simplify("2x + 3x").',
  'The expressions of one call share variables, e.g. ["r = 4 cm", "pi * r^2"].',
].join(' ');

const inputSchema = z.object({
  expressions: z
    .array(z.string().trim().min(1).max(MAX_EXPRESSION_LENGTH))
    .min(1)
    .max(MAX_CALCULATIONS)
    .describe('math.js expressions, evaluated in order'),
});

/**
 * What the model reads: one line per expression.
 *
 * @param output - The calculator's output.
 * @returns The text.
 */
export function calculatorText(output: CalculatorOutput): string {
  return output.results
    .map((entry) => (entry.error === undefined ? `${entry.expression} = ${entry.result ?? ''}` : `${entry.expression} → error: ${entry.error}`))
    .join('\n');
}

/**
 * The `calculate` tool of ordinary chats. Runs on the server, so it works
 * without a device and goes on while the page is closed.
 *
 * @param calculator - The math.js worker.
 * @returns The tool.
 */
export function createCalculatorTool(calculator: Calculator) {
  return tool({
    description: DESCRIPTION,
    inputSchema,
    // A CalculatorError (too slow, too big) reaches the model and the chat
    // as it is (describeStreamError).
    execute: async ({ expressions }): Promise<CalculatorOutput> => ({ results: await calculator.evaluate(expressions) }),
    toModelOutput: ({ output }) => ({ type: 'text', value: calculatorText(output) }),
  });
}
