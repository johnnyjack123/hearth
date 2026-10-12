import { Switch as SwitchPrimitive } from 'radix-ui';
import type { ComponentProps } from 'react';
import { cn } from '@/lib/utils.ts';

/**
 * On/off toggle for settings that apply immediately.
 *
 * @param props - Radix switch props.
 * @returns The switch.
 */
export function Switch({ className, ...props }: ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      className={cn(
        'peer relative inline-flex h-6 w-10 shrink-0 cursor-pointer items-center rounded-full bg-input p-0.5',
        'transition-colors duration-(--duration-fast) ease-standard data-[state=checked]:bg-primary',
        'disabled:cursor-not-allowed disabled:opacity-50',
        // Larger invisible hit area for touch without changing the visual size.
        'pointer-coarse:after:absolute pointer-coarse:after:-inset-2.5 pointer-coarse:after:content-[""]',
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb className="pointer-events-none block size-5 rounded-full bg-control-thumb shadow-elevation-1 transition-transform duration-(--duration-fast) ease-standard data-[state=checked]:translate-x-4 rtl:data-[state=checked]:-translate-x-4" />
    </SwitchPrimitive.Root>
  );
}
