import type { DeviceSummary } from '@harness/shared';
import { useQueryClient } from '@tanstack/react-query';
import { Laptop, Trash2 } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { ConfirmDialog } from '@/components/ui/dialog.tsx';
import { Badge, Card, EmptyState, Separator, Skeleton } from '@/components/ui/display.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { springs } from '@/lib/motion.ts';
import { usePreferences } from '@/lib/preferences.tsx';
import { queryKeys, useDevices } from '@/lib/queries.ts';
import { DeviceVm } from '../devices/device-vm.tsx';
import { Section } from '../shell/page.tsx';

/**
 * Lists the computers paired via `harness login` and lets the user revoke
 * them.
 *
 * @returns The devices panel.
 */
export function DevicesPanel() {
  const { t } = usePreferences();
  const devices = useDevices(true);

  return (
    <Section title={t('devices.title')} description={t('devices.description')}>
      <Card>
        {devices.isPending ? (
          <div className="flex flex-col gap-3 p-4">
            <Skeleton className="h-10 w-full" />
          </div>
        ) : devices.data?.length ? (
          <ul>
            <AnimatePresence initial={false}>
              {devices.data.map((device, index) => (
                <motion.li
                  key={device.id}
                  layout="position"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={springs.layout}
                >
                  {index > 0 ? <Separator /> : null}
                  <DeviceRow device={device} />
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
        ) : (
          <EmptyState icon={<Laptop />} title={t('devices.empty')} description={t('devices.emptyHint')} />
        )}
      </Card>
    </Section>
  );
}

function DeviceRow({ device }: { device: DeviceSummary }) {
  const { t, locale } = usePreferences();
  const queryClient = useQueryClient();
  const [removing, setRemoving] = useState(false);
  const date = (epoch: number) => new Date(epoch).toLocaleDateString(locale);

  async function setJevReview(jevReview: boolean) {
    // Shown at once; the answer of the server replaces it.
    queryClient.setQueryData<DeviceSummary[]>(queryKeys.devices, (devices) =>
      devices?.map((candidate) => (candidate.id === device.id ? { ...candidate, jevReview } : candidate)),
    );
    try {
      const updated = await api.devices.update(device.id, { jevReview });
      queryClient.setQueryData<DeviceSummary[]>(queryKeys.devices, (devices) =>
        devices?.map((candidate) => (candidate.id === device.id ? updated : candidate)),
      );
    } catch (error) {
      toast.error(errorMessage(t, error));
      await queryClient.invalidateQueries({ queryKey: queryKeys.devices });
    }
  }

  async function remove() {
    try {
      await api.devices.remove(device.id);
      await queryClient.invalidateQueries({ queryKey: queryKeys.devices });
      toast.success(t('devices.removed'));
    } catch (error) {
      toast.error(errorMessage(t, error));
      throw error;
    }
  }

  return (
    <div className="flex items-start gap-4 p-4">
      <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
        <Laptop className="size-5" aria-hidden="true" />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate text-sm font-medium text-foreground">{device.name}</span>
          {device.online ? (
            <Badge variant="success" className="shrink-0">
              {t('devices.online')}
            </Badge>
          ) : null}
        </span>
        <span className="text-xs text-muted-foreground">
          {t('devices.paired', { date: date(device.createdAt) })} ·{' '}
          {device.lastUsedAt ? t('devices.lastUsed', { date: date(device.lastUsedAt) }) : t('devices.neverUsed')}
        </span>
        <DeviceVm device={device} />
        <label className="flex cursor-pointer items-start gap-3 pt-2">
          <Switch checked={device.jevReview} onCheckedChange={(checked) => void setJevReview(checked)} />
          <span className="flex min-w-0 flex-col gap-0.5">
            <span className="text-sm text-foreground">{t('devices.jevReview')}</span>
            <span className="text-xs text-muted-foreground">{t('devices.jevReviewHint')}</span>
          </span>
        </label>
      </div>
      <Button variant="ghost" size="icon-sm" aria-label={t('common.delete')} onClick={() => setRemoving(true)}>
        <Trash2 />
      </Button>
      <ConfirmDialog
        open={removing}
        onOpenChange={setRemoving}
        title={t('devices.removeTitle')}
        description={t('devices.removeBody', { name: device.name })}
        confirmLabel={t('common.delete')}
        destructive
        onConfirm={remove}
      />
    </div>
  );
}
