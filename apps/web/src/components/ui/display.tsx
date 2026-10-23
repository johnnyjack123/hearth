import { cva, type VariantProps } from 'class-variance-authority';
import { useReducedMotion } from 'motion/react';
import { Avatar as AvatarPrimitive } from 'radix-ui';
import { type ComponentProps, type CSSProperties, type ReactNode, useLayoutEffect, useRef, useState } from 'react';
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
 * @param props - The text, classes for the outer element, and whether the
 *   text wraps on touch screens instead, where nothing can point at it.
 * @returns The text.
 */
export function MarqueeText({ text, className, wrapOnTouch = false }: { text: string; className?: string; wrapOnTouch?: boolean }) {
  const outer = useRef<HTMLSpanElement>(null);
  const inner = useRef<HTMLSpanElement>(null);
  const [shift, setShift] = useState(0);
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
  const style = scrolls
    ? ({ '--marquee-shift': `-${shift}px`, '--marquee-duration': `${Math.max(shift / speeds.marquee, durations.slow)}s` } as CSSProperties)
    : undefined;
  return (
    <span ref={outer} title={shift > 0 ? text : undefined} className={cn('block min-w-0 overflow-hidden', className)}>
      <span
        ref={inner}
        style={style}
        className={cn(
          'inline-block max-w-full truncate align-top',
          wrapOnTouch && 'pointer-coarse:whitespace-normal pointer-coarse:break-words',
          scrolls && MARQUEE_ACTIVE,
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
