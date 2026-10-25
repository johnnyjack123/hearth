/**
 * Jev's second opinion on prompt injection warnings in chat replies: what
 * the warning of a tool call is about, and asking Jev about it. The opinion
 * is stored with the tool's output, which the model never sees: its text
 * comes from `modelText` and friends, which leave it out.
 */
import { WEB_FETCH_TOOL, WEB_SEARCH_TOOL, type AgentToolOutput, type InjectionAssessment, type WebFetchOutput, type WebSearchOutput } from '@harness/shared';
import { assessInjection, type WarnedContent } from './helpers/injection.ts';
import type { Helpers } from './helpers/openrouter.ts';
import type { ChatMessage } from './services/conversations.ts';
import { findInjections } from './webfetch/injection.ts';
import { modelText } from './webfetch/tool.ts';
import { searchModelText } from './websearch/tool.ts';

/** Asks Jev about one warning; null if Jev did not answer. */
export type Assessor = (warned: WarnedContent) => Promise<InjectionAssessment | null>;

/**
 * An {@link Assessor} with the user's helpers.
 *
 * @param helpers - The user's helpers; without them nothing is assessed.
 * @param signal - Ends the calls, e.g. with the request.
 * @returns The assessor, or null without helpers.
 */
export function assessor(helpers: Helpers | null, signal?: AbortSignal): Assessor | null {
  if (!helpers) {
    return null;
  }
  return async (warned) => {
    const result = await assessInjection(helpers, warned, signal);
    return result ? { model: helpers.jevModel, danger: result.danger, cost: result.cost, at: Date.now() } : null;
  };
}

type Part = ChatMessage['parts'][number];

/**
 * What the warning of a stored tool call is about: the content as the model
 * read it and the flagged places.
 *
 * @param part - A part of a reply.
 * @returns The warned content, or null if the part has no warning.
 */
export function warnedContent(part: Part): WarnedContent | null {
  if (part.type === `tool-${WEB_FETCH_TOOL}` && part.state === 'output-available') {
    const output = part.output as WebFetchOutput;
    const findings = output.findings ?? [];
    return findings.length > 0 ? { source: 'a web page', location: output.url, content: modelText(output), findings } : null;
  }
  if (part.type === `tool-${WEB_SEARCH_TOOL}` && part.state === 'output-available') {
    const output = part.output as WebSearchOutput;
    return output.findings.length > 0
      ? { source: 'web search results', location: `search: ${output.query}`, content: searchModelText(output), findings: output.findings }
      : null;
  }
  if (part.type === 'dynamic-tool' && part.state === 'output-available') {
    const output = part.output as AgentToolOutput;
    return agentWarning(part.toolName, part.input, output.text);
  }
  return null;
}

/**
 * The warning of an agent's tool call, from the server's own check of its
 * output (the device's guard only reports a notice).
 *
 * @param toolName - The tool.
 * @param input - Its arguments.
 * @param text - What it returned.
 * @returns The warned content, or null if nothing matched.
 */
export function agentWarning(toolName: string, input: unknown, text: string): WarnedContent | null {
  const findings = findInjections(text);
  if (findings.length === 0) {
    return null;
  }
  const call = JSON.stringify(input ?? {}).slice(0, 300);
  return { source: `the output of the tool ${toolName} of an AI coding agent`, location: `${toolName} ${call}`, content: text, findings };
}

/**
 * The part with Jev's opinion stored in its output.
 *
 * @param part - A tool part with output.
 * @param assessment - Jev's opinion.
 * @returns The changed part.
 */
export function withAssessment(part: Part, assessment: InjectionAssessment): Part {
  if (!('output' in part) || typeof part.output !== 'object' || part.output === null) {
    return part;
  }
  return { ...part, output: { ...part.output, assessment } } as Part;
}
