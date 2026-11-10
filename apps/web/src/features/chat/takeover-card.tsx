import type { ReportedFinding, TakeoverReport } from '@hearth/shared';
import { Check, ChevronRight, CircleAlert, Copy, GitMerge, ShieldAlert, TriangleAlert } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Badge } from '@/components/ui/display.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import type { TranslateFn } from '@/i18n/index.ts';
import { transitions } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

const SEVERITY_BADGE = { high: 'destructive', medium: 'warning', low: 'neutral' } as const;

/**
 * The headline of a takeover report.
 *
 * @param t - Translation.
 * @param report - The report.
 * @returns E.g. "In main übernommen".
 */
export function takeoverTitle(t: TranslateFn, report: TakeoverReport): string {
  const branch = report.branch ?? t('takeover.yourBranch');
  switch (report.state) {
    case 'running':
      return t('takeover.running');
    case 'merged':
      return t('takeover.merged', { branch });
    case 'up_to_date':
      return t('takeover.upToDate');
    case 'held':
      return t('takeover.held');
    case 'conflict':
      return t('takeover.conflict');
    case 'refused':
      return t('takeover.refused');
    case 'failed':
      return t('takeover.failed');
  }
}

function StateIcon({ state }: { state: TakeoverReport['state'] }) {
  const className = 'mt-0.5 size-4 shrink-0';
  switch (state) {
    case 'running':
      return <Spinner className={className} />;
    case 'merged':
      return <GitMerge className={cn(className, 'text-success')} aria-hidden="true" />;
    case 'up_to_date':
      return <Check className={cn(className, 'text-muted-foreground')} aria-hidden="true" />;
    case 'held':
      return <ShieldAlert className={cn(className, 'text-destructive')} aria-hidden="true" />;
    case 'conflict':
    case 'refused':
      return <TriangleAlert className={cn(className, 'text-foreground')} aria-hidden="true" />;
    case 'failed':
      return <CircleAlert className={cn(className, 'text-destructive')} aria-hidden="true" />;
  }
}

/**
 * Taking over an agent's work into the user's folder, as a card in the
 * chat: how it ended, what the check found and, when severe findings hold
 * the merge, the command that approves it on the computer. Texts from the
 * device (findings, reasons) are German, like its other messages.
 *
 * @param props - The report.
 * @returns The card.
 */
export function TakeoverCard({ report }: { report: TakeoverReport }) {
  const t = useT();
  const held = report.state === 'held';
  const [open, setOpen] = useState(held);
  const findings = report.findings;
  const details: string[] = [];
  if (report.state === 'merged' && report.commits !== undefined) {
    details.push(t('takeover.mergedDetail', { commits: report.commits, files: report.files ?? 0 }));
  }
  if (report.committed) {
    details.push(t('takeover.committed', { count: report.committed }));
  }
  if (report.checks.length > 0) {
    details.push(
      `${t('takeover.checks', { checks: report.checks.join(', ') })} · ${
        findings.length === 0 ? t('takeover.noFindings') : t('takeover.findings', { count: findings.length })
      }`,
    );
  }
  return (
    <section
      aria-label={takeoverTitle(t, report)}
      className="flex min-w-0 flex-col gap-2 rounded-xl border border-border bg-card p-3 text-sm text-card-foreground"
    >
      <div className="flex min-w-0 items-start gap-2">
        <StateIcon state={report.state} />
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="flex flex-wrap items-center gap-2">
            <span className="break-words font-medium text-foreground">{takeoverTitle(t, report)}</span>
            {report.auto ? <Badge variant="neutral">{t('takeover.auto')}</Badge> : null}
          </span>
          {report.state === 'running' && report.step ? <span className="text-xs text-muted-foreground">{report.step}</span> : null}
          {details.map((line) => (
            <span key={line} className="break-words text-xs text-muted-foreground">
              {line}
            </span>
          ))}
        </div>
      </div>
      {report.reason ? <p className="break-words ps-6 text-sm text-muted-foreground">{report.reason}</p> : null}
      {report.state === 'conflict' ? <Conflicts files={report.conflicts ?? []} /> : null}
      {held && report.request ? <ApproveCommand id={report.request} /> : null}
      {report.notes?.length ? (
        <ul className="flex flex-col gap-1 ps-6 text-xs text-muted-foreground">
          {report.notes.map((note) => (
            <li key={note} className="break-words">
              {note}
            </li>
          ))}
        </ul>
      ) : null}
      {findings.length > 0 ? (
        <div className="flex flex-col gap-1 ps-6">
          <Button
            variant="link"
            size="sm"
            aria-expanded={open}
            onClick={() => setOpen(!open)}
            className="h-auto gap-1 self-start px-0 py-1"
          >
            <ChevronRight
              className={cn('size-3.5 transition-transform duration-(--duration-fast) ease-standard', open && 'rotate-90')}
              aria-hidden="true"
            />
            {open ? t('takeover.hideFindings') : t('takeover.showFindings')}
          </Button>
          <AnimatePresence initial={false}>
            {open ? (
              <motion.ul
                initial={{ opacity: 0 }}
                animate={{ opacity: 1, transition: transitions.enter }}
                exit={{ opacity: 0, transition: transitions.exit }}
                className="flex flex-col gap-2"
              >
                {findings.map((finding, index) => (
                  <FindingRow key={`${finding.rule}-${finding.path}-${finding.line ?? ''}-${index}`} finding={finding} />
                ))}
              </motion.ul>
            ) : null}
          </AnimatePresence>
        </div>
      ) : null}
    </section>
  );
}

function FindingRow({ finding }: { finding: ReportedFinding }) {
  const t = useT();
  const place = finding.path ? `${finding.path}${finding.line ? `:${finding.line}` : ''}` : '';
  return (
    <li className="flex min-w-0 flex-col gap-1">
      <span className="flex min-w-0 flex-wrap items-center gap-2">
        <Badge variant={SEVERITY_BADGE[finding.severity]}>{t(`takeover.severity.${finding.severity}`)}</Badge>
        {place ? <code className="wrap-anywhere font-mono text-xs text-muted-foreground">{place}</code> : null}
      </span>
      <span className="break-words">{finding.message}</span>
      {finding.excerpt ? (
        <code className="wrap-anywhere whitespace-pre-wrap rounded-md bg-muted px-2 py-1 font-mono text-xs text-muted-foreground">
          {finding.excerpt}
        </code>
      ) : null}
    </li>
  );
}

function Conflicts({ files }: { files: string[] }) {
  const t = useT();
  return (
    <div className="flex flex-col gap-1 ps-6 text-xs text-muted-foreground">
      <span>{t('takeover.conflictBody')}</span>
      <ul className="flex flex-col gap-0.5">
        {files.map((file) => (
          <li key={file}>
            <code className="wrap-anywhere font-mono">{file}</code>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** The command that approves a held merge, ready to copy. */
function ApproveCommand({ id }: { id: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const command = `hearth merge --approve ${id}`;
  return (
    <div className="flex flex-col gap-1.5 ps-6">
      <span className="text-xs text-muted-foreground">{t('takeover.heldBody')}</span>
      <div className="flex min-w-0 items-center gap-2 rounded-md bg-muted px-2 py-1">
        <code className="min-w-0 flex-1 wrap-anywhere font-mono text-xs text-foreground">{command}</code>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={copied ? t('takeover.copied') : t('takeover.copy')}
          onClick={() => {
            void navigator.clipboard.writeText(command).then(() => setCopied(true));
          }}
        >
          {copied ? <Check /> : <Copy />}
        </Button>
      </div>
    </div>
  );
}
