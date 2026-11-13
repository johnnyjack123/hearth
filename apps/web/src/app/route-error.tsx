import { ErrorComponent, type ErrorComponentProps, useRouter } from '@tanstack/react-router';
import { CloudOff } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { useT } from '@/lib/preferences.tsx';
import { isTransientError } from '@/lib/queries.ts';
import { isStaleBuildError, reloadForNewBuild } from '@/lib/stale-build.ts';

/**
 * What a route shows when it fails. A part of the app that could not be
 * loaded after an update reloads the page instead of showing the error; a
 * server that cannot be reached right now gets a note and "try again".
 *
 * @param props - The error from the router.
 * @returns A short note while reloading, the note on the connection, else
 *   the router's error view.
 */
export function RouteError({ error, reset }: ErrorComponentProps) {
  const t = useT();
  const router = useRouter();
  const stale = isStaleBuildError(error);
  const [reloading, setReloading] = useState(stale);
  useEffect(() => {
    if (stale && !reloadForNewBuild()) {
      setReloading(false);
    }
  }, [stale]);
  if (reloading) {
    return (
      <div role="status" className="flex flex-1 items-center justify-center p-6 text-sm text-muted-foreground">
        {t('common.newVersion')}
      </div>
    );
  }
  if (isTransientError(error)) {
    return (
      <div role="alert" className="pad-safe-area flex min-h-dvh flex-1 flex-col items-center justify-center gap-3 bg-background p-6 text-center">
        <CloudOff className="size-6 text-muted-foreground" aria-hidden="true" />
        <p className="text-sm text-foreground">{t('errors.network')}</p>
        <p className="max-w-sm text-xs break-words text-muted-foreground">{error instanceof Error ? error.message : null}</p>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => {
            reset();
            void router.invalidate();
          }}
        >
          {t('common.retry')}
        </Button>
      </div>
    );
  }
  return <ErrorComponent error={error} />;
}
