import { cn } from '@/lib/utils.ts';

/**
 * Indeterminate progress indicator. Decorative: pair it with text or
 * `aria-busy` on the element that is loading.
 *
 * @param props - Optional extra classes, e.g. a `size-*` override.
 * @returns The spinner SVG.
 */
export function Spinner({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true" className={cn('size-4 animate-spin', className)}>
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeOpacity="0.2" strokeWidth="2.5" />
      <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
    </svg>
  );
}
