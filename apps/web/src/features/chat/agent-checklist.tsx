import type { DynamicToolUIPart, ToolUIPart } from 'ai';
import { Check, ChevronRight, Circle, CircleDot, ListChecks, X } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { ProgressBar } from '@/components/ui/display.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import type { ChatMessage } from '@/lib/api.ts';
import { transitions } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

/** Name of pi's progress tool (`crates/hearth-core/pi/progress.ts`). */
export const PROGRESS_TOOL = 'progress';

/** State of one step of an agent's checklist. */
export type ChecklistState = 'pending' | 'active' | 'done';

/** One step of an agent's checklist. */
export interface ChecklistItem {
  /** The step, in a few words. */
  text: string;
  /** Its state. */
  state: ChecklistState;
}

const STATES: readonly string[] = ['pending', 'active', 'done'];

/**
 * The checklist a `progress` call carries; the agent wrote it, so anything
 * of another shape is left out.
 *
 * @param input - The call's arguments.
 * @returns The steps, or null if there are none.
 */
export function readChecklist(input: unknown): ChecklistItem[] | null {
  const items = input && typeof input === 'object' ? (input as { items?: unknown }).items : undefined;
  if (!Array.isArray(items)) {
    return null;
  }
  const list = items.flatMap((item: unknown) => {
    const { text, state } = (item ?? {}) as { text?: unknown; state?: unknown };
    return typeof text === 'string' && typeof state === 'string' && STATES.includes(state) ? [{ text, state: state as ChecklistState }] : [];
  });
  return list.length > 0 ? list : null;
}

/** The checklist above the message box, named by the call that wrote its latest state. */
export interface CurrentChecklist {
  /** Conversation and tool call; a newer call of the model makes a new list. */
  id: string;
  /** Its steps. */
  items: ChecklistItem[];
}

/**
 * The checklist to show above the message box: the latest one, if it
 * belongs to the last reply, or to the one before while the last is still
 * being written and it has open steps. A reply that ended without touching
 * it means the model moved on and forgot it (seen 3.11.); a list it finished
 * earlier belongs to an earlier task.
 *
 * @param conversationId - The chat, for the list's id.
 * @param messages - The chat's messages.
 * @param live - Whether a reply is being written.
 * @returns The list, or null.
 */
export function currentChecklist(conversationId: string, messages: ChatMessage[], live: boolean): CurrentChecklist | null {
  let lastReply = true;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message?.role !== 'assistant') {
      continue;
    }
    // An agent's list is a dynamic tool, the chat model's its own tool.
    const part = message.parts.findLast(
      (candidate): candidate is DynamicToolUIPart | ToolUIPart =>
        ((candidate.type === 'dynamic-tool' && candidate.toolName === PROGRESS_TOOL) || candidate.type === `tool-${PROGRESS_TOOL}`) &&
        candidate.state !== 'output-error',
    );
    const items = part ? readChecklist(part.input) : null;
    if (part && items) {
      return lastReply || items.some((item) => item.state !== 'done') ? { id: `${conversationId}:${part.toolCallId}`, items } : null;
    }
    if (!(lastReply && live)) {
      return null;
    }
    lastReply = false;
  }
  return null;
}

const HIDDEN_KEY = 'hearth.hiddenChecklists';

/** Lists remembered as hidden; older ones belong to long finished tasks. */
const MAX_HIDDEN = 200;

/**
 * Checklists the user hid in this browser.
 *
 * @returns Their ids (see {@link CurrentChecklist}).
 */
export function hiddenChecklists(): Set<string> {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(HIDDEN_KEY) ?? '[]');
    return new Set(Array.isArray(stored) ? stored.filter((id): id is string => typeof id === 'string') : []);
  } catch {
    return new Set();
  }
}

/**
 * Hides a checklist until the model writes a new state of it.
 *
 * @param id - The list's id.
 * @returns All hidden lists, this one included.
 */
export function hideChecklist(id: string): Set<string> {
  const hidden = [...hiddenChecklists(), id].slice(-MAX_HIDDEN);
  try {
    localStorage.setItem(HIDDEN_KEY, JSON.stringify(hidden));
  } catch {
    // Private mode: hidden for this page only.
  }
  return new Set(hidden);
}

function counts(items: ChecklistItem[]): { done: number; total: number; active: ChecklistItem | undefined } {
  return {
    done: items.filter((item) => item.state === 'done').length,
    total: items.length,
    active: items.find((item) => item.state === 'active'),
  };
}

/**
 * The steps of a checklist; the active one spins while the agent works.
 *
 * @param props - The steps and whether the agent is answering.
 * @returns The list.
 */
export function ChecklistItems({ items, live }: { items: ChecklistItem[]; live: boolean }) {
  const t = useT();
  return (
    <ol className="flex flex-col gap-1">
      {items.map((item, index) => (
        <li key={index} className="flex min-w-0 items-start gap-2 text-sm">
          {item.state === 'done' ? (
            <Check className="mt-0.5 size-4 shrink-0 text-primary" aria-label={t('agent.checklist.done')} />
          ) : item.state === 'active' ? (
            live ? (
              <span role="img" aria-label={t('agent.checklist.active')} className="mt-0.5 flex shrink-0">
                <Spinner className="size-4 text-primary" />
              </span>
            ) : (
              <CircleDot className="mt-0.5 size-4 shrink-0 text-primary" aria-label={t('agent.checklist.active')} />
            )
          ) : (
            <Circle className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-label={t('agent.checklist.pending')} />
          )}
          <span
            className={cn(
              'min-w-0 break-words',
              item.state === 'done' && 'text-muted-foreground line-through',
              item.state === 'active' && 'font-medium text-foreground',
              item.state === 'pending' && 'text-muted-foreground',
            )}
          >
            {item.text}
          </span>
        </li>
      ))}
    </ol>
  );
}

/**
 * The agent's current checklist above the message box: a bar with the
 * count and the active step, the steps below on demand. The user can hide
 * it, e.g. when the model forgot to check off its last steps.
 *
 * @param props - The steps, whether the agent is answering, and what hiding does.
 * @returns The panel.
 */
export function ChecklistPanel({ items, live, onHide }: { items: ChecklistItem[]; live: boolean; onHide: () => void }) {
  const t = useT();
  const [open, setOpen] = useState(true);
  const { done, total, active } = counts(items);
  // The reply ended with steps still open: the model will not check them off.
  const stalled = !live && done < total;
  return (
    <section aria-label={t('agent.checklist.title')} className="mb-2 flex min-w-0 flex-col rounded-lg border border-border bg-card">
      <div className="flex min-w-0 items-center gap-1 pe-1">
        <button
          type="button"
          onClick={() => setOpen(!open)}
          aria-expanded={open}
          aria-label={open ? t('agent.checklist.hide') : t('agent.checklist.show')}
          title={t('agent.checklist.hint')}
          className="flex min-w-0 flex-1 items-center gap-2 rounded-lg px-3 py-2 text-start text-sm transition-colors duration-(--duration-instant) ease-standard hover:bg-accent focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
        >
          <ListChecks className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span className="shrink-0 font-medium text-foreground">{t('agent.checklist.title')}</span>
          <span className="min-w-0 flex-1 truncate text-muted-foreground">
            {stalled ? t('agent.checklist.stalled') : active ? active.text : null}
          </span>
          <ProgressBar value={done} max={total} label={t('agent.checklist.count', { done, total })} className="w-16 shrink-0 sm:w-24" />
          <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
            {done}/{total}
          </span>
          <ChevronRight
            className={cn('size-3.5 shrink-0 text-muted-foreground transition-transform duration-(--duration-fast) ease-standard', open && 'rotate-90')}
            aria-hidden="true"
          />
        </button>
        <Button variant="ghost" size="icon-sm" onClick={onHide} aria-label={t('agent.checklist.dismiss')} title={t('agent.checklist.dismissHint')}>
          <X aria-hidden="true" />
        </Button>
      </div>
      <AnimatePresence initial={false}>
        {open ? (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1, transition: transitions.enter }}
            exit={{ opacity: 0, transition: transitions.exit }}
            className="max-h-48 overflow-y-auto border-t border-border px-3 py-2"
          >
            <ChecklistItems items={items} live={live} />
          </motion.div>
        ) : null}
      </AnimatePresence>
    </section>
  );
}

/**
 * A `progress` call in the reply: one quiet line with the count, the list
 * on demand. The panel above the message box shows the current one.
 *
 * @param props - The tool part and whether the reply is streaming.
 * @returns The line.
 */
export function ChecklistUpdate({ items, live }: { items: ChecklistItem[]; live: boolean }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const { done, total } = counts(items);
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        className="flex min-w-0 items-center gap-2 self-start rounded-md text-sm text-muted-foreground transition-colors duration-(--duration-instant) ease-standard hover:text-foreground focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
      >
        <ListChecks className="size-4 shrink-0" aria-hidden="true" />
        <span>{t('agent.checklist.update', { done, total })}</span>
        <ProgressBar value={done} max={total} label={t('agent.checklist.count', { done, total })} className="w-16 shrink-0" />
        <ChevronRight
          className={cn('size-3.5 shrink-0 transition-transform duration-(--duration-fast) ease-standard', open && 'rotate-90')}
          aria-hidden="true"
        />
      </button>
      {open ? (
        <div className="ms-6">
          <ChecklistItems items={items} live={live} />
        </div>
      ) : null}
    </div>
  );
}
