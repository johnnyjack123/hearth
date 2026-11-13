import { cva, type VariantProps } from 'class-variance-authority';
import { useReducedMotion } from 'motion/react';
import { Avatar as AvatarPrimitive } from 'radix-ui';
import { type ComponentProps, type CSSProperties, type ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { durations, speeds } from '@/lib/motion.ts';
import { cn } from '@/lib/utils.ts';

const badgeVariants = cva(
  'inline-flex items-center gap-1 whitespace-nowrap rounded-sm px-1.5 py-0.5 text-xs font-medium [&_svg]:size-3',
  {
    variants: {
      variant: {
        neutral: 'bg-muted text-muted-foreground',
        primary: 'bg-primary/12 text-primary',
        success: 'bg-success/12 text-success',
        warning: 'bg-warning text-warning-foreground',
        destructive: 'bg-destructive-subtle text-destructive',
      },
    },
    defaultVariants: { variant: 'neutral' },
  },
);

/**
 * Small status label. Pair color with text; never rely on color alone.
 *
 * @param props - Span props plus `variant`.
 * @returns The badge.
 */
export function Badge({ className, variant, ...props }: ComponentProps<'span'> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />;
}

/**
 * Round user avatar showing initials (or an image when provided).
 *
 * @param props - Display name, optional image and classes.
 * @returns The avatar.
 */
export function Avatar({ name, src, className }: { name: string; src?: string | null; className?: string }) {
  const initials =
    name
      .split(/[\s@._-]+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0]?.toUpperCase())
      .join('') || '?';
  return (
    <AvatarPrimitive.Root className={cn('relative inline-flex size-8 shrink-0 overflow-hidden rounded-full', className)}>
      {src ? <AvatarPrimitive.Image src={src} alt="" className="size-full object-cover" /> : null}
      <AvatarPrimitive.Fallback className="flex size-full items-center justify-center bg-primary/15 text-xs font-semibold text-primary">
        {initials}
      </AvatarPrimitive.Fallback>
    </AvatarPrimitive.Root>
  );
}

/**
 * Placeholder in the shape of content that is still loading.
 *
 * @param props - Div props; size it like the real content.
 * @returns The skeleton block.
 */
export function Skeleton({ className, ...props }: ComponentProps<'div'>) {
  return <div aria-hidden="true" className={cn('animate-skeleton rounded-md bg-muted', className)} {...props} />;
}

/** With `autoplay` on touch screens: pause between runs, and rest at the end of one. */
const AUTOPLAY_PAUSE_MS = 5_000;
const AUTOPLAY_REST_MS = 1_500;

/** Scrolls while the `group/marquee` control is pointed at, focused or highlighted (cmdk). */
const MARQUEE_ACTIVE = [
  'group-hover/marquee:max-w-none group-hover/marquee:animate-marquee',
  'group-focus-visible/marquee:max-w-none group-focus-visible/marquee:animate-marquee',
  'group-data-[selected=true]/marquee:max-w-none group-data-[selected=true]/marquee:animate-marquee',
];

/**
 * One line of text, cut off with an ellipsis. When it does not fit, it
 * scrolls once to its end while the surrounding control is pointed at,
 * focused or highlighted, so long names stay readable; put the class
 * `group/marquee` on that control. The full text is also the tooltip, which
 * is what users who prefer reduced motion get instead.
 *
 * @param props - The text, classes for the outer element, whether the text
 *   wraps on touch screens instead, where nothing can point at it, and
 *   whether it scrolls by itself every few seconds there (`autoplay`, for
 *   a control that opens something when tapped).
 * @returns The text.
 */
export function MarqueeText({
  text,
  className,
  wrapOnTouch = false,
  autoplay = false,
}: {
  text: string;
  className?: string;
  wrapOnTouch?: boolean;
  autoplay?: boolean;
}) {
  const outer = useRef<HTMLSpanElement>(null);
  const inner = useRef<HTMLSpanElement>(null);
  const [shift, setShift] = useState(0);
  const [playing, setPlaying] = useState(false);
  const reducedMotion = useReducedMotion();

  useLayoutEffect(() => {
    const box = outer.current;
    const line = inner.current;
    if (!box || !line) {
      return;
    }
    const measure = () => setShift(Math.max(0, line.scrollWidth - box.clientWidth));
    measure();
    // The web font may arrive after the first measurement and change the width.
    void document.fonts?.ready.then(measure);
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    return () => observer.disconnect();
  }, [text]);

  const scrolls = shift > 0 && !reducedMotion;
  const seconds = Math.max(shift / speeds.marquee, durations.slow);

  // Touch screens cannot point at the control: run, rest at the end, jump
  // back, and run again after a pause, like a car radio.
  useEffect(() => {
    if (!autoplay || !scrolls || !window.matchMedia('(hover: none)').matches) {
      setPlaying(false);
      return;
    }
    // Delay and run of the animation, then a moment at the end.
    const run = (seconds + durations.slow) * 1000 + AUTOPLAY_REST_MS;
    let timer = 0;
    const cycle = (on: boolean) => {
      setPlaying(on);
      timer = window.setTimeout(() => cycle(!on), on ? run : AUTOPLAY_PAUSE_MS);
    };
    cycle(false);
    return () => window.clearTimeout(timer);
  }, [autoplay, scrolls, seconds]);

  const style = scrolls ? ({ '--marquee-shift': `-${shift}px`, '--marquee-duration': `${seconds}s` } as CSSProperties) : undefined;
  return (
    <span ref={outer} title={shift > 0 ? text : undefined} className={cn('block min-w-0 overflow-hidden', className)}>
      <span
        ref={inner}
        style={style}
        className={cn(
          'inline-block max-w-full truncate align-top',
          wrapOnTouch && 'pointer-coarse:whitespace-normal pointer-coarse:break-words',
          scrolls && MARQUEE_ACTIVE,
          playing && 'max-w-none animate-marquee',
        )}
      >
        {text}
      </span>
    </span>
  );
}

/**
 * Keyboard key, e.g. in shortcut hints.
 *
 * @param props - kbd props.
 * @returns The key element.
 */
export function Kbd({ className, ...props }: ComponentProps<'kbd'>) {
  return (
    <kbd
      className={cn(
        'inline-flex h-5 min-w-5 items-center justify-center rounded-xs border border-border bg-muted px-1 font-mono text-xs text-muted-foreground',
        className,
      )}
      {...props}
    />
  );
}

/**
 * Raised surface grouping related content.
 *
 * @param props - Div props.
 * @returns The card.
 */
export function Card({ className, ...props }: ComponentProps<'div'>) {
  return <div className={cn('rounded-xl bg-card text-card-foreground shadow-elevation-1', className)} {...props} />;
}

/**
 * Thin bar for how much of something is done. The fill grows with a
 * transform, which reduced motion still allows (it only changes size).
 *
 * @param props - Done and total, an accessible label and optional classes.
 * @returns The bar.
 */
export function ProgressBar({ value, max, label, className }: { value: number; max: number; label: string; className?: string }) {
  const share = max > 0 ? Math.min(1, Math.max(0, value / max)) : 0;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value}
      className={cn('h-1.5 w-full overflow-hidden rounded-full bg-muted', className)}
    >
      <div
        className="h-full w-full origin-left rounded-full bg-primary transition-transform duration-(--duration-normal) ease-standard rtl:origin-right"
        style={{ transform: `scaleX(${share})` }}
      />
    </div>
  );
}

/**
 * Horizontal divider.
 *
 * @param props - Optional classes.
 * @returns The separator.
 */
export function Separator({ className }: { className?: string }) {
  return <div role="separator" className={cn('h-px w-full shrink-0 bg-border', className)} />;
}

/**
 * Placeholder for lists or pages without content: explains why it is empty
 * and offers the next step.
 *
 * @param props - Icon, title, description and optional action.
 * @returns The empty state.
 */
export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
}: {
  icon?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn('flex flex-col items-center gap-3 px-6 py-10 text-center', className)}>
      {icon ? (
        <div className="flex size-11 items-center justify-center rounded-xl bg-muted text-muted-foreground [&_svg]:size-5">{icon}</div>
      ) : null}
      <div className="flex max-w-sm flex-col gap-1">
        <p className="text-sm font-medium text-foreground">{title}</p>
        {description ? <p className="text-sm text-muted-foreground">{description}</p> : null}
      </div>
      {action}
    </div>
  );
}
