/**
 * harness documents for pi: `read_document` searches a document the user
 * attached in the web chat the way the chat models do, by meaning and
 * keywords (`focus`), by words (`grep`) or by section numbers. The
 * document's text and its embeddings live on the harness server, which
 * makes the embeddings once per document; pi finds the file itself in
 * `~/uploads` as well.
 *
 * Requests go to `http://llm.harness/_documents`; the gateway adds the device
 * token. The server answers only for the chats whose replies run on this
 * computer.
 *
 * The web chat loads this file with `-e` from the tools volume once the
 * computer is paired with a server; `harness run` has no attached files.
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const ENDPOINT = "http://llm.harness/_documents";

// The same text as the web chat's tool (apps/server/src/files/read.ts),
// plus where the file is.
const DESCRIPTION =
  "Reads parts of a long document attached to this chat. Name the file by the id given with it (or its name). " +
  "Set focus to what you look for to get the matching sections (by meaning and keywords), grep to words next to an exact figure to get those lines, " +
  "or sections to numbers from the outline. Ask again for other parts instead of guessing what the document says. " +
  "The whole text is also in ~/uploads/<name>.md; for long documents this tool is faster and reads less.";

// A server older than this tool answers with its own error object.
type Answer = { file: string; text: string } | { error: string | { message?: string } };

export default function documents(pi: ExtensionAPI): void {
  pi.registerTool({
    name: "read_document",
    label: "Read document",
    description: DESCRIPTION,
    promptSnippet: "Search a document the user attached by meaning, words or section numbers",
    parameters: Type.Object({
      file: Type.String({ minLength: 1, maxLength: 300, description: "The id given with the document, or its name" }),
      focus: Type.Optional(Type.String({ maxLength: 300, description: "What you look for, as a short question or a few words" })),
      grep: Type.Optional(Type.String({ maxLength: 300, description: "Words or phrases to find, separated by |" })),
      sections: Type.Optional(
        Type.Array(Type.Integer({ minimum: 1 }), { maxItems: 30, description: "Section numbers from the outline" }),
      ),
    }),
    async execute(_toolCallId, params, signal) {
      let response: Response;
      try {
        response = await fetch(ENDPOINT, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(params),
          ...(signal ? { signal } : {}),
        });
      } catch (error) {
        if (signal?.aborted) {
          throw error;
        }
        throw new Error(`The harness server is not reachable: ${error instanceof Error ? error.message : String(error)}`);
      }
      if (response.headers.get("x-harness-gateway")) {
        throw new Error((await response.text()).trim());
      }
      let answer: Answer;
      try {
        answer = (await response.json()) as Answer;
      } catch {
        throw new Error(`The harness server answered with HTTP ${response.status} and no result.`);
      }
      if ("error" in answer) {
        throw new Error(
          typeof answer.error === "string"
            ? answer.error
            : `The harness server cannot search documents (HTTP ${response.status}); read ~/uploads/<name>.md instead.`,
        );
      }
      return { content: [{ type: "text", text: answer.text }], details: { file: answer.file } };
    },
  });
}
