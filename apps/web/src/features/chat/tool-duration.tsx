import { useEffect, useState } from 'react';
import { usePreferences } from '@/lib/preferences.tsx';

/**
 * Formats how long a tool ran: tenths below ten seconds, whole seconds
 * below a minute, then minutes and seconds.
 *
 * @param locale - UI language.
 * @param ms - Milliseconds.
 * @returns E.g. `0,4 s`, `12 s` or `2 min 5 s`.
 */
export function formatDuration(locale: string, ms: number): string {
  const seconds = ms / 1000;
  if (seconds < 10) {
    return `${new Intl.NumberFormat(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(seconds)} s`;
  }
  if (seconds < 60) {
    return `${Math.round(seconds)} s`;
  }
  const whole = Math.round(seconds);
  return `${Math.floor(whole / 60)} min ${whole % 60} s`;
}

/**
 * How long a tool call ran, next to its state; while it runs, the seconds
 * since this browser saw it start, so a slow command shows as slow and not
 * as silence. After a reload the count restarts, the stored value follows
 * once the call is done.
 *
 * @param props - The stored duration, if the call is done, and whether it
 *   still runs.
 * @returns The duration, or nothing for a short running call.
 */
export function ToolDuration({ ms, running }: { ms: number | undefined; running: boolean }) {
  const { t, locale } = usePreferences();
  const [since] = useState(() => Date.now());
  const [now, setNow] = useState(since);

  useEffect(() => {
    if (!running) {
      return;
    }
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running]);

  if (running) {
    const seconds = Math.floor((now - since) / 1000);
    // The first second is noise; a spinner says enough. Counting goes in
    // whole seconds.
    return seconds >= 1 ? (
      <span className="shrink-0 text-xs tabular-nums text-muted-foreground">{seconds < 60 ? `${seconds} s` : formatDuration(locale, seconds * 1000)}</span>
    ) : null;
  }
  if (ms === undefined) {
    return null;
  }
  const text = formatDuration(locale, ms);
  return (
    <span className="shrink-0 text-xs tabular-nums text-muted-foreground" title={t('agent.tool.duration', { duration: text })}>
      {text}
    </span>
  );
}
