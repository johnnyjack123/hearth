import { cva, type VariantProps } from 'class-variance-authority';
import { Avatar as AvatarPrimitive } from 'radix-ui';
import type { ComponentProps, ReactNode } from 'react';
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
