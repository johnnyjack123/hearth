import { ErrorComponent, type ErrorComponentProps } from '@tanstack/react-router';
import { useEffect, useState } from 'react';
import { useT } from '@/lib/preferences.tsx';
import { isStaleBuildError, reloadForNewBuild } from '@/lib/stale-build.ts';

/**
 * What a route shows when it fails. A part of the app that could not be
 * loaded after an update reloads the page instead of showing the error.
 *
 * @param props - The error from the router.
 * @returns A short note while reloading, else the router's error view.
 */
export function RouteError({ error }: ErrorComponentProps) {
  const t = useT();
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
  return <ErrorComponent error={error} />;
}
