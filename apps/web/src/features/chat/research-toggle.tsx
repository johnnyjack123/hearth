import { Telescope } from 'lucide-react';
import { Button } from '@/components/ui/button.tsx';
import { Tooltip } from '@/components/ui/menu.tsx';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

/**
 * Research mode in the composer toolbar: on, the model may search the web
 * and read found pages without asking. Like granting a host, it is a
 * capability the user switches on per chat.
 *
 * @param props - Whether it is on, and how to change that.
 * @returns The toggle button.
 */
export function ResearchToggle({ pressed, onPressedChange }: { pressed: boolean; onPressedChange: (pressed: boolean) => void }) {
  const t = useT();
  return (
    <Tooltip content={pressed ? t('chat.researchOn') : t('chat.researchOff')}>
      <Button
        variant="ghost"
        size="sm"
        aria-pressed={pressed}
        aria-label={t('chat.research')}
        onClick={() => onPressedChange(!pressed)}
        className={cn(
          'min-w-0 gap-1.5 px-2 text-muted-foreground hover:text-foreground',
          pressed && 'bg-primary/10 text-primary hover:bg-primary/15 hover:text-primary',
        )}
      >
        <Telescope />
        <span className="max-sm:sr-only">{t('chat.research')}</span>
      </Button>
    </Tooltip>
  );
}
