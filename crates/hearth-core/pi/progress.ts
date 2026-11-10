/**
 * hearth progress list for pi: the agent keeps a short checklist of its
 * plan, and the web chat shows it live above the message box, so the user
 * sees where the agent stands without reading every tool call.
 *
 * The tool does nothing but answer briefly; the chat reads the list from the
 * call itself. Each call carries the whole list, which costs a few dozen
 * tokens and keeps the state in one place: the latest call.
 *
 * The web chat loads this file with `-e` from the tools volume; `hearth run`
 * in a terminal does not.
 */
import { StringEnum } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const DESCRIPTION =
  "Show the user your plan as a checklist; they follow it live in the web chat. " +
  "For tasks with three or more steps, call it at the start with all steps (the first one active), " +
  "then again whenever a step is done, a new one starts or the plan changes. " +
  "Send the whole list every time, at most one step active. Keep each step to a few words. " +
  "Before your final answer, send it once more with every finished step done and steps you dropped removed: " +
  "the user sees a list with open steps as unfinished work. " +
  "Do not use it for simple questions or single steps.";

const parameters = Type.Object({
  items: Type.Array(
    Type.Object({
      text: Type.String({ minLength: 1, maxLength: 200, description: "The step, in a few words" }),
      state: StringEnum(["pending", "active", "done"] as const, { description: "Its state" }),
    }),
    { minItems: 1, maxItems: 30, description: "All steps in order" },
  ),
});

export default function progress(pi: ExtensionAPI): void {
  pi.registerTool({
    name: "progress",
    label: "Progress",
    description: DESCRIPTION,
    promptSnippet: "Show the user a live checklist of your plan in the web chat",
    promptGuidelines: [
      "For tasks with several steps, keep the progress list current with the progress tool: the user follows it live.",
    ],
    parameters,
    async execute(_toolCallId, params) {
      const done = params.items.filter((item) => item.state === "done").length;
      const open = done < params.items.length ? " Check off the rest before your final answer." : "";
      // Short on purpose: the answer goes back to the model every time.
      return { content: [{ type: "text", text: `Shown: ${done}/${params.items.length} done.${open}` }], details: undefined };
    },
  });
}
