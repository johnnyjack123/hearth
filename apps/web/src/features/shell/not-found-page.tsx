import { Link } from '@tanstack/react-router';
import { Compass } from 'lucide-react';
import { Button } from '@/components/ui/button.tsx';
import { EmptyState } from '@/components/ui/display.tsx';
import { useT } from '@/lib/preferences.tsx';

/**
 * Fallback for unknown routes.
 *
 * @returns The not-found page.
 */
export function NotFoundPage() {
  const t = useT();
  return (
    <div className="flex min-h-dvh flex-1 items-center justify-center bg-background p-6">
      <EmptyState
        icon={<Compass />}
        title={t('errors.not_found')}
        action={
          <Button variant="primary" asChild>
            <Link to="/">{t('chat.backHome')}</Link>
          </Button>
        }
      />
    </div>
  );
}
