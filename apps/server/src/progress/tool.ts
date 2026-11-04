import { tool } from 'ai';
import { z } from 'zod';

/** Name of the progress list tool; pi's has the same (`pi/progress.ts`). */
export const PROGRESS_TOOL = 'progress';

/** Like pi's description, for the chat model. */
const DESCRIPTION =
  'Show the user your plan as a checklist; they follow it live above the message box. ' +
  'For tasks with three or more steps that use tools (research, several calculations, reading several pages), ' +
  'call it at the start with all steps (the first one active), then again whenever a step is done or the plan changes. ' +
  'Send the whole list every time, at most one step active. Keep each step to a few words. ' +
  'Before your final answer, send it once more with every finished step done and steps you dropped removed: ' +
  'the user sees a list with open steps as unfinished work. ' +
  'Do not use it for simple questions or single steps.';

/**
 * Part of the system prompt whenever the tool exists, so it stays the same
 * for the whole chat.
 */
export const PROGRESS_INSTRUCTIONS =
  'For tasks with three or more steps that need tools, keep the user informed with the progress tool; never for simple questions.';

const inputSchema = z.object({
  items: z
    .array(
      z.object({
        text: z.string().trim().min(1).max(200).describe('The step, in a few words'),
        state: z.enum(['pending', 'active', 'done']).describe('Its state'),
      }),
    )
    .min(1)
    .max(30)
    .describe('All steps in order'),
});

/** What the tool returns: how far the list is. */
export interface ProgressOutput {
  /** Steps done. */
  done: number;
  /** All steps. */
  total: number;
}

/**
 * The model's line for a list it showed. Short on purpose: it goes back to
 * the model with every later call.
 *
 * @param output - The tool's output.
 * @returns The text.
 */
export function progressText(output: ProgressOutput): string {
  const open = output.done < output.total ? ' Check off the rest before your final answer.' : '';
  return `Shown: ${output.done}/${output.total} done.${open}`;
}

/**
 * The progress list of ordinary chats. It does nothing but answer briefly;
 * the chat reads the list from the call itself.
 *
 * @returns The tool.
 */
export function createProgressTool() {
  return tool({
    description: DESCRIPTION,
    inputSchema,
    execute: async ({ items }): Promise<ProgressOutput> => ({ done: items.filter((item) => item.state === 'done').length, total: items.length }),
    toModelOutput: ({ output }) => ({ type: 'text', value: progressText(output) }),
  });
}
