import { Monitor, Moon, Sun } from 'lucide-react';
import { Card, Separator } from '@/components/ui/display.tsx';
import { SegmentedControl } from '@/components/ui/tabs.tsx';
import type { Locale } from '@/i18n/index.ts';
import { type Density, type ThemePreference, usePreferences } from '@/lib/preferences.tsx';
import { Section, SettingRow } from '../shell/page.tsx';

/**
 * Per-browser appearance settings: color scheme, density and language.
 *
 * @returns The appearance panel.
 */
export function AppearancePanel() {
  const { t, theme, setTheme, density, setDensity, locale, setLocale } = usePreferences();

  return (
    <Section title={t('appearance.title')} description={t('appearance.description')}>
      <Card>
        <SettingRow
          label={t('appearance.theme')}
          control={
            <SegmentedControl<ThemePreference>
              label={t('appearance.theme')}
              value={theme}
              onValueChange={setTheme}
              options={[
                { value: 'system', label: t('appearance.themeSystem'), icon: <Monitor /> },
                { value: 'light', label: t('appearance.themeLight'), icon: <Sun /> },
                { value: 'dark', label: t('appearance.themeDark'), icon: <Moon /> },
              ]}
            />
          }
        />
        <Separator />
        <SettingRow
          label={t('appearance.density')}
          control={
            <SegmentedControl<Density>
              label={t('appearance.density')}
              value={density}
              onValueChange={setDensity}
              options={[
                { value: 'compact', label: t('appearance.densityCompact') },
                { value: 'normal', label: t('appearance.densityNormal') },
                { value: 'comfortable', label: t('appearance.densityComfortable') },
              ]}
            />
          }
        />
        <Separator />
        <SettingRow
          label={t('appearance.language')}
          control={
            <SegmentedControl<Locale>
              label={t('appearance.language')}
              value={locale}
              onValueChange={setLocale}
              // Language names are shown in their own language on purpose.
              options={[
                { value: 'de', label: 'Deutsch' },
                { value: 'en', label: 'English' },
              ]}
            />
          }
        />
      </Card>
    </Section>
  );
}
