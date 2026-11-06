import { Slider as SliderPrimitive } from 'radix-ui';
import type { ComponentProps } from 'react';
import { cn } from '@/lib/utils.ts';

/**
 * Picks a value on a scale by dragging or with the arrow keys, e.g. a level
 * from a fixed list. One thumb per value.
 *
 * @param props - Radix slider props plus `label`, the accessible name of
 *   the thumbs, and `valueText`, what a screen reader says for the value
 *   (e.g. the level's name instead of its index).
 * @returns The slider.
 */
export function Slider({
  className,
  label,
  valueText,
  ...props
}: ComponentProps<typeof SliderPrimitive.Root> & { label: string; valueText?: string }) {
  const values = props.value ?? props.defaultValue ?? [0];
  return (
    <SliderPrimitive.Root
      className={cn(
        'relative flex w-full touch-none select-none items-center py-2 data-disabled:cursor-not-allowed data-disabled:opacity-50',
        className,
      )}
      {...props}
    >
      <SliderPrimitive.Track className="relative h-1.5 w-full grow overflow-hidden rounded-full bg-input">
        <SliderPrimitive.Range className="absolute h-full bg-primary" />
      </SliderPrimitive.Track>
      {values.map((_, index) => (
        <SliderPrimitive.Thumb
          key={index}
          aria-label={label}
          aria-valuetext={valueText}
          className={cn(
            'relative block size-5 cursor-grab rounded-full bg-control-thumb shadow-elevation-1 active:cursor-grabbing',
            'transition-transform duration-(--duration-fast) ease-standard hover:scale-110',
            'focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring',
            // Larger invisible hit area for touch without changing the visual size.
            'pointer-coarse:after:absolute pointer-coarse:after:-inset-3 pointer-coarse:after:content-[""]',
          )}
        />
      ))}
    </SliderPrimitive.Root>
  );
}
