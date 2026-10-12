import { Tabs as TabsPrimitive, ToggleGroup as ToggleGroupPrimitive } from 'radix-ui';
import type { ComponentProps, ReactNode } from 'react';
import { cn } from '@/lib/utils.ts';

const trackClasses = 'inline-flex max-w-full items-center gap-1 overflow-x-auto rounded-lg bg-muted p-1';
const segmentClasses = cn(
  'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground pointer-coarse:min-h-11',
  'transition-[color,background-color,box-shadow] duration-(--duration-fast) ease-standard hover:text-foreground',
  'disabled:pointer-events-none disabled:opacity-50 [&_svg]:size-4',
);

/** Tabs root; controls the active tab. */
export const Tabs = TabsPrimitive.Root;

/**
 * Row of tab triggers.
 *
 * @param props - Radix tabs list props.
 * @returns The tab list.
 */
export function TabsList({ className, ...props }: ComponentProps<typeof TabsPrimitive.List>) {
  return <TabsPrimitive.List className={cn(trackClasses, className)} {...props} />;
}

/**
 * A single tab.
 *
 * @param props - Radix tab trigger props.
 * @returns The trigger.
 */
export function TabsTrigger({ className, ...props }: ComponentProps<typeof TabsPrimitive.Trigger>) {
  return (
    <TabsPrimitive.Trigger
      className={cn(
        segmentClasses,
        'data-[state=active]:bg-card data-[state=active]:text-foreground data-[state=active]:shadow-elevation-1',
        className,
      )}
      {...props}
    />
  );
}

/**
 * Panel of a tab.
 *
 * @param props - Radix tab content props.
 * @returns The panel.
 */
export function TabsContent({ className, ...props }: ComponentProps<typeof TabsPrimitive.Content>) {
  return (
    <TabsPrimitive.Content
      className={cn('outline-none data-[state=active]:animate-fade-in', className)}
      {...props}
    />
  );
}

/**
 * Single-choice segmented control for small option sets (e.g. theme).
 *
 * @param props - Value, change handler, options and accessible label.
 * @returns The toggle group.
 */
export function SegmentedControl<T extends string>({
  value,
  onValueChange,
  options,
  label,
  className,
}: {
  value: T;
  onValueChange: (value: T) => void;
  options: { value: T; label: string; icon?: ReactNode }[];
  label: string;
  className?: string;
}) {
  return (
    <ToggleGroupPrimitive.Root
      type="single"
      value={value}
      aria-label={label}
      // Radix emits "" when the active item is clicked again; keep the selection.
      onValueChange={(next) => next && onValueChange(next as T)}
      className={cn(trackClasses, className)}
    >
      {options.map((option) => (
        <ToggleGroupPrimitive.Item
          key={option.value}
          value={option.value}
          className={cn(
            segmentClasses,
            'data-[state=on]:bg-card data-[state=on]:text-foreground data-[state=on]:shadow-elevation-1',
          )}
        >
          {option.icon}
          {option.label}
        </ToggleGroupPrimitive.Item>
      ))}
    </ToggleGroupPrimitive.Root>
  );
}
