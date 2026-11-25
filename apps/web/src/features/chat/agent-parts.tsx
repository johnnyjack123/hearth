import type { AgentDataParts, AgentToolOutput, ApprovalDecision, Inspection, WebFetchDecision } from '@hearth/shared';
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
import type { MessageKey } from '@/i18n/index.ts';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';
import { CommandRisk, isCommandAssessment } from './host-parts.tsx';
import { InjectionWarning, isAssessment, isFinding } from './injection-findings.tsx';
import { ToolDuration } from './tool-duration.tsx';

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
  if ((part.toolName === 'bash' || part.toolName === 'shell') && command) {
    return { icon: SquareTerminal, summary: `$ ${command.split('\n')[0]}` };
  }
  const pattern = stringField(part.input, 'pattern');
  if (pattern && (part.toolName === 'find' || part.toolName === 'grep')) {
    return { icon: FileText, summary: `${part.toolName} ${pattern}${path ? ` in ${path}` : ''}` };
  }
  if (part.toolName === 'list') {
    return { icon: FileText, summary: `list ${path ?? '.'}` };
  }
  if (path && (part.toolName === 'edit' || part.toolName === 'write')) {
    return { icon: FilePen, summary: `${part.toolName} ${path}` };
  }
  if (path) {
    return { icon: FileText, summary: `${part.toolName} ${path}` };
  }
  return { icon: Wrench, summary: part.toolName };
}

/**
 * The exit code of a bash command that ran but ended with one. pi reports
 * that as a failed tool call ("Command exited with code 2"), yet `grep`
 * without a match or `diff` with differences say so on purpose; it is the
 * agent's information, not a failure of the tool.
 *
 * @param toolName - The tool.
 * @param errorText - What the tool reported.
 * @returns The code, or null for anything else.
 */
export function bashExitCode(toolName: string, errorText: string): number | null {
  if (toolName !== 'bash') {
    return null;
  }
  const match = /Command exited with code (\d+)\s*$/.exec(errorText);
  return match ? Number(match[1]) : null;
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
 * @param props - The tool part, whether its message is still streaming and
 *   how long the call ran.
 * @returns The tool block.
 */
export function ToolCall({
  part,
  streaming,
  ms,
  onAssess,
  onInspect,
}: {
  part: DynamicToolUIPart;
  streaming: boolean;
  ms?: number | undefined;
  onAssess?: (() => Promise<void>) | undefined;
  onInspect?: (() => Promise<Inspection>) | undefined;
}) {
  const t = useT();
  // The direct mode reports its outcome in the output (the model reads
  // the text either way); pi reports a failed call.
  const hostOutcome = part.state === 'output-available' ? (part.output as { isError?: unknown; exitCode?: unknown } | null) : null;
  const hostExit = typeof hostOutcome?.exitCode === 'number' && hostOutcome.exitCode !== 0 ? hostOutcome.exitCode : null;
  const exitCode = part.state === 'output-error' ? bashExitCode(part.toolName, part.errorText) : hostExit;
  const failed = (part.state === 'output-error' && exitCode === null) || (hostOutcome?.isError === true && hostExit === null && hostOutcome.exitCode === undefined);
  const running =
    streaming &&
    (part.state === 'input-available' ||
      part.state === 'input-streaming' ||
      (part.state === 'output-available' && part.preliminary === true));
  const [open, setOpen] = useState<boolean | null>(null);
  const expanded = open ?? failed;
  const { icon: Icon, summary } = describeTool(part);
  const output = outputText(part);
  // The server's own check of the output, with Jev's opinion if asked.
  const stored =
    part.state === 'output-available' ? (part.output as { findings?: unknown; assessment?: unknown; commandAssessment?: unknown } | null) : null;
  // Jev's rating of a command of the direct mode, taken before it ran.
  const risk = isCommandAssessment(stored?.commandAssessment) ? stored.commandAssessment : undefined;
  const findings = Array.isArray(stored?.findings) ? stored.findings.filter(isFinding) : [];
  const warnings = [...new Set(findings.map((finding) => finding.kind))];
  const assessment = isAssessment(stored?.assessment) ? stored.assessment : undefined;
  const command = stringField(part.input, 'command');
  // bash shows its command in full; other tools show their arguments.
  const input =
    part.toolName === 'bash' || part.toolName === 'shell' ? (command?.includes('\n') ? command : null) : JSON.stringify(part.input, null, 2);

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
        {risk ? <CommandRisk assessment={risk} compact /> : null}
        <ToolDuration ms={ms} running={running} />
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
        ) : exitCode !== null ? (
          <Badge className="shrink-0 font-mono" title={t('agent.tool.exitCodeHint', { code: exitCode })}>
            {t('agent.tool.exitCode', { code: exitCode })}
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
      {warnings.length > 0 ? <InjectionWarning warnings={warnings} findings={findings} assessment={assessment} onAssess={onAssess} onInspect={onInspect} /> : null}
    </div>
  );
}

/**
 * A labelled block of tool input or output, scrolling when long.
 *
 * @param props - Label and text.
 * @returns The block.
 */
export function ToolText({ label, text }: { label: string; text: string }) {
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
 * A message of hearth or the harness in an agent reply. The first line is
 * the message; further lines are details shown on demand, such as where the
 * pi guard found a possible prompt injection and why it matters.
 *
 * @param props - The notice.
 * @returns The notice row.
 */
export function AgentNotice({ notice }: { notice: AgentDataParts['notice'] }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const Icon = notice.level === 'error' ? CircleAlert : notice.level === 'warning' ? TriangleAlert : Info;
  // A notice of the server with a code reads in the user's language.
  const text = notice.code ? t(`errors.${notice.code}` as MessageKey, { detail: '' }) : notice.text;
  const lineBreak = text.indexOf('\n');
  const summary = lineBreak >= 0 ? text.slice(0, lineBreak) : text;
  const details = lineBreak >= 0 ? text.slice(lineBreak + 1).trim() : '';
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
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="break-words">{summary}</span>
        {details ? (
          <>
            <Button
              variant="link"
              size="sm"
              aria-expanded={open}
              onClick={() => setOpen(!open)}
              className="h-auto gap-1 self-start px-0 py-1 text-current underline"
            >
              <ChevronRight
                className={cn('size-3.5 transition-transform duration-(--duration-fast) ease-standard', open && 'rotate-90')}
                aria-hidden="true"
              />
              {open ? t('agent.hideDetails') : t('agent.showDetails')}
            </Button>
            <AnimatePresence initial={false}>
              {open ? (
                <motion.p
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1, transition: transitions.enter }}
                  exit={{ opacity: 0, transition: transitions.exit }}
                  className="wrap-anywhere whitespace-pre-wrap rounded-md bg-card px-2 py-1.5 font-mono text-xs text-card-foreground"
                >
                  {details}
                </motion.p>
              ) : null}
            </AnimatePresence>
          </>
        ) : null}
      </div>
    </div>
  );
}

type DecisionButton = { decision: ApprovalDecision; variant: 'primary' | 'secondary' | 'outline' };

const DECISIONS: DecisionButton[] = [
  { decision: 'once', variant: 'primary' },
  { decision: 'minutes', variant: 'secondary' },
  { decision: 'session', variant: 'secondary' },
  // No "always": the computer would take it as "until stopped" anyway. Only
  // the computer itself writes a project's allow list (`hearth net allow`).
  { decision: 'deny', variant: 'outline' },
];

const FETCH_DECISIONS: (DecisionButton & { decision: WebFetchDecision })[] = [
  { decision: 'once', variant: 'primary' },
  { decision: 'session', variant: 'secondary' },
  { decision: 'deny', variant: 'outline' },
];

/**
 * A question of the project's gateway: a host the agent wants to reach.
 * Answers go to the computer, which applies them; the container never sees
 * them. With a `url`, it is a web fetch of the server in an ordinary chat:
 * the card then shows the full URL, because that is what leaves the server.
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
  const fetchUrl = approval.url;
  const buttons = fetchUrl
    ? FETCH_DECISIONS.map((button) => ({ ...button, label: t(`webFetch.approval.${button.decision}`) }))
    : DECISIONS.map((button) => ({ ...button, label: t(`agent.approval.${button.decision}`) }));
  const allowed = approval.answer === 'once' || approval.answer === 'session';
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
          <p className="break-words text-sm font-medium text-foreground">
            {fetchUrl ? t('webFetch.approval.title', { host: approval.host }) : t('agent.approval.title', { host })}
          </p>
          {fetchUrl ? (
            <code className="max-h-32 overflow-y-auto break-all rounded-md bg-muted px-2 py-1.5 font-mono text-xs text-foreground">
              {fetchUrl}
            </code>
          ) : null}
          <p className="text-sm text-muted-foreground">
            {fetchUrl ? (approval.research ? t('webFetch.approval.researchBody') : t('webFetch.approval.body')) : t('agent.approval.body', { host })}
          </p>
          {fetchUrl && approval.research?.similar ? (
            <>
              <span className="text-xs font-medium text-muted-foreground">{t('webFetch.approval.similar')}</span>
              <code className="max-h-32 overflow-y-auto break-all rounded-md bg-muted px-2 py-1.5 font-mono text-xs text-muted-foreground">
                {approval.research.similar}
              </code>
            </>
          ) : null}
        </div>
      </div>
      {open ? (
        <div className="flex flex-wrap gap-2" aria-busy={sending !== null}>
          {buttons.map(({ decision, variant, label }) => (
            <Button
              key={decision}
              size="sm"
              variant={variant}
              disabled={sending !== null}
              loading={sending === decision}
              onClick={() => void decide(decision)}
            >
              {label}
            </Button>
          ))}
        </div>
      ) : (
        <Badge className="w-fit">
          {approval.state === 'resolved' && (allowed || !approval.answer) ? <Check aria-hidden="true" /> : null}
          {approval.answer
            ? t(`webFetch.approval.answers.${approval.answer}`)
            : approval.state === 'resolved'
              ? t('agent.approval.answered')
              : t('agent.approval.expired')}
        </Badge>
      )}
    </Card>
  );
}
