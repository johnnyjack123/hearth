import { useNavigate, useSearch } from '@tanstack/react-router';
import { BookMarked, KeyRound, Laptop, Palette, UserRound } from 'lucide-react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs.tsx';
import { useT } from '@/lib/preferences.tsx';
import { OwnAppsSection } from '../devices/own-apps.tsx';
import { Page } from '../shell/page.tsx';
import { AccountPanel } from './account-panel.tsx';
import { AppearancePanel } from './appearance-panel.tsx';
import { DevicesPanel } from './devices-panel.tsx';
import { HostCheckSection } from './host-check.tsx';
import { MemoryPanel } from './memory-panel.tsx';
import { ProvidersPanel } from './providers-panel.tsx';

/**
 * Tabs of the settings page, also used as the `?tab=` search parameter.
 */
export const SETTINGS_TABS = ['providers', 'devices', 'memory', 'appearance', 'account'] as const;

/**
 * One of {@link SETTINGS_TABS}.
 */
export type SettingsTab = (typeof SETTINGS_TABS)[number];

/**
 * Settings page (`/settings`) with tabs for API keys, paired devices, the
 * memory, appearance and account.
 *
 * @returns The settings page.
 */
export function SettingsPage() {
  const t = useT();
  const navigate = useNavigate();
  const { tab = 'providers' } = useSearch({ strict: false }) as { tab?: SettingsTab };

  return (
    <Page title={t('settings.title')}>
      <Tabs value={tab} onValueChange={(next) => void navigate({ to: '/settings', search: { tab: next as SettingsTab }, replace: true })}>
        <TabsList className="mb-8">
          <TabsTrigger value="providers">
            <KeyRound />
            {t('settings.tabProviders')}
          </TabsTrigger>
          <TabsTrigger value="devices">
            <Laptop />
            {t('settings.tabDevices')}
          </TabsTrigger>
          <TabsTrigger value="memory">
            <BookMarked />
            {t('settings.tabMemory')}
          </TabsTrigger>
          <TabsTrigger value="appearance">
            <Palette />
            {t('settings.tabAppearance')}
          </TabsTrigger>
          <TabsTrigger value="account">
            <UserRound />
            {t('settings.tabAccount')}
          </TabsTrigger>
        </TabsList>
        <TabsContent value="providers">
          <div className="flex flex-col gap-10">
            <ProvidersPanel />
            <HostCheckSection />
          </div>
        </TabsContent>
        <TabsContent value="devices">
          <div className="flex flex-col gap-10">
            <DevicesPanel />
            <OwnAppsSection />
          </div>
        </TabsContent>
        <TabsContent value="memory">
          <MemoryPanel />
        </TabsContent>
        <TabsContent value="appearance">
          <AppearancePanel />
        </TabsContent>
        <TabsContent value="account">
          <AccountPanel />
        </TabsContent>
      </Tabs>
    </Page>
  );
}
