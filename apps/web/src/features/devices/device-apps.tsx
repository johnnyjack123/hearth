import type { AppInfo, DeviceSummary, RemoteProject } from '@harness/shared';
import { useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { ChevronRight, Package } from 'lucide-react';
import { useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { Badge } from '@/components/ui/display.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { usePreferences } from '@/lib/preferences.tsx';
import { queryKeys } from '@/lib/queries.ts';
import { cn } from '@/lib/utils.ts';
import { DeviceFiles } from './device-files.tsx';
import { OWN_APPS_ID } from './own-apps.tsx';

/**
 * The apps a project gets: the ones it switched on and everything they
 * require, each with the titles of the apps that brought it along.
 *
 * @param catalog - The device's apps.
 * @param enabled - What the project switched on.
 * @returns App name → titles of the apps that require it (empty: switched on itself).
 */
export function activeApps(catalog: AppInfo[], enabled: string[]): Map<string, string[]> {
  const active = new Map<string, string[]>();
  const visit = (name: string, by: string | null) => {
    const known = active.get(name);
    if (known) {
      if (by && !known.includes(by)) {
        known.push(by);
      }
      return;
    }
    active.set(name, by ? [by] : []);
    const app = catalog.find((candidate) => candidate.name === name);
    for (const required of app?.requires ?? []) {
      visit(required, app?.title ?? name);
    }
  };
  for (const name of enabled) {
    visit(name, null);
  }
  return active;
}

/**
 * Titles of the apps a project of a device gets, in catalog order.
 *
 * @param device - The device.
 * @param project - Project name.
 * @returns The titles, or null if the device never reported its apps.
 */
export function projectAppTitles(device: DeviceSummary, project: string): string[] | null {
  const report = device.apps;
  if (!report) {
    return null;
  }
  const enabled = report.projects.find((candidate) => candidate.name === project)?.enabled ?? [];
  const active = activeApps(report.catalog, enabled);
  return report.catalog.filter((app) => active.has(app.name)).map((app) => app.title);
}

/**
 * Switches for every app of a device in one of its projects. The device
 * decides: it only takes apps it knows for projects it offers, and the
 * change applies from the next start of the environment.
 *
 * @param props - The device and the project.
 * @returns The list.
 */
export function ProjectApps({ device, project }: { device: DeviceSummary; project: string }) {
  const { t } = usePreferences();
  const queryClient = useQueryClient();
  const [pending, setPending] = useState<string | null>(null);
  const report = device.apps;
  if (!report) {
    return (
      <p className="text-xs text-muted-foreground">
        {device.online ? t('devices.apps.outdatedDevice') : t('devices.apps.neverReported')}
      </p>
    );
  }
  const enabled = report.projects.find((candidate) => candidate.name === project)?.enabled ?? [];
  const active = activeApps(report.catalog, enabled);

  async function toggle(app: AppInfo, on: boolean) {
    const next = on ? [...enabled, app.name] : enabled.filter((name) => name !== app.name);
    const previous = queryClient.getQueryData<DeviceSummary[]>(queryKeys.devices);
    // Shown at once; the device's own report replaces it.
    queryClient.setQueryData<DeviceSummary[]>(queryKeys.devices, (devices) =>
      devices?.map((candidate) =>
        candidate.id === device.id && candidate.apps
          ? {
              ...candidate,
              apps: {
                ...candidate.apps,
                projects: candidate.apps.projects.map((entry) => (entry.name === project ? { ...entry, enabled: next } : entry)),
              },
            }
          : candidate,
      ),
    );
    setPending(app.name);
    try {
      const result = await api.devices.setApps(device.id, project, next);
      if (!result.ok) {
        queryClient.setQueryData(queryKeys.devices, previous);
        toast.error(t('devices.apps.failed', { detail: result.message }));
      }
    } catch (error) {
      queryClient.setQueryData(queryKeys.devices, previous);
      toast.error(errorMessage(t, error));
    } finally {
      setPending(null);
      void queryClient.invalidateQueries({ queryKey: queryKeys.devices });
    }
  }

  return (
    <div className="flex min-w-0 flex-col gap-2">
      <ul className="flex flex-col">
        {report.catalog.map((app) => {
          const requiredBy = active.get(app.name) ?? [];
          const on = active.has(app.name);
          // Brought along by another app: it goes with that one.
          const locked = requiredBy.length > 0;
          // Own programs reinstall by themselves after a change.
          const reinstalls = app.source === 'server' && app.outdated;
          let status: string | null = null;
          if (locked) {
            status = t('devices.apps.requiredBy', { apps: requiredBy.join(', ') });
          } else if (on && report.versions && app.version === null) {
            status = t('devices.apps.installsNext');
          } else if (on && reinstalls) {
            status = t('devices.apps.reinstallsNext');
          }
          return (
            <li key={app.name}>
              <label
                className={cn(
                  'flex cursor-pointer items-start gap-3 rounded-lg px-2 py-2 transition-colors duration-(--duration-instant) ease-standard hover:bg-accent',
                  'has-[:disabled]:cursor-default has-[:disabled]:hover:bg-transparent',
                )}
              >
                <Switch
                  checked={on}
                  disabled={!device.online || pending !== null || locked}
                  onCheckedChange={(checked) => void toggle(app, checked)}
                />
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
                    <span className="text-sm text-foreground">{app.title}</span>
                    {app.version !== null ? (
                      <Badge variant="neutral" title={t('devices.apps.installed')}>
                        {app.version}
                      </Badge>
                    ) : null}
                    {app.outdated && !reinstalls ? <Badge variant="warning">{t('devices.apps.outdated')}</Badge> : null}
                    {app.source === 'server' ? (
                      <Badge variant="primary" title={t('devices.apps.sourceServerHint')}>
                        {t('devices.apps.sourceServer')}
                      </Badge>
                    ) : null}
                    {app.source === 'local' ? <Badge title={t('devices.apps.sourceLocalHint')}>{t('devices.apps.sourceLocal')}</Badge> : null}
                  </span>
                  {app.description ? <span className="break-words text-xs text-muted-foreground">{app.description}</span> : null}
                  {status ? <span className="break-words text-xs text-muted-foreground">{status}</span> : null}
                </span>
              </label>
            </li>
          );
        })}
      </ul>
      <p className="text-xs text-muted-foreground">
        {device.online ? t('devices.apps.hint') : t('devices.apps.offline')}
        {device.online && !report.versions ? ` ${t('devices.apps.versionsLater')}` : null}
      </p>
      <Link
        to="/settings"
        search={{ tab: 'devices' }}
        hash={OWN_APPS_ID}
        className="self-start text-xs font-medium text-primary underline-offset-4 hover:underline"
      >
        {t('devices.apps.ownLink')}
      </Link>
    </div>
  );
}

/**
 * The apps of every project a device offers, one folding row each, for
 * Settings → Devices.
 *
 * @param props - The device.
 * @returns The section, or nothing without projects.
 */
export function DeviceApps({ device }: { device: DeviceSummary }) {
  const { t } = usePreferences();
  if (device.projects.length === 0) {
    return null;
  }
  return (
    <div className="flex min-w-0 flex-col gap-0.5 pt-2">
      <span className="text-sm text-foreground">{t('devices.apps.title')}</span>
      {device.projects.map((project) => (
        <ProjectRow key={project.name} device={device} project={project} />
      ))}
      <DeviceFiles device={device} />
    </div>
  );
}

function ProjectRow({ device, project }: { device: DeviceSummary; project: RemoteProject }) {
  const { t } = usePreferences();
  const [open, setOpen] = useState(false);
  const titles = projectAppTitles(device, project.name);
  const summary = titles === null ? null : titles.length > 0 ? titles.join(', ') : t('devices.apps.none');
  return (
    <div className="flex min-w-0 flex-col">
      <Button
        variant="ghost"
        size="sm"
        className="-ms-2 min-w-0 justify-start px-2"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        <ChevronRight
          className={cn('transition-transform duration-(--duration-fast) ease-standard rtl:-scale-x-100', open && 'rotate-90 rtl:-rotate-90')}
        />
        <span className="shrink-0 font-medium">{project.name}</span>
        {summary ? <span className="min-w-0 truncate text-muted-foreground">{summary}</span> : null}
      </Button>
      {open ? (
        <div className="ps-4">
          <ProjectApps device={device} project={project.name} />
        </div>
      ) : null}
    </div>
  );
}

/**
 * The apps of a linked chat's project, for the environment menu: what the
 * agent has, as a button that opens the switches.
 *
 * @param props - The device, the project and what the button does.
 * @returns The line.
 */
export function ProjectAppsLine({ device, project, onChange }: { device: DeviceSummary; project: string; onChange: () => void }) {
  const { t } = usePreferences();
  const titles = projectAppTitles(device, project);
  if (titles === null) {
    return <p className="text-xs text-muted-foreground">{t('devices.apps.neverReported')}</p>;
  }
  return (
    <Button
      variant="ghost"
      size="sm"
      className="-ms-2 min-w-0 self-start px-2 text-muted-foreground hover:text-foreground"
      aria-haspopup="dialog"
      title={t('devices.apps.change')}
      onClick={onChange}
    >
      <Package />
      <span className="truncate">
        {t('devices.apps.summary', { apps: titles.length > 0 ? titles.join(', ') : t('devices.apps.none') })}
      </span>
      <ChevronRight className="opacity-60 rtl:-scale-x-100" />
    </Button>
  );
}

/**
 * The switches of {@link ProjectApps} in a dialog, opened from the
 * environment menu of a chat.
 *
 * @param props - Open state, the device and the project.
 * @returns The dialog.
 */
export function ProjectAppsDialog({
  open,
  onOpenChange,
  device,
  project,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  device: DeviceSummary;
  project: string;
}) {
  const { t } = usePreferences();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('devices.apps.dialogTitle', { project })}</DialogTitle>
          <DialogDescription>{t('devices.apps.dialogDescription', { device: device.name })}</DialogDescription>
        </DialogHeader>
        <div className="max-h-[60vh] overflow-y-auto">
          <ProjectApps device={device} project={project} />
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t('common.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
