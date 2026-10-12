import { CircleAlert, CircleCheck, Info } from 'lucide-react';
import { Toaster as Sonner } from 'sonner';
import { usePreferences } from '@/lib/preferences.tsx';

/**
 * Toast outlet; render once at the root. Trigger toasts with `toast()` from
 * `sonner`. Styled entirely through tokens (sonner's own styles are off).
 *
 * @returns The toaster.
 */
export function Toaster() {
  const { resolvedTheme } = usePreferences();
  return (
    <Sonner
      theme={resolvedTheme}
      position="bottom-right"
      icons={{
        success: <CircleCheck className="size-4 text-success" />,
        error: <CircleAlert className="size-4 text-destructive" />,
        info: <Info className="size-4 text-primary" />,
      }}
      toastOptions={{
        unstyled: true,
        classNames: {
          toast:
            'flex w-(--width) items-start gap-3 rounded-lg bg-popover p-4 text-sm text-popover-foreground shadow-elevation-2',
          title: 'font-medium',
          description: 'mt-0.5 text-muted-foreground',
          icon: 'mt-0.5',
          actionButton: 'ms-auto rounded-md bg-primary px-2 py-1 text-xs font-medium text-primary-foreground',
          cancelButton: 'rounded-md bg-muted px-2 py-1 text-xs font-medium text-muted-foreground',
        },
      }}
    />
  );
}
