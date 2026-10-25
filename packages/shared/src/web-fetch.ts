/**
 * Name of the server-side tool that loads web pages in ordinary chats. Its
 * parts are stored as `tool-web_fetch` in the chat history.
 */
export const WEB_FETCH_TOOL = 'web_fetch';

/**
 * Answers to a web fetch question in the chat: this URL once, this host for
 * the rest of the chat, or no. A subset of the gateway's choices.
 */
export const WEB_FETCH_DECISIONS = ['once', 'session', 'deny'] as const;

/** See {@link WEB_FETCH_DECISIONS}. */
export type WebFetchDecision = (typeof WEB_FETCH_DECISIONS)[number];

/**
 * How a web fetch question ended: one of the user's answers, or `timeout`
 * when nobody answered in time (counts as no).
 */
export type WebFetchAnswer = WebFetchDecision | 'timeout';

/**
 * Patterns of a possible prompt injection found in fetched content; the web
 * app shows them as a warning, the model gets them as a note.
 */
export const WEB_FETCH_WARNINGS = [
  'ignore_instructions',
  'role_reassignment',
  'injected_instructions',
  'addressed_to_ai',
  'role_markers',
  'html_comment',
  'hide_from_user',
  'unicode_tags',
  'bidi_controls',
  'zero_width',
] as const;

/** See {@link WEB_FETCH_WARNINGS}. */
export type WebFetchWarning = (typeof WEB_FETCH_WARNINGS)[number];

/**
 * One place in fetched content where an injection pattern matched, so the
 * user can judge it. The texts are as found, with runs of whitespace
 * collapsed: invisible characters stay in them, and the web app has to make
 * them visible before showing them.
 */
export interface InjectionFinding {
  /** Which pattern matched. */
  kind: WebFetchWarning;
  /** Text before the match, starting with `…` when cut. */
  before: string;
  /** The match, shortened in the middle when long. */
  match: string;
  /** Text after the match, ending with `…` when cut. */
  after: string;
}

/**
 * Jev's second opinion on a prompt injection warning, stored with the tool
 * output it is about. It stands next to the warning and never replaces it:
 * the content may address Jev as well.
 */
export interface InjectionAssessment {
  /** Jev's model. */
  model: string;
  /** How likely it is an attack, from 0 (false alarm) to 1. */
  danger: number;
  /** Cost in US dollars, if OpenRouter reported it. */
  cost: number | null;
  /** When it was asked, epoch milliseconds. */
  at: number;
}

/**
 * Why a web fetch failed. The error text of a failed `web_fetch` call starts
 * with `[code]` or `[code:detail]`, followed by an English explanation for
 * the model; see {@link parseWebFetchError}.
 */
export const WEB_FETCH_ERRORS = [
  'invalid_url',
  'blocked_address',
  'blocked_port',
  'denied',
  'no_answer',
  'aborted',
  'timeout',
  'too_many_redirects',
  'http_error',
  'unsupported_type',
  'unsupported_encoding',
  'network',
] as const;

/** See {@link WEB_FETCH_ERRORS}. */
export type WebFetchErrorCode = (typeof WEB_FETCH_ERRORS)[number];

/**
 * Result of a successful `web_fetch` call as stored in the chat. The model
 * receives it wrapped as untrusted data, never as plain text.
 */
export interface WebFetchOutput {
  /** URL the content came from, after redirects. */
  url: string;
  /** HTTP status of the final response. */
  status: number;
  /** Media type without parameters, e.g. `text/html`. */
  contentType: string;
  /** Title of an HTML page. */
  title: string | null;
  /** Readable text, Markdown for HTML, possibly shortened. */
  text: string;
  /** True if the download or the text was cut off at a limit. */
  truncated: boolean;
  /**
   * True if {@link text} is only the main content of an HTML page, without
   * navigation, sidebars and footer; missing in replies stored before
   * 2026-10-18.
   */
  mainContent?: boolean;
  /** Possible prompt injection patterns in the content. */
  warnings: WebFetchWarning[];
  /**
   * Where the patterns of {@link warnings} matched; missing in replies
   * stored before 2026-10-18.
   */
  findings?: InjectionFinding[];
  /** Jev's opinion on the warnings, if asked. */
  assessment?: InjectionAssessment;
  /** What the model looked for, when it asked for matching sections only. */
  focus?: string;
  /** Set when the model asked for the lines with certain words only. */
  grep?: WebFetchGrep;
  /**
   * Set when {@link text} holds only some sections of a long page; the
   * model can ask for others by number.
   */
  sections?: WebFetchSections;
}

/** What a `web_fetch` call with `grep` returned. */
export interface WebFetchGrep {
  /** The words, separated by `|`, as the model wrote them. */
  pattern: string;
  /** How many lines matched; null when the page was short and came whole. */
  lines: number | null;
  /** True if more lines matched than fit. */
  truncated: boolean;
  /** Words left out because they were too short to mean anything ("W", "24"). */
  ignored?: string[];
}

/**
 * Which sections of a page a `web_fetch` call returned. Sections are
 * numbered from 1 in page order; long ones are split at paragraphs.
 */
export interface WebFetchSections {
  /** How many sections the page has. */
  total: number;
  /** Numbers of the returned sections. */
  shown: number[];
  /**
   * Consecutive sections under the same heading, one entry each; an empty
   * heading stands for the part before the first heading.
   */
  outline: { from: number; to: number; heading: string }[];
  /** False when nothing matched the focus and the page's beginning stands in. */
  matched: boolean;
  /**
   * How a focus was matched: by keywords alone, or also by meaning with the
   * embedding model; missing for sections asked for by number and for
   * replies stored before 23.10.2026.
   */
  ranking?: 'keywords' | 'meaning';
}

const ERROR_PREFIX = /^\[([a-z_]+)(?::([^\]]{0,200}))?\]\s*/;

/**
 * Reads the code and detail from the error text of a failed `web_fetch` call.
 *
 * @param errorText - Error text of the tool part.
 * @returns Code, optional detail (e.g. the HTTP status) and the rest of the
 *   text, or null if the text has no known code.
 */
export function parseWebFetchError(
  errorText: string,
): { code: WebFetchErrorCode; detail: string | null; message: string } | null {
  const match = ERROR_PREFIX.exec(errorText);
  const code = match?.[1] as WebFetchErrorCode | undefined;
  if (!match || !code || !WEB_FETCH_ERRORS.includes(code)) {
    return null;
  }
  return { code, detail: match[2] ?? null, message: errorText.slice(match[0].length) };
}
