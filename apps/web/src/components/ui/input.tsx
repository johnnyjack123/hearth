import type { ComponentProps } from 'react';
import { cn } from '@/lib/utils.ts';

/**
 * Shared look of text inputs and textareas: border, focus ring and the error
 * state driven by `aria-invalid`.
 */
export const fieldControlClasses = cn(
  'w-full min-w-0 rounded-md border border-input bg-card px-3 text-sm text-foreground',
  'transition-[border-color,box-shadow] duration-(--duration-fast) ease-standard',
  'placeholder:text-muted-foreground',
  'focus-visible:border-ring focus-visible:shadow-elevation-1 focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring/25',
  'disabled:cursor-not-allowed disabled:opacity-50',
  'aria-invalid:border-destructive aria-invalid:ring-destructive/20',
);

/**
 * Single-line text input.
 *
 * @param props - Native input props.
 * @returns The input element.
 */
export function Input({ className, type = 'text', ...props }: ComponentProps<'input'>) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(fieldControlClasses, 'h-9 py-1 pointer-coarse:h-11', className)}
      {...props}
    />
  );
}

/**
 * Multi-line text input that grows with its content.
 *
 * @param props - Native textarea props.
 * @returns The textarea element.
 */
export function Textarea({ className, ...props }: ComponentProps<'textarea'>) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(fieldControlClasses, 'field-sizing-content min-h-20 py-2', className)}
      {...props}
    />
  );
}
