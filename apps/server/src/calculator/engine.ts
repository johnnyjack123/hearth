import { createRequire } from 'node:module';
import { Worker } from 'node:worker_threads';
import type { CalculationResult } from '@hearth/shared';

/** Longest result text; a huge matrix would otherwise fill the context. */
const MAX_RESULT_LENGTH = 4_000;

/**
 * math.js in a worker of its own. What the model sends is untrusted (a page
 * it read may have asked for it), and some expressions take a lot of time
 * or memory (`ones(30000, 30000)`, `factorial(1e6)`): in the server's own
 * thread they would block or crash every chat on a Raspberry Pi. Plain
 * CommonJS run with `eval`, so the same code works under tsx, vitest and
 * the tsup bundle without a second entry point.
 *
 * Every call gets a fresh math.js instance (about 20 ms): expressions can
 * change their instance (`config(...)`, `createUnit`, `import`), and one
 * chat must not change the math of the next. Within the instance nothing
 * needs taking away, so simplify and derivative keep the parser they use.
 */
const WORKER_SOURCE = `
'use strict';
const { parentPort, workerData } = require('node:worker_threads');
const { create, all } = require(workerData.mathjs);
// Fixed notation up to 14 digits, as many as the precision shows; exact
// big integers need bignumber().
const FORMAT = { precision: 14, lowerExp: -9, upperExp: 14 };
function show(math, value) {
  if (value && value.isResultSet) {
    return value.entries.map((entry) => show(math, entry)).join('; ');
  }
  if (typeof value === 'function') {
    return 'function';
  }
  if (value === undefined) {
    return 'undefined';
  }
  return math.format(value, FORMAT);
}
parentPort.on('message', ({ id, expressions }) => {
  const math = create(all);
  // One scope per call, so "r = 4 cm" is known to the next expression.
  const scope = new Map();
  const results = expressions.map((expression) => {
    try {
      const text = show(math, math.evaluate(expression, scope));
      const result = text.length > workerData.max ? text.slice(0, workerData.max) + ' … (shortened)' : text;
      return { expression, result };
    } catch (error) {
      return { expression, error: error instanceof Error ? error.message : String(error) };
    }
  });
  parentPort.postMessage({ id, results });
});
// math.js is loaded; the time limit of calls starts from here.
parentPort.postMessage({ ready: true });
`;

/** A calculation that did not finish: too slow, too big, or the worker failed. */
export class CalculatorError extends Error {
  /**
   * @param code - Stable reason (`timeout`, `memory`, `failed`).
   * @param message - English text for the model.
   */
  constructor(
    readonly code: 'timeout' | 'memory' | 'failed',
    message: string,
  ) {
    super(message);
    this.name = 'CalculatorError';
  }
}

interface Pending {
  expressions: string[];
  resolve: (results: CalculationResult[]) => void;
  reject: (error: Error) => void;
  /** Set once the call went to the worker. */
  timer?: ReturnType<typeof setTimeout>;
}

/** A worker that never gets math.js loaded is given up after this long. */
const STARTUP_TIMEOUT_MS = 60_000;

/** Options of {@link Calculator}; tests shorten the time limit. */
export interface CalculatorOptions {
  /** How long one call may take. */
  timeoutMs?: number;
  /** Heap of the worker in MiB; math.js itself needs about 30. */
  memoryMb?: number;
}

/**
 * Evaluates math.js expressions in a worker that starts with the first
 * call and is replaced when a calculation runs too long or too big.
 */
export class Calculator {
  readonly #timeoutMs: number;
  readonly #memoryMb: number;
  readonly #pending = new Map<number, Pending>();
  #worker: Worker | null = null;
  /** Whether the worker has math.js loaded. */
  #ready = false;
  #next = 0;

  /**
   * @param options - Limits; see {@link CalculatorOptions}.
   */
  constructor(options: CalculatorOptions = {}) {
    this.#timeoutMs = options.timeoutMs ?? 3_000;
    this.#memoryMb = options.memoryMb ?? 128;
  }

  /**
   * Evaluates expressions in order; they share variables.
   *
   * @param expressions - math.js expressions.
   * @returns One result or error per expression.
   * @throws {CalculatorError} If the whole call took too long or too much
   *   memory; the worker is replaced then.
   */
  evaluate(expressions: string[]): Promise<CalculationResult[]> {
    this.#start();
    const id = ++this.#next;
    return new Promise<CalculationResult[]>((resolve, reject) => {
      this.#pending.set(id, { expressions, resolve, reject });
      if (this.#ready) {
        this.#send(id);
      }
    });
  }

  /**
   * Hands a waiting call to the worker. Its time limit starts here: loading
   * math.js takes seconds on a slow computer and must not count.
   */
  #send(id: number): void {
    const pending = this.#pending.get(id);
    const worker = this.#worker;
    if (!pending || !worker || pending.timer) {
      return;
    }
    pending.timer = setTimeout(() => {
      this.#stop(new CalculatorError('timeout', `The calculation took longer than ${this.#timeoutMs / 1000} s and was stopped. Try a smaller one.`));
    }, this.#timeoutMs);
    worker.postMessage({ id, expressions: pending.expressions });
  }

  /** Ends the worker; the next call starts a new one. */
  close(): void {
    this.#stop(new CalculatorError('failed', 'The calculator was shut down.'));
  }

  #start(): Worker {
    if (this.#worker) {
      return this.#worker;
    }
    const worker = new Worker(WORKER_SOURCE, {
      eval: true,
      workerData: { mathjs: MATHJS_ENTRY, max: MAX_RESULT_LENGTH },
      // Keys, database URL and server secret stay in the server's thread.
      env: {},
      resourceLimits: { maxOldGenerationSizeMb: this.#memoryMb, maxYoungGenerationSizeMb: 16, stackSizeMb: 4 },
    });
    // The server keeps the process alive; tests end without closing it.
    worker.unref();
    const startup = setTimeout(() => {
      if (this.#worker === worker && !this.#ready) {
        this.#stop(new CalculatorError('failed', 'The calculator did not start.'));
      }
    }, STARTUP_TIMEOUT_MS);
    startup.unref();
    worker.on('message', (message: { ready: true } | { id: number; results: CalculationResult[] }) => {
      if ('ready' in message) {
        clearTimeout(startup);
        this.#ready = true;
        for (const id of this.#pending.keys()) {
          this.#send(id);
        }
        return;
      }
      const pending = this.#pending.get(message.id);
      if (pending) {
        clearTimeout(pending.timer);
        this.#pending.delete(message.id);
        pending.resolve(message.results);
      }
    });
    worker.on('error', (error: Error & { code?: string }) => {
      this.#stop(
        error.code === 'ERR_WORKER_OUT_OF_MEMORY'
          ? new CalculatorError('memory', 'The calculation needed too much memory and was stopped. Try a smaller one.')
          : new CalculatorError('failed', `The calculator failed: ${error.message}`),
      );
    });
    worker.on('exit', () => {
      if (this.#worker === worker) {
        this.#stop(new CalculatorError('failed', 'The calculator stopped unexpectedly.'));
      }
    });
    this.#worker = worker;
    return worker;
  }

  /** Ends the worker and every call waiting for it. */
  #stop(reason: CalculatorError): void {
    const worker = this.#worker;
    this.#worker = null;
    this.#ready = false;
    void worker?.terminate();
    for (const pending of this.#pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(reason);
    }
    this.#pending.clear();
  }
}

/** math.js's CommonJS entry, for the worker's `require`. */
const MATHJS_ENTRY = createRequire(import.meta.url).resolve('mathjs');

let shared: Calculator | null = null;

/**
 * The calculator of the whole server; its worker starts with the first
 * calculation.
 *
 * @returns The shared instance.
 */
export function sharedCalculator(): Calculator {
  shared ??= new Calculator();
  return shared;
}
