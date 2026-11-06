import { DEFAULT_VERBOSITY, EFFORT_LEVELS, type EffortLevel, type Verbosity, VERBOSITY_LEVELS } from '@harness/shared';
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
  /**
   * Research mode of the chat used last; a new chat starts with it.
   * Existing chats keep their own.
   */
  research: boolean;
  /** Thinking effort of the chat used last (null: the provider's default); a new chat starts with it. */
  effort: EffortLevel | null;
  /** Answer length of the chat used last; a new chat starts with it. */
  verbosity: Verbosity;
  /** Jev judges prompt injection warnings right away, see {@link autoAssessEnabled}. */
  autoAssess: boolean;
  /**
   * The model's thinking shows while it writes; set by opening or closing
   * a thinking block, which the next ones follow.
   */
  showThinking: boolean;
  setTheme: (theme: ThemePreference) => void;
  setDensity: (density: Density) => void;
  setLocale: (locale: Locale) => void;
  setResearch: (on: boolean) => void;
  setEffort: (effort: EffortLevel | null) => void;
  setVerbosity: (verbosity: Verbosity) => void;
  setAutoAssess: (on: boolean) => void;
  setShowThinking: (on: boolean) => void;
  t: TranslateFn;
}

// Keys and values of theme, density and locale must match public/theme-init.js,
// which applies them before first paint.
const STORAGE = {
  theme: 'harness.theme',
  density: 'harness.density',
  locale: 'harness.locale',
  research: 'harness.research',
  effort: 'harness.effort',
  verbosity: 'harness.verbosity',
  autoAssess: 'harness.autoAssess',
  showThinking: 'harness.showThinking',
} as const;

const PreferencesContext = createContext<PreferencesValue | null>(null);

/**
 * Whether Jev should judge prompt injection warnings as soon as they come
 * up (on unless switched off in the settings); read by the chat transport
 * at request time.
 *
 * @returns True unless switched off.
 */
export function autoAssessEnabled(): boolean {
  return read(STORAGE.autoAssess, ['on', 'off'], 'on') === 'on';
}

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
 * Provides per-browser preferences (theme, density, language, research
 * mode) and the translation function, and keeps the `<html>` element in
 * sync with them.
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
  const [research, setResearchState] = useState(() => read(STORAGE.research, ['on', 'off'], 'off') === 'on');
  const [effort, setEffortState] = useState<EffortLevel | null>(() => {
    const stored = read(STORAGE.effort, ['default', ...EFFORT_LEVELS], 'default');
    return stored === 'default' ? null : stored;
  });
  const [verbosity, setVerbosityState] = useState<Verbosity>(() => read(STORAGE.verbosity, VERBOSITY_LEVELS, DEFAULT_VERBOSITY));
  const [systemDark, setSystemDark] = useState(systemPrefersDark);

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
  const setEffort = useCallback((next: EffortLevel | null) => {
    write(STORAGE.effort, next ?? 'default');
    setEffortState(next);
  }, []);
  const setVerbosity = useCallback((next: Verbosity) => {
    write(STORAGE.verbosity, next);
    setVerbosityState(next);
  }, []);
  const setResearch = useCallback((on: boolean) => {
    write(STORAGE.research, on ? 'on' : 'off');
    setResearchState(on);
  }, []);
  const [autoAssess, setAutoAssessState] = useState(autoAssessEnabled);
  const setAutoAssess = useCallback((on: boolean) => {
    write(STORAGE.autoAssess, on ? 'on' : 'off');
    setAutoAssessState(on);
  }, []);
  const [showThinking, setShowThinkingState] = useState(() => read(STORAGE.showThinking, ['on', 'off'], 'on') === 'on');
  const setShowThinking = useCallback((on: boolean) => {
    write(STORAGE.showThinking, on ? 'on' : 'off');
    setShowThinkingState(on);
  }, []);

  const value = useMemo<PreferencesValue>(
    () => ({
      theme,
      resolvedTheme,
      density,
      locale,
      research,
      effort,
      verbosity,
      autoAssess,
      showThinking,
      setTheme,
      setDensity,
      setLocale,
      setResearch,
      setEffort,
      setVerbosity,
      setAutoAssess,
      setShowThinking,
      t: (key, params) => translate(locale, key, params),
    }),
    [
      theme,
      resolvedTheme,
      density,
      locale,
      research,
      effort,
      verbosity,
      autoAssess,
      showThinking,
      setTheme,
      setDensity,
      setLocale,
      setResearch,
      setEffort,
      setVerbosity,
      setAutoAssess,
      setShowThinking,
    ],
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
