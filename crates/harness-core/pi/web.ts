/**
 * harness web_fetch for pi: reads web pages through the harness server,
 * the same way the web chat does. The server returns the readable text
 * (or only the sections or lines the model asks for), marked as untrusted
 * data, with prompt injection warnings.
 *
 * Requests go to `http://llm.harness/_web/fetch` with the URL in a header.
 * The project's gateway checks its host against the network rules first,
 * asking the user where they say so, exactly as for a direct connection,
 * and passes the checked host on to the server. The server loads nothing
 * else: a redirect to another host comes back here and is requested anew,
 * so the gateway checks that host as well. The device token is added by the
 * gateway; nothing secret is in here.
 *
 * `harness run` and the web chat load this file with `-e` from the tools
 * volume once the computer is paired with a server.
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const ENDPOINT = "http://llm.harness/_web/fetch";
/** Redirects to other hosts followed per call; the server handles the rest. */
const MAX_HOPS = 5;

const DESCRIPTION =
  "Load a web page over http(s) and return its readable text (HTML as Markdown; also plain text, Markdown, JSON, XML). " +
  "For HTML pages it returns only the main content when one stands out; set fullPage to get navigation and other links too. " +
  "To save tokens on long pages, set focus to what you look for when you need information on a topic: " +
  "you then get only the matching sections plus an outline, and can ask for other sections by number. " +
  "For exact figures (prices, dates, specifications, versions) set grep instead, to words that stand next to the figure, " +
  "such as \"price|€|per month\": you get only the lines with them, with their headings and table headers. " +
  "If grep does not find what you need, or you do not know which words to look for, load the page without grep and focus. " +
  "Only public hosts on ports 80 and 443 work. The harness server loads the page; its host must be allowed by the " +
  "project's network rules like any other connection, otherwise the user is asked. If the user declines, do not try again.";

const parameters = Type.Object({
  url: Type.String({ description: "Absolute http:// or https:// URL of the page" }),
  fullPage: Type.Optional(
    Type.Boolean({
      description: "Return the whole page including navigation, sidebars and footer instead of only the main content",
    }),
  ),
  focus: Type.Optional(
    Type.String({
      maxLength: 300,
      description: "What you look for on the page, as a short question or a few words; returns only the matching sections",
    }),
  ),
  grep: Type.Optional(
    Type.String({
      maxLength: 300,
      description:
        "Words or phrases of three or more characters to find on the page, separated by |; returns only the lines that contain them. Written as plain text: case is ignored, no regular expressions",
    }),
  ),
  sections: Type.Optional(
    Type.Array(Type.Integer({ minimum: 1, maximum: 100000 }), {
      maxItems: 30,
      description: "Numbers of sections to return, from the outline of an earlier call with focus",
    }),
  ),
});

/** What the server tells about a loaded page besides the model's text. */
interface PageDetails {
  url: string;
  status: number;
  contentType: string;
  title: string | null;
  truncated: boolean;
  warnings: string[];
}

type Answer = { text: string; page: PageDetails } | { redirect: string } | { error: string };

/** The URL as the gateway and the server expect it: absolute, http(s), ASCII. */
function normalize(raw: string): string {
  let url: URL;
  try {
    url = new URL(raw.trim());
  } catch {
    throw new Error(`Not a valid URL: ${raw}. Only absolute http:// and https:// URLs work.`);
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error(`The scheme ${url.protocol} is not supported; only http:// and https:// work.`);
  }
  url.hash = "";
  return url.href;
}

async function request(url: string, options: Record<string, unknown>, signal: AbortSignal | undefined): Promise<Answer> {
  let response: Response;
  try {
    response = await fetch(ENDPOINT, {
      method: "POST",
      headers: { "content-type": "application/json", "x-harness-url": url },
      body: JSON.stringify(options),
      ...(signal ? { signal } : {}),
    });
  } catch (error) {
    if (signal?.aborted) {
      throw error;
    }
    throw new Error(`The harness server is not reachable: ${error instanceof Error ? error.message : String(error)}`);
  }
  // The gateway refused the host (rules, the user, no server): its text
  // explains why and how the user can allow it.
  if (response.headers.get("x-harness-gateway")) {
    throw new Error((await response.text()).trim());
  }
  let answer: Answer;
  try {
    answer = (await response.json()) as Answer;
  } catch {
    throw new Error(`The harness server answered with HTTP ${response.status} and no result.`);
  }
  if (!response.ok && !("error" in answer)) {
    throw new Error(`The harness server answered with HTTP ${response.status}.`);
  }
  return answer;
}

export default function web(pi: ExtensionAPI): void {
  pi.registerTool({
    name: "web_fetch",
    label: "Web fetch",
    description: DESCRIPTION,
    promptSnippet: "Read a web page as clean text, or only the parts of it you need",
    promptGuidelines: [
      "Use web_fetch rather than curl or wget to read web pages: it returns readable text, can return only the sections or lines you need, and marks the content as untrusted.",
    ],
    parameters,
    async execute(_toolCallId, params, signal) {
      const { url: raw, ...options } = params;
      let url = normalize(raw);
      for (let hop = 0; hop <= MAX_HOPS; hop += 1) {
        const answer = await request(url, options, signal);
        if ("error" in answer) {
          throw new Error(answer.error);
        }
        if ("redirect" in answer) {
          url = normalize(answer.redirect);
          continue;
        }
        return { content: [{ type: "text", text: answer.text }], details: answer.page };
      }
      throw new Error(`Gave up after ${MAX_HOPS} redirects to other hosts.`);
    },
  });
}
