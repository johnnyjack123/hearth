import type { ChatLink, ConversationDetail, ConversationSummary, DeviceSummary, LinkConversationInput } from '@harness/shared';
import { useQuery } from '@tanstack/react-query';
import { Check, ChevronDown, Container, Laptop, Unlink } from 'lucide-react';
import { useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Badge, EmptyState, Skeleton } from '@/components/ui/display.tsx';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/menu.tsx';
import { api, type ChatMessage, errorMessage } from '@/lib/api.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryClient } from '@/lib/query-client.ts';
import { queryKeys } from '@/lib/queries.ts';
import { cn } from '@/lib/utils.ts';

/** Online state changes when `harness connect` starts or stops elsewhere. */
const DEVICE_REFRESH_MS = 5_000;

function useLiveDevices(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.devices,
    queryFn: api.devices.list,
    refetchInterval: enabled ? DEVICE_REFRESH_MS : false,
  });
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
export function useLinkOnline(link: ChatLink | null): boolean | undefined {
  return useLinkDevice(link)?.online;
}

/**
 * The device of a linked chat, refreshed while the chat is open.
 *
 * @param link - The chat's link, if any.
 * @returns The device, or undefined while unknown.
 */
export function useLinkDevice(link: ChatLink | null): DeviceSummary | undefined {
  const devices = useLiveDevices(link !== null);
  return link ? devices.data?.find((device) => device.id === link.deviceId) : undefined;
}

/**
 * The composer's environment control: links the chat to a project on one of
 * the user's computers, shows the link and dissolves it.
 *
 * @param props - The conversation and whether its device is online.
 * @returns The control.
 */
export function EnvironmentButton({ conversation, online }: { conversation: ConversationSummary; online: boolean | undefined }) {
  const t = useT();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [unlinking, setUnlinking] = useState(false);
  const { link } = conversation;
  const device = useLinkDevice(link);

  async function unlink() {
    setUnlinking(true);
    try {
      await api.conversations.unlink(conversation.id);
      storeConversation({ ...conversation, link: null });
      setMenuOpen(false);
      toast.success(t('agent.unlinked'));
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setUnlinking(false);
    }
  }

  const dialog = <LinkDialog open={dialogOpen} onOpenChange={setDialogOpen} conversation={conversation} />;
  if (!link) {
    return (
      <>
        <Button
          variant="ghost"
          size="sm"
          className="min-w-0 gap-1.5 px-2 text-muted-foreground hover:text-foreground"
          aria-label={t('agent.connect')}
          onClick={() => setDialogOpen(true)}
        >
          <Container />
          <span className="truncate max-sm:sr-only">{t('agent.environment')}</span>
        </Button>
        {dialog}
      </>
    );
  }

  return (
    <>
      <Popover open={menuOpen} onOpenChange={setMenuOpen}>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="sm" className="min-w-0 max-w-64 gap-1.5 px-2 text-muted-foreground hover:text-foreground">
            <Container className="text-primary" />
            <span className="truncate">{t('agent.linkedTo', { harness: link.harness, project: link.project })}</span>
            <StatusDot online={online} />
            <ChevronDown className="opacity-60" />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" side="top" className="flex w-[min(var(--spacing)*80,calc(100vw-var(--spacing)*8))] flex-col gap-3 p-3">
          <div className="flex items-start gap-3">
            <Laptop className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <span className="break-words text-sm font-medium text-foreground">
                {t('agent.linkedTo', { harness: link.harness, project: link.project })}
              </span>
              <span className="break-words text-xs text-muted-foreground">{t('agent.onDevice', { device: link.deviceName })}</span>
              <OnlineBadge online={online} />
              {online ? <VmLine vm={device?.vm ?? null} /> : null}
              {online === false ? <span className="text-xs text-muted-foreground">{t('agent.offlineHint')}</span> : null}
            </div>
          </div>
          <div className="flex flex-col gap-1 border-t border-border pt-2">
            <Button
              variant="ghost"
              size="sm"
              className="justify-start"
              onClick={() => {
                setMenuOpen(false);
                setDialogOpen(true);
              }}
            >
              <Container />
              {t('agent.change')}
            </Button>
            <Button variant="ghost" size="sm" className="justify-start text-destructive" loading={unlinking} onClick={() => void unlink()}>
              <Unlink />
              {t('agent.unlink')}
            </Button>
          </div>
        </PopoverContent>
      </Popover>
      {dialog}
    </>
  );
}

function StatusDot({ online }: { online: boolean | undefined }) {
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
  return vm ? <span className="text-xs text-muted-foreground">{t(`agent.vm.${vm}`)}</span> : null;
}

function sameChoice(a: LinkConversationInput | null, b: LinkConversationInput | null): boolean {
  return a !== null && b !== null && a.deviceId === b.deviceId && a.project === b.project && a.harness === b.harness;
}

/**
 * Picks a device, project and harness for a chat. Only projects the computer
 * offers right now can be chosen; the computer confirms the first use.
 *
 * @param props - Open state and the conversation.
 * @returns The dialog.
 */
export function LinkDialog({
  open,
  onOpenChange,
  conversation,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  conversation: ConversationSummary;
}) {
  const t = useT();
  const devices = useLiveDevices(open);
  const current = conversation.link
    ? { deviceId: conversation.link.deviceId, project: conversation.link.project, harness: conversation.link.harness }
    : null;
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
      const summary = await api.conversations.link(conversation.id, choice);
      storeConversation(summary);
      toast.success(t('agent.linked', { project: choice.project }));
      onOpenChange(false);
    } catch (error) {
      toast.error(errorMessage(t, error));
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
            {t('agent.link')}
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
                <span className="shrink-0 text-xs text-muted-foreground">{option.harness}</span>
                <Check className={cn('size-4 shrink-0 text-primary', !selected && 'invisible')} aria-hidden="true" />
              </label>
            );
          })}
        </div>
      )}
    </div>
  );
}
