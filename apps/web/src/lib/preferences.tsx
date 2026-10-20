import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { detectLocale, type Locale, translate, type TranslateFn } from '@/i18n/index.ts';

/**
 * Color scheme preference; `system` follows the OS setting.
 */
export type ThemePreference = 'system' | 'light' | 'dark';

/**
 * Spacing density, see the density axis in docs/design/DESIGN.md.
 */
export type Density = 'compact' | 'normal' | 'comfortable';

interface PreferencesValue {
  theme: ThemePreference;
  resolvedTheme: 'light' | 'dark';
  density: Density;
  locale: Locale;
  setTheme: (theme: ThemePreference) => void;
  setDensity: (density: Density) => void;
  setLocale: (locale: Locale) => void;
  /** Extra views for developers, such as tokens and cost of each model call. */
  devMode: boolean;
  setDevMode: (on: boolean) => void;
  t: TranslateFn;
}

// Keys and values must match public/theme-init.js, which applies them before first paint.
const STORAGE = { theme: 'harness.theme', density: 'harness.density', locale: 'harness.locale' } as const;
// Not needed before the first paint, so not in theme-init.js.
const DEV_MODE_KEY = 'harness.devMode';

const PreferencesContext = createContext<PreferencesValue | null>(null);

function read<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const value = localStorage.getItem(key);
    return allowed.includes(value as T) ? (value as T) : fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Private mode: preferences simply do not persist.
  }
}

function systemPrefersDark(): boolean {
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

/**
 * Provides per-browser preferences (theme, density, language, developer
 * mode) and the
 * translation function, and keeps the `<html>` element in sync with them.
 *
 * @param props - Children to render.
 * @returns The provider element.
 */
export function PreferencesProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<ThemePreference>(() => read(STORAGE.theme, ['system', 'light', 'dark'], 'system'));
  const [density, setDensityState] = useState<Density>(() =>
    read(STORAGE.density, ['compact', 'normal', 'comfortable'], 'normal'),
  );
  const [locale, setLocaleState] = useState<Locale>(() => read(STORAGE.locale, ['de', 'en'], detectLocale()));
  const [systemDark, setSystemDark] = useState(systemPrefersDark);
  const [devMode, setDevModeState] = useState(() => read(DEV_MODE_KEY, ['on', 'off'], 'off') === 'on');

  useEffect(() => {
    const query = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => setSystemDark(query.matches);
    query.addEventListener('change', onChange);
    return () => query.removeEventListener('change', onChange);
  }, []);

  const resolvedTheme = theme === 'system' ? (systemDark ? 'dark' : 'light') : theme;

  useEffect(() => {
    document.documentElement.classList.toggle('dark', resolvedTheme === 'dark');
  }, [resolvedTheme]);

  useEffect(() => {
    const root = document.documentElement;
    if (density === 'normal') {
      delete root.dataset.density;
    } else {
      root.dataset.density = density;
    }
  }, [density]);

  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  const setTheme = useCallback((next: ThemePreference) => {
    write(STORAGE.theme, next);
    setThemeState(next);
  }, []);
  const setDensity = useCallback((next: Density) => {
    write(STORAGE.density, next);
    setDensityState(next);
  }, []);
  const setLocale = useCallback((next: Locale) => {
    write(STORAGE.locale, next);
    setLocaleState(next);
  }, []);
  const setDevMode = useCallback((on: boolean) => {
    write(DEV_MODE_KEY, on ? 'on' : 'off');
    setDevModeState(on);
  }, []);

  const value = useMemo<PreferencesValue>(
    () => ({
      theme,
      resolvedTheme,
      density,
      locale,
      setTheme,
      setDensity,
      setLocale,
      devMode,
      setDevMode,
      t: (key, params) => translate(locale, key, params),
    }),
    [theme, resolvedTheme, density, locale, setTheme, setDensity, setLocale, devMode, setDevMode],
  );

  return <PreferencesContext value={value}>{children}</PreferencesContext>;
}

/**
 * Reads the current preferences and setters.
 *
 * @returns The preferences context value.
 * @throws {Error} If used outside {@link PreferencesProvider}.
 */
export function usePreferences(): PreferencesValue {
  const value = useContext(PreferencesContext);
  if (!value) {
    throw new Error('usePreferences must be used inside PreferencesProvider');
  }
  return value;
}

/**
 * Shorthand for the translation function of the active language.
 *
 * @returns `t(key, params)`.
 */
export function useT(): TranslateFn {
  return usePreferences().t;
}
