/** Name of the calculator tool of ordinary chats (math.js on the server). */
export const CALCULATOR_TOOL = 'calculate';

/** Most expressions in one call of the calculator. */
export const MAX_CALCULATIONS = 20;

/** Longest expression the calculator takes. */
export const MAX_EXPRESSION_LENGTH = 2_000;

/** One expression the calculator evaluated. */
export interface CalculationResult {
  /** The expression as the model wrote it. */
  expression: string;
  /** The result as math.js formats it; missing if it failed. */
  result?: string;
  /** Why it failed, from math.js (English). */
  error?: string;
}

/** Output of the calculator tool. */
export interface CalculatorOutput {
  /** One entry per expression, in order. */
  results: CalculationResult[];
}
