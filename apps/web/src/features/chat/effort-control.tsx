import { EFFORT_LEVELS, type EffortLevel } from '@harness/shared';
import { Brain } from 'lucide-react';
import { Button } from '@/components/ui/button.tsx';
import { Popover, PopoverContent, PopoverTrigger, Tooltip } from '@/components/ui/menu.tsx';
import { Slider } from '@/components/ui/slider.tsx';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

/** The slider's stops: the provider's default first, then pi's levels. */
const STOPS: readonly (EffortLevel | null)[] = [null, ...EFFORT_LEVELS];

/**
 * Thinking effort in the composer toolbar: a button with the current level
 * that opens a slider. Without a level the provider decides, as before.
 *
 * @param props - The level (null for the provider's default) and how to change it.
 * @returns The control.
 */
export function EffortControl({ effort, onEffortChange }: { effort: EffortLevel | null; onEffortChange: (effort: EffortLevel | null) => void }) {
  const t = useT();
  const name = t(`chat.effort.levels.${effort ?? 'default'}`);
  const index = Math.max(0, STOPS.indexOf(effort));
  return (
    <Popover>
      <Tooltip content={t('chat.effort.label', { level: name })}>
        <PopoverTrigger asChild>
          <Button
            variant="ghost"
            size="sm"
            aria-label={t('chat.effort.label', { level: name })}
            className={cn(
              'min-w-0 gap-1.5 px-2 text-muted-foreground hover:text-foreground',
              effort !== null && 'bg-primary/10 text-primary hover:bg-primary/15 hover:text-primary',
            )}
          >
            <Brain />
            <span className="max-sm:sr-only">{name}</span>
          </Button>
        </PopoverTrigger>
      </Tooltip>
      <PopoverContent align="start" side="top" className="flex w-[min(var(--spacing)*80,calc(100vw-var(--spacing)*8))] flex-col gap-3 p-4">
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-sm font-medium text-foreground">{t('chat.effort.title')}</span>
          <span className="text-sm font-medium text-primary" aria-live="polite">
            {name}
          </span>
        </div>
        <Slider
          label={t('chat.effort.title')}
          min={0}
          max={STOPS.length - 1}
          step={1}
          value={[index]}
          onValueChange={([value]) => onEffortChange(STOPS[value ?? 0] ?? null)}
        />
        <div className="flex justify-between text-xs text-muted-foreground" aria-hidden="true">
          <span>{t('chat.effort.levels.default')}</span>
          <span>{t('chat.effort.levels.max')}</span>
        </div>
        <p className="text-sm text-foreground">{t(`chat.effort.hints.${effort ?? 'default'}`)}</p>
        <p className="text-xs text-muted-foreground">{t('chat.effort.note')}</p>
      </PopoverContent>
    </Popover>
  );
}
