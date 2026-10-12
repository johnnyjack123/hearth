import { cva, type VariantProps } from 'class-variance-authority';
import type { ComponentProps } from 'react';
import { Slot } from 'radix-ui';
import { cn } from '@/lib/utils.ts';
import { Spinner } from './spinner.tsx';

/**
 * Class builder for buttons, exported for elements that must look like a
 * button but are not one (e.g. router links styled via `asChild`).
 */
export const buttonVariants = cva(
  [
    'relative inline-flex shrink-0 select-none items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium',
    'transition-[color,background-color,box-shadow,transform,opacity] duration-(--duration-instant) ease-standard',
    'active:scale-98 disabled:pointer-events-none disabled:opacity-50 aria-disabled:cursor-not-allowed aria-disabled:opacity-50',
    '[&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*=size-])]:size-4',
  ],
  {
    variants: {
      variant: {
        primary: 'bg-primary text-primary-foreground shadow-elevation-1 hover:bg-primary/90',
        secondary: 'bg-secondary text-secondary-foreground hover:bg-accent hover:text-accent-foreground',
        outline: 'border border-border bg-card text-foreground hover:bg-accent hover:text-accent-foreground',
        ghost: 'text-foreground hover:bg-accent hover:text-accent-foreground',
        destructive: 'bg-destructive text-destructive-foreground shadow-elevation-1 hover:bg-destructive/90',
        link: 'h-auto px-0 text-primary underline-offset-4 hover:underline active:scale-100',
      },
      size: {
        sm: 'h-8 px-3 text-xs pointer-coarse:min-h-11',
        md: 'h-9 px-4 pointer-coarse:min-h-11',
        lg: 'h-11 px-6 text-base',
        icon: 'size-9 pointer-coarse:size-11',
        'icon-sm': 'size-8 rounded-md pointer-coarse:size-11',
      },
    },
    defaultVariants: { variant: 'secondary', size: 'md' },
  },
);

/**
 * Props of {@link Button}.
 */
export type ButtonProps = ComponentProps<'button'> &
  VariantProps<typeof buttonVariants> & {
    /** Render the single child instead of a `<button>`, merging props onto it. */
    asChild?: boolean;
    /** Shows a spinner, disables the button and sets `aria-busy`. */
    loading?: boolean;
  };

/**
 * The only button in the app. Icon-only buttons must pass `aria-label`.
 *
 * @param props - See {@link ButtonProps}.
 * @returns The button element.
 */
export function Button({ className, variant, size, asChild = false, loading = false, disabled, children, ...props }: ButtonProps) {
  const Component = asChild ? Slot.Root : 'button';
  return (
    <Component
      data-slot="button"
      className={cn(buttonVariants({ variant, size }), className)}
      disabled={asChild ? undefined : disabled || loading}
      aria-busy={loading || undefined}
      {...props}
    >
      {asChild ? (
        children
      ) : (
        <>
          {loading ? <Spinner /> : null}
          {children}
        </>
      )}
    </Component>
  );
}
