import {
  type CommandAssessment,
  HIGH_COMMAND_DANGER,
  type HostApproval,
  type HostDecision,
  HOST_HARNESS,
  HOST_TOOLS,
  type HostTool,
} from '@hearth/shared';
import type { DynamicToolUIPart } from 'ai';
import { Check, Gauge, Laptop, ShieldAlert, X } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Badge, Card } from '@/components/ui/display.tsx';
import { errorMessage } from '@/lib/api.ts';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

const HOST_PART_TYPES = new Set<string>(HOST_TOOLS.map((name) => `tool-${name}`));

/** A static tool part of the direct mode as stored in a reply. */
interface HostToolPart {
  type: string;
  toolCallId: string;
  state: DynamicToolUIPart['state'];
  input?: unknown;
  output?: unknown;
  errorText?: string;
}

/**
 * Whether a part is a tool call of the direct mode (`tool-read`,
 * `tool-shell` …).
 *
 * @param part - A part of a reply.
 * @returns True for the direct mode's tools.
 */
export function isHostToolPart(part: { type: string }): part is HostToolPart {
  return HOST_PART_TYPES.has(part.type);
}

/**
 * The part in the shape the agent's tool card reads, so the direct mode
 * looks like the sandbox: a command as `$ …`, paths, output on demand.
 *
 * @param part - A tool part of the direct mode.
 * @returns The same call as a dynamic tool part.
 */
export function asDynamicTool(part: HostToolPart): DynamicToolUIPart {
  return { ...part, type: 'dynamic-tool', toolName: part.type.slice('tool-'.length) as HostTool } as DynamicToolUIPart;
}

/**
 * A link for people: `pi in demo`, or the project in the direct mode.
 *
 * @param t - Translate function.
 * @param link - Project and harness.
 * @returns The label.
 */
export function linkLabel(t: ReturnType<typeof useT>, link: { project: string; harness: string }): string {
  return link.harness === HOST_HARNESS ? t('host.linkedTo', { project: link.project }) : t('agent.linkedTo', link);
}

/**
 * The message box's placeholder of a linked chat.
 *
 * @param t - Translate function.
 * @param link - Project and harness.
 * @returns The placeholder.
 */
export function linkPlaceholder(t: ReturnType<typeof useT>, link: { project: string; harness: string }): string {
  return link.harness === HOST_HARNESS ? t('host.placeholder', { project: link.project }) : t('agent.placeholder', link);
}

/**
 * The harness for people: the direct mode has no program of its own.
 *
 * @param t - Translate function.
 * @param harness - The link's harness.
 * @returns Its name in the UI.
 */
export function harnessLabel(t: ReturnType<typeof useT>, harness: string): string {
  return harness === HOST_HARNESS ? t('host.harness') : harness;
}

/** How much harm Jev expects of a command, in four levels. */
export type CommandRiskLevel = 'high' | 'medium' | 'low' | 'minimal';

/**
 * The level of Jev's rating of a command.
 *
 * @param danger - Potential for harm from 0 to 1.
 * @returns The level.
 */
export function commandRiskLevel(danger: number): CommandRiskLevel {
  return danger >= HIGH_COMMAND_DANGER ? 'high' : danger >= 0.5 ? 'medium' : danger >= 0.25 ? 'low' : 'minimal';
}

/**
 * Whether a stored value is Jev's rating of a command.
 *
 * @param value - E.g. `commandAssessment` of a tool's output.
 * @returns True for a rating.
 */
export function isCommandAssessment(value: unknown): value is CommandAssessment {
  const candidate = value as Partial<CommandAssessment> | null;
  return typeof candidate?.danger === 'number' && typeof candidate.model === 'string';
}

/**
 * Jev's rating of a command: level and percent, what it means on hover.
 * Advice only; it never approves anything.
 *
 * @param props - The rating; `compact` for the row of a tool card.
 * @returns The badge.
 */
export function CommandRisk({ assessment, compact = false }: { assessment: CommandAssessment; compact?: boolean }) {
  const t = useT();
  const level = commandRiskLevel(assessment.danger);
  const percent = Math.round(assessment.danger * 100);
  const label = t('host.jev.label', { level: t(`host.jev.levels.${level}`), percent });
  return (
    <Badge
      variant={level === 'high' ? 'destructive' : level === 'medium' ? 'warning' : 'neutral'}
      className={cn('w-fit', compact ? 'shrink-0' : 'whitespace-normal')}
      title={t('host.jev.hint', { model: assessment.model })}
      aria-label={label}
      data-command-risk={level}
    >
      <Gauge aria-hidden="true" />
      {compact ? t('host.jev.short', { percent }) : label}
    </Badge>
  );
}

/**
 * A question of the direct mode: a command, a file outside the project, a
 * secret or a protected file. The chat can answer only where the project
 * allows it on the computer; otherwise the card says where to answer.
 *
 * @param props - The question, whether its reply still runs, and how to answer.
 * @returns The card.
 */
export function HostApprovalCard({
  approval,
  live,
  onDecide,
}: {
  approval: HostApproval;
  live: boolean;
  onDecide?: ((approval: string, decision: HostDecision) => Promise<void>) | undefined;
}) {
  const t = useT();
  const [sending, setSending] = useState<HostDecision | null>(null);
  const pending = approval.state === 'pending' && live;
  const ref = useRef<HTMLDivElement>(null);

  // The agent waits for this answer; it must not hide below the fold.
  useEffect(() => {
    if (pending) {
      const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      ref.current?.scrollIntoView({ block: 'nearest', behavior: reduced ? 'auto' : 'smooth' });
    }
  }, [pending]);

  async function decide(decision: HostDecision) {
    if (!onDecide) {
      return;
    }
    setSending(decision);
    try {
      await onDecide(approval.approval, decision);
    } catch (error) {
      toast.error(errorMessage(t, error));
      setSending(null);
    }
  }

  const request = approval.request;
  const detail = request.kind === 'command' ? request.command : request.path;
  const allowed = approval.decision === 'once' || approval.decision === 'chat';
  return (
    <Card ref={ref} className="flex scroll-mb-4 flex-col gap-3 p-4" data-host-approval={approval.state}>
      <div className="flex items-start gap-3">
        <ShieldAlert className="mt-0.5 size-4 shrink-0 text-warning" aria-hidden="true" />
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <p className="break-words text-sm font-medium text-foreground">{t(`host.approval.title.${request.kind}`)}</p>
          <code className="max-h-48 overflow-y-auto whitespace-pre-wrap break-all rounded-md bg-muted px-2 py-1.5 font-mono text-xs text-foreground">
            {detail}
          </code>
          {request.kind === 'command' ? (
            <span className="break-words text-xs text-muted-foreground">{t('host.approval.where', { shell: request.shell, cwd: request.cwd })}</span>
          ) : null}
          <p className="text-sm text-muted-foreground">{approval.why}</p>
          {approval.assessment ? (
            <div className="flex flex-col gap-1">
              <CommandRisk assessment={approval.assessment} />
              {pending && commandRiskLevel(approval.assessment.danger) === 'high' ? (
                <span className="text-xs text-muted-foreground">{t('host.jev.highHint')}</span>
              ) : null}
            </div>
          ) : null}
          <span className="font-mono text-xs text-muted-foreground">{t('host.approval.id', { id: approval.approval })}</span>
        </div>
      </div>
      {pending && approval.browser && onDecide ? (
        <div className="flex flex-wrap gap-2" aria-busy={sending !== null}>
          <Button size="sm" variant="primary" disabled={sending !== null} loading={sending === 'once'} onClick={() => void decide('once')}>
            {t('host.approval.once')}
          </Button>
          {approval.chatWide ? (
            <Button size="sm" variant="secondary" disabled={sending !== null} loading={sending === 'chat'} onClick={() => void decide('chat')}>
              {t(request.kind === 'command' ? 'host.approval.chatCommand' : 'host.approval.chatFolder')}
            </Button>
          ) : null}
          <Button size="sm" variant="ghost" disabled={sending !== null} loading={sending === 'deny'} onClick={() => void decide('deny')}>
            {t('host.approval.deny')}
          </Button>
        </div>
      ) : pending ? (
        <Badge variant="warning" className="w-fit whitespace-normal">
          <Laptop aria-hidden="true" />
          {t('host.approval.onComputer')}
        </Badge>
      ) : (
        <Badge className="w-fit whitespace-normal" variant={allowed ? 'success' : 'neutral'}>
          {allowed ? <Check aria-hidden="true" /> : <X aria-hidden="true" />}
          {approval.decision
            ? t(`host.approval.answers.${approval.decision}`, { by: t(`host.approval.by.${approval.by ?? 'computer'}`) })
            : t('host.approval.expired')}
        </Badge>
      )}
    </Card>
  );
}
