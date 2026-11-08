import {
  DEFAULT_TONE,
  DEFAULT_VERBOSITY,
  EFFORT_LEVELS,
  type EffortLevel,
  type Tone,
  TONES,
  type Verbosity,
  VERBOSITY_LEVELS,
} from '@harness/shared';
import { AlignLeft, ArrowLeft, Brain, ChevronDown, ChevronRight, Container, MessageCircle, Server, Settings, Telescope } from 'lucide-react';
import { type ReactNode, useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
  Popover,
  PopoverContent,
  PopoverTrigger,
  Tooltip,
} from '@/components/ui/menu.tsx';
import { Slider } from '@/components/ui/slider.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';
import { type EnvironmentOption, StatusDot, WorkspaceMark } from './environment.tsx';
import type { HostOption } from './openrouter-host.tsx';

/** The effort slider's stops: the provider's default first, then pi's levels. */
const EFFORT_STOPS: readonly (EffortLevel | null)[] = [null, ...EFFORT_LEVELS];

/**
 * Props of {@link ChatOptions}.
 */
export interface ChatOptionsProps {
  /** The chat's environment, see `useChatEnvironment` and `useNewChatEnvironment`. */
  environment: EnvironmentOption;
  /** Research mode; null when no search provider is set up and the mode does not exist. */
  research: { on: boolean; onChange: (on: boolean) => void } | null;
  /** Thinking effort; null for the provider's default. */
  effort: EffortLevel | null;
  /** Changes the effort. */
  onEffortChange: (effort: EffortLevel | null) => void;
  /** Answer length. */
  verbosity: Verbosity;
  /** Changes the answer length. */
  onVerbosityChange: (verbosity: Verbosity) => void;
  /** Tone. */
  tone: Tone;
  /** Changes the tone. */
  onToneChange: (tone: Tone) => void;
  /** The OpenRouter host of the chat's model; null for other providers. */
  host: HostOption | null;
}

/**
 * The composer's settings behind one gear button: environment, research
 * mode, thinking effort, answer length, tone and the OpenRouter host. The button
 * is highlighted when something differs from the defaults and names it in
 * its label and tooltip, so the composer stays uncluttered without hiding
 * what is switched on.
 *
 * @param props - See {@link ChatOptionsProps}.
 * @returns The button with its menu.
 */
export function ChatOptions({
  environment,
  research,
  effort,
  onEffortChange,
  verbosity,
  onVerbosityChange,
  tone,
  onToneChange,
  host,
}: ChatOptionsProps) {
  const t = useT();
  const [open, setOpen] = useState(false);
  // A chosen environment and the hosts have a page of their own inside the menu.
  const [page, setPage] = useState<'main' | 'environment' | 'host'>('main');
  const { link, online, workspace } = environment;
  const agent = link?.harness;
  const unsaved = workspace !== null && (workspace.commits > 0 || workspace.changes > 0);

  const active = [
    ...(link ? [t('agent.linkedTo', { harness: link.harness, project: link.project })] : []),
    ...(research?.on ? [t('chat.research')] : []),
    ...(effort !== null ? [t('chat.effort.label', { level: t(`chat.effort.levels.${effort}`) })] : []),
    ...(verbosity !== DEFAULT_VERBOSITY ? [t('chat.verbosity.label', { level: t(`chat.verbosity.levels.${verbosity}`) })] : []),
    ...(tone !== DEFAULT_TONE ? [t('chat.tone.label', { level: t(`chat.tone.levels.${tone}`) })] : []),
    ...(host?.pinned ? [t('chat.host.label', { host: host.pinned })] : []),
  ];
  const label = active.length > 0 ? t('chat.options.labelActive', { active: active.join(', ') }) : t('chat.options.label');
  const close = () => setOpen(false);

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) {
          setPage('main');
        }
      }}
    >
      <Tooltip content={label}>
        <PopoverTrigger asChild>
          <Button
            variant="ghost"
            size="sm"
            aria-label={label}
            className={cn(
              'min-w-0 shrink-0 gap-1.5 px-2 text-muted-foreground hover:text-foreground',
              active.length > 0 && 'bg-primary/10 text-primary hover:bg-primary/15 hover:text-primary',
            )}
          >
            <Settings />
            {/* The highlight says something is on, label and tooltip say what; the
                model name keeps the room. Work missing from the folder stays visible. */}
            {link && unsaved && workspace ? <WorkspaceMark workspace={workspace} /> : null}
          </Button>
        </PopoverTrigger>
      </Tooltip>
      <PopoverContent
        data-chat-options=""
        align="start"
        side="top"
        className="flex max-h-(--radix-popover-content-available-height) w-[min(var(--spacing)*84,calc(100vw-var(--spacing)*8))] flex-col overflow-y-auto p-0"
      >
        {(page === 'environment' && link) || (page === 'host' && host) ? (
          <div className="flex flex-col gap-2 p-3">
            <Button variant="ghost" size="sm" className="self-start text-muted-foreground" onClick={() => setPage('main')}>
              <ArrowLeft />
              {t('chat.options.back')}
            </Button>
            {page === 'host' ? host?.details : environment.details(close)}
          </div>
        ) : (
          <div className="flex flex-col divide-y divide-border">
            <button
              type="button"
              aria-label={link ? undefined : t('agent.connect')}
              onClick={() => {
                if (link) {
                  setPage('environment');
                } else {
                  close();
                  environment.pick();
                }
              }}
              className={cn(
                'flex w-full items-center gap-3 px-4 py-3 text-start transition-colors duration-(--duration-instant) ease-standard hover:bg-accent',
                'focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring focus-visible:ring-inset',
              )}
            >
              <Container className={cn('size-4 shrink-0', link ? 'text-primary' : 'text-muted-foreground')} aria-hidden="true" />
              <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="text-sm font-medium text-foreground">{t('agent.environment')}</span>
                <span className="truncate text-xs text-muted-foreground">
                  {link ? t('agent.linkedTo', { harness: link.harness, project: link.project }) : t('chat.options.noEnvironment')}
                </span>
              </span>
              {link ? <StatusDot online={online} /> : null}
              {link && workspace ? <WorkspaceMark workspace={workspace} /> : null}
              <ChevronRight className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            </button>
            {research ? (
              <label className="flex cursor-pointer items-start gap-3 px-4 py-3">
                <Telescope className={cn('mt-0.5 size-4 shrink-0', research.on ? 'text-primary' : 'text-muted-foreground')} aria-hidden="true" />
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="text-sm font-medium text-foreground">{t('chat.research')}</span>
                  <span className="text-xs text-muted-foreground">{research.on ? t('chat.researchOn') : t('chat.researchOff')}</span>
                </span>
                <Switch checked={research.on} onCheckedChange={research.onChange} aria-label={t('chat.research')} />
              </label>
            ) : null}
            <LevelSetting
              icon={<Brain />}
              title={t('chat.effort.title')}
              stops={EFFORT_STOPS.map((stop) => t(`chat.effort.levels.${stop ?? 'default'}`))}
              index={Math.max(0, EFFORT_STOPS.indexOf(effort))}
              onIndexChange={(index) => onEffortChange(EFFORT_STOPS[index] ?? null)}
              ends
              hint={agent && effort === null ? t('chat.effort.agentDefault', { harness: agent }) : t(`chat.effort.hints.${effort ?? 'default'}`)}
              note={agent ? t('chat.effort.agentNote', { harness: agent }) : t('chat.effort.note')}
            />
            <LevelSetting
              icon={<AlignLeft />}
              title={t('chat.verbosity.title')}
              stops={VERBOSITY_LEVELS.map((level) => t(`chat.verbosity.levels.${level}`))}
              index={Math.max(0, VERBOSITY_LEVELS.indexOf(verbosity))}
              onIndexChange={(index) => onVerbosityChange(VERBOSITY_LEVELS[index] ?? DEFAULT_VERBOSITY)}
              hint={t(`chat.verbosity.hints.${verbosity}`)}
              note={agent ? t('chat.verbosity.agentNote', { harness: agent }) : t('chat.verbosity.note')}
            />
            <ToneSetting tone={tone} onToneChange={onToneChange} agent={agent} />
            {host ? (
              <button
                type="button"
                onClick={() => setPage('host')}
                className={cn(
                  'flex w-full items-center gap-3 px-4 py-3 text-start transition-colors duration-(--duration-instant) ease-standard hover:bg-accent',
                  'focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring focus-visible:ring-inset',
                )}
              >
                <Server className={cn('size-4 shrink-0', host.pinned ? 'text-primary' : 'text-muted-foreground')} aria-hidden="true" />
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="text-sm font-medium text-foreground">{t('chat.host.title')}</span>
                  <span className="truncate text-xs text-muted-foreground">{host.pinned ?? t('chat.host.auto')}</span>
                </span>
                <ChevronRight className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
              </button>
            ) : null}
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}

/**
 * The tone, picked from a list: unlike length and effort, the tones are not
 * steps of one scale.
 */
function ToneSetting({ tone, onToneChange, agent }: { tone: Tone; onToneChange: (tone: Tone) => void; agent: string | undefined }) {
  const t = useT();
  const name = t(`chat.tone.levels.${tone}`);
  return (
    <div className="flex flex-col gap-2 px-4 py-3">
      <div className="flex items-center gap-3">
        <MessageCircle className={cn('size-4 shrink-0', tone !== DEFAULT_TONE ? 'text-primary' : 'text-muted-foreground')} aria-hidden="true" />
        <span className="flex-1 text-sm font-medium text-foreground">{t('chat.tone.title')}</span>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="sm" data-tone="" aria-label={t('chat.tone.label', { level: name })}>
              {name}
              <ChevronDown />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuRadioGroup value={tone} onValueChange={(value) => onToneChange(TONES.find((level) => level === value) ?? DEFAULT_TONE)}>
              {TONES.map((level) => (
                <DropdownMenuRadioItem key={level} value={level}>
                  {t(`chat.tone.levels.${level}`)}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <div className="flex flex-col gap-1 ps-7">
        <p className="text-sm text-foreground">{t(`chat.tone.hints.${tone}`)}</p>
        <p className="text-xs text-muted-foreground">{agent ? t('chat.tone.agentNote', { harness: agent }) : t('chat.tone.note')}</p>
      </div>
    </div>
  );
}

/**
 * A setting picked from a few named levels: title with the level, a slider,
 * the names under it (all, or with `ends` the first and last), what the level
 * does and when it applies.
 */
function LevelSetting({
  icon,
  title,
  stops,
  index,
  onIndexChange,
  ends = false,
  hint,
  note,
}: {
  icon: ReactNode;
  title: string;
  stops: string[];
  index: number;
  onIndexChange: (index: number) => void;
  ends?: boolean;
  hint: string;
  note: string;
}) {
  const name = stops[index] ?? '';
  return (
    <div className="flex flex-col gap-2 px-4 py-3">
      <div className="flex items-center gap-3 [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-muted-foreground">
        {icon}
        <span className="flex-1 text-sm font-medium text-foreground">{title}</span>
        <span className="text-sm font-medium text-primary" aria-live="polite">
          {name}
        </span>
      </div>
      <div className="flex flex-col gap-1 ps-7">
        <Slider
          label={title}
          valueText={name}
          min={0}
          max={stops.length - 1}
          step={1}
          value={[index]}
          onValueChange={([value]) => onIndexChange(value ?? 0)}
        />
        <div className="flex justify-between text-xs text-muted-foreground" aria-hidden="true">
          {(ends ? [stops[0], stops.at(-1)] : stops).map((stop) => (
            <span key={stop}>{stop}</span>
          ))}
        </div>
        <p className="pt-1 text-sm text-foreground">{hint}</p>
        <p className="text-xs text-muted-foreground">{note}</p>
      </div>
    </div>
  );
}
