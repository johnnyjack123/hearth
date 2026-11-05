/**
 * Provider-side tools in requests that devices send through the LLM
 * pass-through. Providers run some tools themselves: Anthropic's MCP
 * connector and web fetch, OpenAI's MCP and web search, Gemini's URL context,
 * OpenRouter's web plugin. Each makes the provider connect to a host the
 * request names, around the project's network rules and its gateway
 * (security review M1, 3.11.). Agents run their tools in the container, so
 * they only ever need tools the client executes.
 */

/** Anthropic tool types the client runs itself (pi's are plain custom tools). */
const ANTHROPIC_CLIENT_TOOL = /^(custom|(bash|text_editor|computer|memory)_\d{8})$/;

/** OpenAI tool types the client runs itself, in Chat Completions and Responses. */
const OPENAI_CLIENT_TOOLS = new Set(['function', 'custom']);

/** Gemini tool fields for functions the client runs itself. */
const GOOGLE_CLIENT_TOOL_FIELDS = new Set(['functionDeclarations', 'function_declarations']);

/** Beta flags of Anthropic that unlock provider-side tools or files. */
const ANTHROPIC_SERVER_BETA = /mcp|web-fetch|web-search|code-execution|files-api|skills|container/i;

function objectOf(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function toolsOf(request: Record<string, unknown>): Record<string, unknown>[] {
  return Array.isArray(request.tools) ? request.tools.map((tool) => objectOf(tool) ?? {}) : [];
}

/**
 * The first provider-side tool a request asks for.
 *
 * @param protocol - The wire protocol of the credential.
 * @param request - The parsed request body.
 * @returns A short name of the tool or field, or null if the request only has client tools.
 */
export function providerSideTool(protocol: 'openai' | 'anthropic' | 'google', request: unknown): string | null {
  const fields = objectOf(request);
  if (!fields) {
    return null;
  }
  switch (protocol) {
    case 'anthropic': {
      for (const field of ['mcp_servers', 'container']) {
        if (fields[field] !== undefined) {
          return field;
        }
      }
      const tool = toolsOf(fields).find((entry) => entry.type !== undefined && !(typeof entry.type === 'string' && ANTHROPIC_CLIENT_TOOL.test(entry.type)));
      return tool ? String(tool.type) : null;
    }
    case 'google': {
      for (const tool of toolsOf(fields)) {
        const other = Object.keys(tool).find((key) => !GOOGLE_CLIENT_TOOL_FIELDS.has(key));
        if (other) {
          return other;
        }
      }
      return null;
    }
    case 'openai': {
      if (fields.web_search_options !== undefined) {
        return 'web_search_options';
      }
      // OpenRouter: the web plugin and `:online` models search and fetch for the model.
      if (Array.isArray(fields.plugins) && fields.plugins.some((plugin) => objectOf(plugin)?.id === 'web')) {
        return 'plugins: web';
      }
      if (typeof fields.model === 'string' && /:online$/i.test(fields.model)) {
        return fields.model;
      }
      const tool = toolsOf(fields).find((entry) => !(typeof entry.type === 'string' && OPENAI_CLIENT_TOOLS.has(entry.type)));
      return tool ? String(tool.type ?? 'tool without type') : null;
    }
  }
}

/**
 * Anthropic's beta header without the flags for provider-side tools.
 *
 * @param value - The header as the client sent it.
 * @returns The flags left, or null if none are.
 */
export function clientBetas(value: string): string | null {
  const kept = value
    .split(',')
    .map((flag) => flag.trim())
    .filter((flag) => flag && !ANTHROPIC_SERVER_BETA.test(flag));
  return kept.length > 0 ? kept.join(',') : null;
}
