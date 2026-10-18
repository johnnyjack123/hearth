import type { AgentDataParts, AgentToolOutput, ApprovalDecision } from '@harness/shared';
import type { DynamicToolUIPart } from 'ai';
import { Check, ChevronRight, CircleAlert, FilePen, FileText, Globe, Info, SquareTerminal, TriangleAlert, Wrench } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Badge, Card } from '@/components/ui/display.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import { errorMessage } from '@/lib/api.ts';
import { transitions } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

function stringField(input: unknown, key: string): string | undefined {
  if (input && typeof input === 'object' && key in input) {
    const value = (input as Record<string, unknown>)[key];
    return typeof value === 'string' ? value : undefined;
  }
  return undefined;
}

/**
 * One line that says what a tool call does, e.g. `$ npm test` or the path
 * of a file the agent edits.
 *
 * @param part - The tool part.
 * @returns The summary and an icon for it.
 */
export function describeTool(part: DynamicToolUIPart): { icon: typeof Wrench; summary: string } {
  const command = stringField(part.input, 'command');
  const path = stringField(part.input, 'path') ?? stringField(part.input, 'file_path');
  if (part.toolName === 'bash' && command) {
    return { icon: SquareTerminal, summary: `$ ${command.split('\n')[0]}` };
  }
  if (path && (part.toolName === 'edit' || part.toolName === 'write')) {
    return { icon: FilePen, summary: `${part.toolName} ${path}` };
  }
  if (path) {
    return { icon: FileText, summary: `${part.toolName} ${path}` };
  }
  return { icon: Wrench, summary: part.toolName };
}

function outputText(part: DynamicToolUIPart): string {
  if (part.state === 'output-error') {
    return part.errorText;
  }
  if (part.state === 'output-available') {
    const output = part.output as Partial<AgentToolOutput> | undefined;
    return typeof output?.text === 'string' ? output.text : '';
  }
  return '';
}

/**
 * A tool call of an agent: a one-line summary that expands to input and
 * output. Failed calls open by themselves.
 *
 * @param props - The tool part and whether its message is still streaming.
 * @returns The tool block.
 */
export function ToolCall({ part, streaming }: { part: DynamicToolUIPart; streaming: boolean }) {
  const t = useT();
  const failed = part.state === 'output-error';
  const running =
    streaming &&
    (part.state === 'input-available' ||
      part.state === 'input-streaming' ||
      (part.state === 'output-available' && part.preliminary === true));
  const [open, setOpen] = useState<boolean | null>(null);
  const expanded = open ?? failed;
  const { icon: Icon, summary } = describeTool(part);
  const output = outputText(part);
  const command = stringField(part.input, 'command');
  // bash shows its command in full; other tools show their arguments.
  const input = part.toolName === 'bash' ? (command?.includes('\n') ? command : null) : JSON.stringify(part.input, null, 2);

  return (
    <div className="flex min-w-0 flex-col rounded-lg border border-border bg-card text-sm">
      <button
        type="button"
        onClick={() => setOpen(!expanded)}
        aria-expanded={expanded}
        aria-label={expanded ? t('agent.tool.hideOutput') : t('agent.tool.showOutput')}
        className="flex min-w-0 items-center gap-2 rounded-lg px-3 py-2 text-start transition-colors duration-(--duration-instant) ease-standard hover:bg-accent focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
      >
        <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground">{summary}</span>
        {running ? (
          <span className="inline-flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
            <Spinner className="size-3.5" />
            {t('agent.tool.running')}
          </span>
        ) : failed ? (
          <Badge variant="destructive" className="shrink-0">
            <CircleAlert aria-hidden="true" />
            {t('agent.tool.failed')}
          </Badge>
        ) : (
          <Check className="size-4 shrink-0 text-muted-foreground" aria-label={t('agent.tool.done')} />
        )}
        <ChevronRight
          className={cn('size-3.5 shrink-0 text-muted-foreground transition-transform duration-(--duration-fast) ease-standard', expanded && 'rotate-90')}
          aria-hidden="true"
        />
      </button>
      <AnimatePresence initial={false}>
        {expanded ? (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1, transition: transitions.enter }}
            exit={{ opacity: 0, transition: transitions.exit }}
            className="flex flex-col gap-2 border-t border-border px-3 py-2"
          >
            {input ? (
              <ToolText label={t('agent.tool.input')} text={input} />
            ) : null}
            <ToolText label={t('agent.tool.output')} text={output || t('agent.tool.noOutput')} />
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}

function ToolText({ label, text }: { label: string; text: string }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted px-2 py-1.5 font-mono text-xs text-foreground">
        {text}
      </pre>
    </div>
  );
}

/**
 * A step on the way to an agent's answer (computer reached, VM, sandbox,
 * agent thinking): a spinner while it runs, so a slow start never looks
 * like silence.
 *
 * @param props - The step.
 * @returns The step row.
 */
export function AgentProgress({ progress }: { progress: AgentDataParts['progress'] }) {
  const t = useT();
  return (
    <div role="status" className="flex min-w-0 items-start gap-2 text-sm text-muted-foreground">
      {progress.state === 'running' ? (
        <Spinner className="mt-0.5 size-4 shrink-0" />
      ) : progress.state === 'failed' ? (
        <CircleAlert className="mt-0.5 size-4 shrink-0 text-destructive" aria-label={t('agent.progressFailed')} />
      ) : (
        <Check className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
      )}
      <span className="min-w-0 break-words">
        <span className={cn(progress.state === 'failed' && 'text-destructive')}>{progress.label}</span>
        {progress.detail ? <span className="ms-2 text-xs">{progress.detail}</span> : null}
      </span>
    </div>
  );
}

/**
 * A message of harness or the harness in an agent reply.
 *
 * @param props - The notice.
 * @returns The notice row.
 */
export function AgentNotice({ notice }: { notice: AgentDataParts['notice'] }) {
  const Icon = notice.level === 'error' ? CircleAlert : notice.level === 'warning' ? TriangleAlert : Info;
  return (
    <div
      role={notice.level === 'info' ? undefined : 'note'}
      className={cn(
        'flex items-start gap-2 rounded-lg px-3 py-2 text-sm',
        notice.level === 'error' && 'bg-destructive-subtle text-destructive',
        notice.level === 'warning' && 'bg-warning text-warning-foreground',
        notice.level === 'info' && 'bg-muted text-muted-foreground',
      )}
    >
      <Icon className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
      <span className="min-w-0 break-words">{notice.text}</span>
    </div>
  );
}

const DECISIONS: { decision: ApprovalDecision; variant: 'primary' | 'secondary' | 'outline' }[] = [
  { decision: 'once', variant: 'primary' },
  { decision: 'minutes', variant: 'secondary' },
  { decision: 'session', variant: 'secondary' },
  { decision: 'always', variant: 'outline' },
  { decision: 'deny', variant: 'outline' },
];

/**
 * A question of the project's gateway: a host the agent wants to reach.
 * Answers go to the computer, which applies them; the container never sees
 * them.
 *
 * @param props - The question, whether its reply still runs, and how to answer.
 * @returns The approval card.
 */
export function ApprovalCard({
  approval,
  live,
  onDecide,
}: {
  approval: AgentDataParts['approval'];
  live: boolean;
  onDecide?: (id: number, decision: ApprovalDecision) => Promise<void>;
}) {
  const t = useT();
  const [sending, setSending] = useState<ApprovalDecision | null>(null);
  const host = approval.port === 443 || approval.port === 80 ? approval.host : `${approval.host}:${approval.port}`;
  const open = approval.state === 'pending' && live && onDecide !== undefined;
  const ref = useRef<HTMLDivElement>(null);

  // The agent waits for this answer: an open question must not hide below
  // the fold, even if the user scrolled away from the bottom.
  useEffect(() => {
    if (open) {
      const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      ref.current?.scrollIntoView({ block: 'nearest', behavior: reduced ? 'auto' : 'smooth' });
    }
  }, [open]);

  async function decide(decision: ApprovalDecision) {
    if (!onDecide) {
      return;
    }
    setSending(decision);
    try {
      await onDecide(approval.id, decision);
    } catch (error) {
      toast.error(errorMessage(t, error));
      setSending(null);
    }
  }

  return (
    <Card ref={ref} className="flex scroll-mb-4 flex-col gap-3 p-4">
      <div className="flex items-start gap-3">
        <Globe className="mt-0.5 size-4 shrink-0 text-primary" aria-hidden="true" />
        <div className="flex min-w-0 flex-col gap-1">
          <p className="break-words text-sm font-medium text-foreground">{t('agent.approval.title', { host })}</p>
          <p className="text-sm text-muted-foreground">{t('agent.approval.body')}</p>
        </div>
      </div>
      {open ? (
        <div className="flex flex-wrap gap-2" aria-busy={sending !== null}>
          {DECISIONS.map(({ decision, variant }) => (
            <Button
              key={decision}
              size="sm"
              variant={variant}
              disabled={sending !== null}
              loading={sending === decision}
              onClick={() => void decide(decision)}
            >
              {t(`agent.approval.${decision}`)}
            </Button>
          ))}
        </div>
      ) : (
        <Badge className="w-fit">
          {approval.state === 'resolved' ? <Check aria-hidden="true" /> : null}
          {approval.state === 'resolved' ? t('agent.approval.answered') : t('agent.approval.expired')}
        </Badge>
      )}
    </Card>
  );
}
