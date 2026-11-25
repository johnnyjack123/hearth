import {
  type AgentDataParts,
  type ChatLink,
  type ConversationDetail,
  type ConversationSummary,
  type DeviceSummary,
  HOST_HARNESS,
  harnessNameSchema,
  type LinkConversationInput,
  projectNameSchema,
} from '@hearth/shared';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { Check, ChevronRight, Container, GitCommitHorizontal, Laptop, MessageSquarePlus, Save, Unlink, X } from 'lucide-react';
import { type ReactNode, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Badge, EmptyState, Skeleton } from '@/components/ui/display.tsx';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import { api, type ChatMessage, errorMessage } from '@/lib/api.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryClient } from '@/lib/query-client.ts';
import { queryKeys, useDevices } from '@/lib/queries.ts';
import { cn } from '@/lib/utils.ts';
import { ProjectAppsDialog, ProjectAppsLine } from '../devices/device-apps.tsx';
import { DeviceVm } from '../devices/device-vm.tsx';
import { upsertMessage } from './chat-store.ts';
import { harnessLabel, linkLabel } from './host-parts.tsx';
import { takeoverTitle } from './takeover-card.tsx';

/** Search parameters of the start page that preselect an environment. */
export interface EnvironmentSearch {
  /** Device id. */
  device?: string;
  /** Project on that device. */
  project?: string;
  /** Harness, e.g. `pi`. */
  harness?: string;
}

/**
 * Keeps only well-formed environment parameters; the server checks the rest
 * when the chat is created.
 *
 * @param search - Raw search parameters.
 * @returns The environment, complete or empty.
 */
export function parseEnvironmentSearch(search: Record<string, unknown>): EnvironmentSearch {
  const { device, project, harness } = search;
  const valid =
    typeof device === 'string' &&
    device.length > 0 &&
    device.length <= 100 &&
    projectNameSchema.safeParse(project).success &&
    harnessNameSchema.safeParse(harness).success;
  return valid ? { device, project: project as string, harness: harness as string } : {};
}

/**
 * The environment the start page's search parameters name, if complete.
 *
 * @param search - Parsed search parameters.
 * @returns The choice, or null.
 */
export function environmentFromSearch(search: EnvironmentSearch): LinkConversationInput | null {
  return search.device && search.project && search.harness
    ? { deviceId: search.device, project: search.project, harness: search.harness }
    : null;
}

function storeConversation(summary: ConversationSummary): void {
  queryClient.setQueryData<ConversationDetail<ChatMessage>>(queryKeys.conversation(summary.id), (current) =>
    current ? { ...current, link: summary.link } : current,
  );
  void queryClient.invalidateQueries({ queryKey: queryKeys.conversations, exact: true });
}

/**
 * Whether a linked chat's device is connected right now.
 *
 * @param link - The chat's link, if any.
 * @returns True, false, or undefined while unknown.
 */
export function useLinkOnline(link: Pick<ChatLink, 'deviceId'> | null): boolean | undefined {
  return useLinkDevice(link)?.online;
}

/**
 * The device of a linked chat, refreshed while the chat is open.
 *
 * @param link - The chat's link, if any.
 * @returns The device, or undefined while unknown.
 */
export function useLinkDevice(link: Pick<ChatLink, 'deviceId'> | null): DeviceSummary | undefined {
  const devices = useDevices(link !== null);
  return link ? devices.data?.find((device) => device.id === link.deviceId) : undefined;
}

/** What of the agent's work the user's folder lacks, as the device last reported it. */
export type WorkspaceState = AgentDataParts['workspace'];

/**
 * The workspace state of a linked chat: the device reports it after every
 * reply and takeover, so the latest report is the current one.
 *
 * @param messages - The chat's messages.
 * @returns The latest report, or null before the first one.
 */
export function latestWorkspace(messages: ChatMessage[]): WorkspaceState | null {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const part = messages[index]?.parts.findLast((candidate) => candidate.type === 'data-workspace');
    if (part) {
      return part.data;
    }
  }
  return null;
}

/** The environment part of the chat options, see `ChatOptions`. */
export interface EnvironmentOption {
  /** The chosen environment; null for an ordinary chat. */
  link: ChatLink | null;
  /** Whether its computer is connected; undefined while unknown. */
  online: boolean | undefined;
  /** What of the agent's work the user's folder lacks, if reported. */
  workspace: WorkspaceState | null;
  /** Opens the dialog that picks an environment. */
  pick: () => void;
  /** The options page of a chosen environment; `close` closes the options. */
  details: (close: () => void) => ReactNode;
  /** The dialogs, rendered outside the options so they outlive them. */
  dialogs: ReactNode;
}

/**
 * The environment of an existing chat: links it to a project on one of the
 * user's computers, shows the link and dissolves it.
 *
 * @param conversation - The chat.
 * @param online - Whether its device is online.
 * @param workspace - What of the agent's work is not taken over yet.
 * @returns The option for the chat options.
 */
export function useChatEnvironment(
  conversation: ConversationSummary,
  online: boolean | undefined,
  workspace: WorkspaceState | null = null,
): EnvironmentOption {
  const t = useT();
  const navigate = useNavigate();
  const [unlinking, setUnlinking] = useState(false);
  const [saving, setSaving] = useState(false);
  const { link } = conversation;

  async function save() {
    setSaving(true);
    try {
      const message = await api.conversations.takeover(conversation.id);
      upsertMessage(conversation.id, message);
      // The card in the chat has the details; the toast reaches a user who
      // closed the menu meanwhile.
      const report = message.parts.findLast((part) => part.type === 'data-takeover')?.data;
      if (report) {
        const title = takeoverTitle(t, report);
        if (report.state === 'merged' || report.state === 'up_to_date') {
          toast.success(title);
        } else {
          toast.warning(title);
        }
      }
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setSaving(false);
    }
  }

  async function setAutosync(autosync: boolean) {
    try {
      storeConversation(await api.conversations.updateLink(conversation.id, { autosync }));
    } catch (error) {
      toast.error(errorMessage(t, error));
    }
  }

  async function choose(choice: LinkConversationInput) {
    try {
      const summary = await api.conversations.link(conversation.id, choice);
      storeConversation(summary);
      toast.success(t('agent.linked', { project: choice.project }));
    } catch (error) {
      toast.error(errorMessage(t, error));
      throw error;
    }
  }

  async function unlink(close: () => void) {
    setUnlinking(true);
    try {
      await api.conversations.unlink(conversation.id);
      storeConversation({ ...conversation, link: null });
      close();
      toast.success(t('agent.unlinked'));
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setUnlinking(false);
    }
  }

  return useEnvironmentOption({
    link,
    conversationId: conversation.id,
    online,
    workspace: link ? workspace : null,
    confirmLabel: t('agent.link'),
    onChoose: choose,
    // In the direct mode the agent works in the folder itself: nothing to save.
    work: link?.harness === HOST_HARNESS ? null : link ? (
      <>
        {workspace ? <WorkspaceLine workspace={workspace} /> : null}
        <Button variant="secondary" size="sm" className="justify-start" loading={saving} disabled={online === false} onClick={() => void save()}>
          <Save />
          {saving ? t('takeover.saving') : t('takeover.save')}
        </Button>
        <span className="text-xs text-muted-foreground">{t('takeover.saveHint')}</span>
        <label className="flex cursor-pointer items-start gap-3 pt-1">
          <Switch checked={link.autosync} onCheckedChange={(checked) => void setAutosync(checked)} />
          <span className="flex min-w-0 flex-col gap-0.5">
            <span className="text-sm text-foreground">{t('takeover.autosync')}</span>
            <span className="text-xs text-muted-foreground">{t('takeover.autosyncHint')}</span>
          </span>
        </label>
      </>
    ) : null,
    actions: (close) =>
      link ? (
        <>
          <Button
            variant="ghost"
            size="sm"
            className="justify-start"
            onClick={() => {
              close();
              void navigate({ to: '/', search: { device: link.deviceId, project: link.project, harness: link.harness } });
            }}
          >
            <MessageSquarePlus />
            {t('agent.newChatHere')}
          </Button>
          <Button variant="ghost" size="sm" className="justify-start text-destructive" loading={unlinking} onClick={() => void unlink(close)}>
            <Unlink />
            {t('agent.unlink')}
          </Button>
        </>
      ) : null,
  });
}

/**
 * The environment of the start page: the chat does not exist yet, so the
 * choice is only kept until the first message creates it.
 *
 * @param value - The chosen environment.
 * @param onChange - Changes it.
 * @returns The option for the chat options.
 */
export function useNewChatEnvironment(
  value: LinkConversationInput | null,
  onChange: (value: LinkConversationInput | null) => void,
): EnvironmentOption {
  const t = useT();
  const device = useLinkDevice(value);
  // Until the device list is there, the id stands in for the name.
  const link: ChatLink | null = value ? { ...value, deviceName: device?.name ?? value.deviceId, autosync: false } : null;
  return useEnvironmentOption({
    link,
    online: device?.online,
    workspace: null,
    confirmLabel: t('agent.choose'),
    onChoose: onChange,
    work: null,
    actions: (close) => (
      <Button
        variant="ghost"
        size="sm"
        className="justify-start"
        onClick={() => {
          close();
          onChange(null);
        }}
      >
        <X />
        {t('agent.withoutEnvironment')}
      </Button>
    ),
  });
}

/**
 * The dialog to pick an environment, and the page of a chosen one: device,
 * the agent's `work`, VM, programs, another environment, and `actions`.
 * The dialogs' state lives here, in the page, so closing the options does
 * not close them.
 */
function useEnvironmentOption({
  link,
  conversationId,
  online,
  workspace,
  confirmLabel,
  onChoose,
  work,
  actions,
}: {
  link: ChatLink | null;
  conversationId?: string;
  online: boolean | undefined;
  workspace: WorkspaceState | null;
  confirmLabel: string;
  onChoose: (choice: LinkConversationInput) => Promise<void> | void;
  work: ReactNode;
  actions: (close: () => void) => ReactNode;
}): EnvironmentOption {
  const t = useT();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [appsOpen, setAppsOpen] = useState(false);
  const device = useLinkDevice(link);

  return {
    link,
    online,
    workspace,
    pick: () => setDialogOpen(true),
    details: (close) =>
      link ? (
        <div className="flex flex-col gap-3">
          <div className="flex items-start gap-3">
            <Laptop className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <span className="break-words text-sm font-medium text-foreground">
                {linkLabel(t, link)}
              </span>
              <span className="break-words text-xs text-muted-foreground">{t('agent.onDevice', { device: link.deviceName })}</span>
              <OnlineBadge online={online} />
              {online === false ? <span className="text-xs text-muted-foreground">{t('agent.offlineHint')}</span> : null}
            </div>
          </div>
          {work ? <div className="flex flex-col gap-1.5 border-t border-border pt-2">{work}</div> : null}
          {link.harness === HOST_HARNESS ? <HostLine device={device} project={link.project} conversationId={conversationId} /> : null}
          {device && link.harness !== HOST_HARNESS ? (
            <div className="flex flex-col gap-2 border-t border-border pt-2">
              <DeviceVm device={device} />
              <ProjectAppsLine
                device={device}
                project={link.project}
                onChange={() => {
                  close();
                  setAppsOpen(true);
                }}
              />
            </div>
          ) : null}
          <div className="flex flex-col gap-1 border-t border-border pt-2">
            <Button
              variant="ghost"
              size="sm"
              className="justify-start"
              onClick={() => {
                close();
                setDialogOpen(true);
              }}
            >
              <Container />
              {t('agent.change')}
            </Button>
            {actions(close)}
          </div>
        </div>
      ) : null,
    dialogs: (
      <>
        <LinkDialog
          open={dialogOpen}
          onOpenChange={setDialogOpen}
          current={link ? { deviceId: link.deviceId, project: link.project, harness: link.harness } : null}
          confirmLabel={confirmLabel}
          onChoose={onChoose}
        />
        {device && link ? <ProjectAppsDialog open={appsOpen} onOpenChange={setAppsOpen} device={device} project={link.project} /> : null}
      </>
    ),
  };
}

/**
 * Green when the computer is connected, grey when not, nothing while unknown.
 *
 * @param props - Whether it is online.
 * @returns The dot.
 */
export function StatusDot({ online }: { online: boolean | undefined }) {
  const t = useT();
  if (online === undefined) {
    return null;
  }
  // The dot only repeats the badge in the menu; the label carries the meaning.
  return (
    <span
      role="img"
      aria-label={online ? t('agent.online') : t('agent.offline')}
      className={cn('size-2 shrink-0 rounded-full', online ? 'bg-success' : 'bg-muted-foreground/50')}
    />
  );
}

function workspaceText(t: ReturnType<typeof useT>, { commits, changes }: WorkspaceState): string {
  if (commits === 0 && changes === 0) {
    return t('takeover.workspaceSaved');
  }
  const what = [
    ...(commits > 0 ? [t('takeover.workspaceCommits', { count: commits })] : []),
    ...(changes > 0 ? [t('takeover.workspaceChanges', { count: changes })] : []),
  ];
  return t('takeover.workspaceUnsaved', { what: what.join(', ') });
}

/**
 * Saved or not, next to the environment; the shape differs, not only the
 * color.
 *
 * @param props - The latest report.
 * @returns The mark.
 */
export function WorkspaceMark({ workspace }: { workspace: WorkspaceState }) {
  const t = useT();
  const saved = workspace.commits === 0 && workspace.changes === 0;
  const text = workspaceText(t, workspace);
  const Icon = saved ? Check : GitCommitHorizontal;
  return (
    <span role="img" aria-label={text} title={text} data-workspace={saved ? 'saved' : 'unsaved'} className="flex shrink-0">
      <Icon className={cn('size-3.5', saved ? 'text-success' : 'text-warning')} aria-hidden="true" />
    </span>
  );
}

function WorkspaceLine({ workspace }: { workspace: WorkspaceState }) {
  const t = useT();
  const saved = workspace.commits === 0 && workspace.changes === 0;
  return (
    <div className="flex flex-col gap-1 pb-1">
      <Badge variant={saved ? 'success' : 'warning'} className="w-fit whitespace-normal">
        {saved ? <Check aria-hidden="true" /> : <GitCommitHorizontal aria-hidden="true" />}
        {workspaceText(t, workspace)}
      </Badge>
      <span className="text-xs text-muted-foreground">{t('takeover.workspaceHint')}</span>
    </div>
  );
}

/**
 * What the direct mode means here: where its questions are answered, what
 * the agent may do (on demand) and what the user allowed for this chat.
 */
function HostLine({ device, project, conversationId }: { device: DeviceSummary | undefined; project: string; conversationId: string | undefined }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const host = device?.projects.find((entry) => entry.name === project)?.host;
  return (
    <div className="flex flex-col gap-1 border-t border-border pt-2 text-xs text-muted-foreground">
      <span>{t('host.linkedHint')}</span>
      {host === undefined ? null : <span>{host.browserApprovals ? t('host.approvalsHere') : t('host.approvalsComputer')}</span>}
      <Button
        variant="link"
        size="sm"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="h-auto gap-1 self-start px-0 py-1"
        data-host-rights-toggle
      >
        <ChevronRight className={cn('size-3.5 transition-transform duration-(--duration-fast) ease-standard', open && 'rotate-90')} aria-hidden="true" />
        {t('host.rights.title')}
      </Button>
      {open ? <HostRights writesAsk={host?.writes === 'ask'} jev={device?.jevReview ?? true} /> : null}
      {conversationId ? <HostGrantList conversationId={conversationId} /> : null}
    </div>
  );
}

/** What the agent of the direct mode may do, in three lists; the computer's rules are the same (`hearth_core::host::rights`). */
function HostRights({ writesAsk, jev }: { writesAsk: boolean; jev: boolean }) {
  const t = useT();
  const groups = [
    {
      key: 'free',
      title: t('host.rights.free'),
      tone: 'text-success',
      items: writesAsk ? [t('host.rights.freeRead')] : [t('host.rights.freeRead'), t('host.rights.freeWrite')],
    },
    {
      key: 'ask',
      title: t('host.rights.ask'),
      tone: 'text-warning',
      items: [
        t('host.rights.askCommand'),
        ...(writesAsk ? [t('host.rights.askWrites')] : []),
        t('host.rights.askOutside'),
        t('host.rights.askProtected'),
      ],
    },
    {
      key: 'never',
      title: t('host.rights.never'),
      tone: 'text-destructive',
      items: [t('host.rights.neverSecrets'), t('host.rights.neverSystem'), t('host.rights.neverHearth'), t('host.rights.neverStartup')],
    },
  ];
  return (
    <div className="flex flex-col gap-2" data-host-rights>
      {groups.map((group) => (
        <div key={group.key} className="flex flex-col gap-0.5">
          <span className={cn('font-medium', group.tone)}>{group.title}</span>
          <ul className="flex list-disc flex-col gap-0.5 ps-4 text-foreground">
            {group.items.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </div>
      ))}
      <span>{jev ? t('host.rights.jev') : t('host.rights.jevOff')}</span>
      <span>{t('host.rights.gap')}</span>
    </div>
  );
}

/** The chat's "for this chat" answers as the computer reports them, each to take back. */
function HostGrantList({ conversationId }: { conversationId: string }) {
  const t = useT();
  const [revoking, setRevoking] = useState<string | null>(null);
  // Answers change on the computer too; a light poll while the page is open.
  const grants = useQuery({
    queryKey: queryKeys.hostGrants(conversationId),
    queryFn: () => api.conversations.hostGrants(conversationId),
    refetchInterval: 5_000,
  });
  const data = grants.data;
  if (!data?.online) {
    return null;
  }
  if (!data.supported) {
    return <span>{t('host.grants.outdated')}</span>;
  }
  if (data.grants.length === 0) {
    return <span data-host-grants="0">{t('host.grants.none')}</span>;
  }

  async function revoke(id: string) {
    setRevoking(id);
    try {
      await api.conversations.hostRevoke(conversationId, id);
      toast.success(t('host.grants.revoked'));
      // The computer answers with its new list a moment later.
      setTimeout(() => void grants.refetch(), 600);
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setRevoking(null);
    }
  }

  return (
    <div className="flex flex-col gap-1" data-host-grants={data.grants.length}>
      <span className="font-medium text-foreground">{t('host.grants.title')}</span>
      <ul className="flex flex-col gap-1">
        {data.grants.map((grant) => (
          <li key={grant.id} className="flex items-start gap-2">
            <span className="min-w-0 flex-1 break-all">
              {t(`host.grants.${grant.kind}`)}: <code className="font-mono text-foreground">{grant.target.split('\n')[0]}</code>
            </span>
            <Button size="sm" variant="ghost" className="h-auto shrink-0 px-1.5 py-0.5" loading={revoking === grant.id} onClick={() => void revoke(grant.id)}>
              {t('host.grants.revoke')}
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function OnlineBadge({ online }: { online: boolean | undefined }) {
  const t = useT();
  if (online === undefined) {
    return null;
  }
  return (
    <Badge variant={online ? 'success' : 'neutral'} className="w-fit">
      {online ? t('agent.online') : t('agent.offline')}
    </Badge>
  );
}

function VmLine({ vm }: { vm: DeviceSummary['vm'] }) {
  const t = useT();
  return vm ? <span className="text-xs text-muted-foreground">{t(`devices.vm.${vm}`)}</span> : null;
}

function sameChoice(a: LinkConversationInput | null, b: LinkConversationInput | null): boolean {
  return a !== null && b !== null && a.deviceId === b.deviceId && a.project === b.project && a.harness === b.harness;
}

/**
 * Picks a device, project and harness for a chat. Only projects the computer
 * offers can be chosen; the computer decides when the first message comes.
 *
 * @param props - Open state, the current choice, the confirm label and what
 *   to do with a new choice (rejecting keeps the dialog open).
 * @returns The dialog.
 */
export function LinkDialog({
  open,
  onOpenChange,
  current,
  confirmLabel,
  onChoose,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  current: LinkConversationInput | null;
  confirmLabel: string;
  onChoose: (choice: LinkConversationInput) => Promise<void> | void;
}) {
  const t = useT();
  const devices = useDevices(open);
  const [choice, setChoice] = useState<LinkConversationInput | null>(current);
  const [saving, setSaving] = useState(false);
  // Each opening starts from the current link, not from the last choice.
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) {
      setChoice(current);
    }
  }

  async function save() {
    if (!choice) {
      return;
    }
    setSaving(true);
    try {
      await onChoose(choice);
      onOpenChange(false);
    } catch {
      // onChoose has told the user why.
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(next) => !saving && onOpenChange(next)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('agent.dialogTitle')}</DialogTitle>
          <DialogDescription>{t('agent.dialogDescription')}</DialogDescription>
        </DialogHeader>
        {devices.isPending ? (
          <div className="flex flex-col gap-2" aria-busy="true">
            <Skeleton className="h-12 w-full" />
            <Skeleton className="h-12 w-full" />
          </div>
        ) : !devices.data?.length ? (
          <EmptyState icon={<Laptop />} title={t('agent.noDevices')} description={t('agent.noDevicesHint')} className="py-6" />
        ) : (
          <div role="radiogroup" aria-label={t('agent.dialogTitle')} className="flex max-h-[50vh] flex-col gap-4 overflow-y-auto">
            {devices.data.map((device) => (
              <DeviceChoices key={device.id} device={device} choice={choice} onChoose={setChoice} />
            ))}
          </div>
        )}
        <p className="text-xs text-muted-foreground">{t('agent.confirmHint')}</p>
        <DialogFooter>
          <Button variant="ghost" disabled={saving} onClick={() => onOpenChange(false)}>
            {t('common.cancel')}
          </Button>
          <Button variant="primary" disabled={!choice || sameChoice(choice, current)} loading={saving} onClick={() => void save()}>
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function DeviceChoices({
  device,
  choice,
  onChoose,
}: {
  device: DeviceSummary;
  choice: LinkConversationInput | null;
  onChoose: (choice: LinkConversationInput) => void;
}) {
  const t = useT();
  const options = device.projects.flatMap((project) =>
    project.harnesses.map((harness) => ({ deviceId: device.id, project: project.name, harness })),
  );
  return (
    <div className="flex flex-col gap-2">
      <div className="flex min-w-0 items-center gap-2">
        <Laptop className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate text-sm font-medium text-foreground">{device.name}</span>
        <OnlineBadge online={device.online} />
      </div>
      <div className="flex flex-col gap-0.5 ps-6">
        {device.online ? <VmLine vm={device.vm} /> : <span className="text-xs text-muted-foreground">{t('agent.deviceOffline')}</span>}
      </div>
      {options.length === 0 ? (
        <p className="ps-6 text-sm text-muted-foreground">{t('agent.noProjects')}</p>
      ) : (
        <div className="flex flex-col gap-1 ps-6">
          {options.map((option) => {
            const selected = sameChoice(option, choice);
            return (
              <label
                key={`${option.project}/${option.harness}`}
                className={cn(
                  'flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors duration-(--duration-instant) ease-standard hover:bg-accent',
                  'has-[:focus-visible]:ring-(length:--focus-ring-width) has-[:focus-visible]:ring-ring',
                  selected && 'bg-accent text-accent-foreground',
                )}
              >
                <input
                  type="radio"
                  name="environment"
                  className="sr-only"
                  checked={selected}
                  onChange={() => onChoose(option)}
                />
                <Container className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                <span className="min-w-0 flex-1 truncate font-medium">{option.project}</span>
                <span className="shrink-0 text-xs text-muted-foreground">{harnessLabel(t, option.harness)}</span>
                <Check className={cn('size-4 shrink-0 text-primary', !selected && 'invisible')} aria-hidden="true" />
              </label>
            );
          })}
        </div>
      )}
    </div>
  );
}
