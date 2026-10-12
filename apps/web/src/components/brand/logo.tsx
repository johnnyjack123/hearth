import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

/**
 * Product mark: two rails joined by a hub, a nod to a harness connecting
 * chats with environments. Colors come from the primary token pair.
 *
 * @param props - Optional classes (size via `size-*`) and whether to show the wordmark.
 * @returns The logo.
 */
export function Logo({ className, withWordmark = false }: { className?: string; withWordmark?: boolean }) {
  const t = useT();
  return (
    <span className={cn('inline-flex items-center gap-2.5', className)}>
      <svg viewBox="0 0 32 32" aria-hidden="true" className="size-7 shrink-0">
        <rect width="32" height="32" rx="9" className="fill-primary" />
        <path
          d="M10 9.5v13M22 9.5v13M10 16h12"
          className="stroke-primary-foreground"
          strokeWidth="2.6"
          strokeLinecap="round"
          fill="none"
        />
        <circle cx="16" cy="16" r="2.6" className="fill-primary-foreground" />
      </svg>
      {withWordmark ? (
        <span className="text-base font-semibold tracking-tight text-foreground">{t('common.appName')}</span>
      ) : null}
    </span>
  );
}
