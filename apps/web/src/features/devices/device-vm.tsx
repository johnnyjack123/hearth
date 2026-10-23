import type { DeviceSummary } from '@harness/shared';
import { useQueryClient } from '@tanstack/react-query';
import { ChevronDown, Power, Timer } from 'lucide-react';
import { useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { ConfirmDialog } from '@/components/ui/dialog.tsx';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/menu.tsx';
import type { TranslateFn } from '@/i18n/index.ts';
import { api, errorMessage } from '@/lib/api.ts';
import { usePreferences } from '@/lib/preferences.tsx';
import { queryKeys } from '@/lib/queries.ts';

/** Idle times offered in the menu, in minutes; 0 means never. */
const IDLE_CHOICES = [15, 30, 60, 120, 0];

function duration(t: TranslateFn, minutes: number): string {
  return minutes % 60 === 0 ? t('devices.vm.hours', { count: minutes / 60 }) : t('devices.vm.minutes', { count: minutes });
}

/**
 * What keeps the VM busy, as one phrase, or null if nothing does.
 *
 * @param t - Translation.
 * @param activity - The device's report.
 * @returns E.g. "ein Agent antwortet · harness im Terminal".
 */
function busyWith(t: TranslateFn, activity: NonNullable<DeviceSummary['activity']>): string | null {
  const parts: string[] = [];
  if (activity.replies > 0) {
    parts.push(activity.replies === 1 ? t('devices.vm.reply') : t('devices.vm.replies', { count: activity.replies }));
  }
  if (activity.terminals > 0) {
    parts.push(activity.terminals === 1 ? t('devices.vm.terminal') : t('devices.vm.terminals', { count: activity.terminals }));
  }
  return parts.length > 0 ? parts.join(' · ') : null;
}

/**
 * State of a device's VM with what keeps it busy, a button to stop it and
 * the idle time until it stops by itself. The device only takes these
 * harmless commands from the browser.
 *
 * @param props - The device.
 * @returns The VM section.
 */
export function DeviceVm({ device }: { device: DeviceSummary }) {
  const { t, locale } = usePreferences();
  const queryClient = useQueryClient();
  const [stopping, setStopping] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const { activity, vm } = device;
  // Online without a report: harness on the computer predates protocol 2.
  const outdated = device.online && vm !== null && activity === null;
  const busy = activity ? busyWith(t, activity) : null;

  async function stop() {
    setStopping(true);
    try {
      const result = await api.devices.stopVm(device.id);
      if (result.ok) {
        toast.success(t('devices.vm.stopDone'));
      } else {
        toast.error(t('devices.vm.stopFailed', { detail: result.message }));
      }
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setStopping(false);
      void queryClient.invalidateQueries({ queryKey: queryKeys.devices });
    }
  }

  async function setIdle(idleMinutes: number) {
    const previous = queryClient.getQueryData<DeviceSummary[]>(queryKeys.devices);
    queryClient.setQueryData<DeviceSummary[]>(queryKeys.devices, (devices) =>
      devices?.map((candidate) => (candidate.id === device.id ? { ...candidate, idleMinutes } : candidate)),
    );
    try {
      const updated = await api.devices.update(device.id, { idleMinutes });
      queryClient.setQueryData<DeviceSummary[]>(queryKeys.devices, (devices) =>
        devices?.map((candidate) => (candidate.id === device.id ? updated : candidate)),
      );
    } catch (error) {
      queryClient.setQueryData(queryKeys.devices, previous);
      toast.error(errorMessage(t, error));
    }
  }

  let detail: string | null = null;
  if (vm === 'running' && activity) {
    if (busy) {
      detail = t('devices.vm.busy', { what: busy });
    } else if (activity.stopsAt !== null) {
      const time = new Date(activity.stopsAt).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' });
      detail = t('devices.vm.stopsAt', { time });
    } else if (device.idleMinutes === 0) {
      detail = t('devices.vm.neverStops');
    }
  }
  const canStop = activity !== null && (vm === 'running' || vm === 'starting' || vm === 'stopping');
  // An idle time not offered in the menu (set through the API) stays visible.
  const choices = IDLE_CHOICES.includes(device.idleMinutes) ? IDLE_CHOICES : [...IDLE_CHOICES, device.idleMinutes];

  return (
    <div className="flex min-w-0 flex-col gap-2">
      {device.online && vm ? (
        <div className="flex min-w-0 flex-wrap items-start gap-x-3 gap-y-2">
          <div className="flex min-w-40 flex-1 flex-col gap-0.5">
            <span className="text-sm text-foreground">{t(`devices.vm.${vm}`)}</span>
            {detail ? <span className="break-words text-xs text-muted-foreground">{detail}</span> : null}
          </div>
          {canStop ? (
            <Button
              variant="outline"
              size="sm"
              className="shrink-0"
              loading={stopping || vm === 'stopping'}
              onClick={() => (busy ? setConfirming(true) : void stop())}
            >
              <Power />
              {t('devices.vm.stop')}
            </Button>
          ) : null}
        </div>
      ) : null}
      {outdated ? (
        <p className="text-xs text-muted-foreground">{t('devices.vm.outdated')}</p>
      ) : (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm" className="-ms-2 min-w-0 self-start px-2 text-muted-foreground hover:text-foreground">
              <Timer />
              <span className="truncate">
                {device.idleMinutes === 0
                  ? t('devices.vm.idleOff')
                  : t('devices.vm.idleAfter', { duration: duration(t, device.idleMinutes) })}
              </span>
              <ChevronDown className="opacity-60" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="max-w-72">
            <DropdownMenuLabel className="font-normal">{t('devices.vm.idleHint')}</DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuRadioGroup value={String(device.idleMinutes)} onValueChange={(value) => void setIdle(Number(value))}>
              {choices.map((minutes) => (
                <DropdownMenuRadioItem key={minutes} value={String(minutes)}>
                  {minutes === 0 ? t('devices.vm.idleNever') : t('devices.vm.idleChoice', { duration: duration(t, minutes) })}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      )}
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title={t('devices.vm.confirmTitle')}
        description={t('devices.vm.confirmBody', { what: busy ?? '' })}
        confirmLabel={t('devices.vm.stop')}
        destructive
        onConfirm={() => {
          // The dialog closes at once; the button shows the stop's progress.
          void stop();
        }}
      />
    </div>
  );
}
