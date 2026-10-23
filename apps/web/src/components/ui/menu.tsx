import { Check } from 'lucide-react';
import { DropdownMenu as MenuPrimitive, Popover as PopoverPrimitive, Tooltip as TooltipPrimitive } from 'radix-ui';
import type { ComponentProps, ReactNode } from 'react';
import { cn } from '@/lib/utils.ts';

const floatingSurface =
  'z-50 rounded-lg bg-popover text-popover-foreground shadow-elevation-2 data-[state=open]:animate-pop-in data-[state=closed]:animate-pop-out';

const itemClasses = cn(
  'relative flex cursor-default select-none items-center gap-2 rounded-md px-2 py-1.5 text-sm outline-none',
  'transition-colors duration-(--duration-instant) data-highlighted:bg-accent data-highlighted:text-accent-foreground',
  'data-disabled:pointer-events-none data-disabled:opacity-50 pointer-coarse:min-h-11',
  '[&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-muted-foreground',
);

/** Dropdown menu root. */
export const DropdownMenu = MenuPrimitive.Root;
/** Element that opens the dropdown menu. */
export const DropdownMenuTrigger = MenuPrimitive.Trigger;

/**
 * Floating list of menu items.
 *
 * @param props - Radix menu content props; `sideOffset` defaults to 6.
 * @returns The portalled menu.
 */
export function DropdownMenuContent({ className, sideOffset = 6, ...props }: ComponentProps<typeof MenuPrimitive.Content>) {
  return (
    <MenuPrimitive.Portal>
      <MenuPrimitive.Content
        sideOffset={sideOffset}
        className={cn(floatingSurface, 'min-w-48 origin-(--radix-dropdown-menu-content-transform-origin) p-1', className)}
        {...props}
      />
    </MenuPrimitive.Portal>
  );
}

/**
 * A menu entry. `destructive` marks irreversible actions.
 *
 * @param props - Radix menu item props plus `destructive`.
 * @returns The menu item.
 */
export function DropdownMenuItem({
  className,
  destructive,
  ...props
}: ComponentProps<typeof MenuPrimitive.Item> & { destructive?: boolean }) {
  return (
    <MenuPrimitive.Item
      className={cn(
        itemClasses,
        destructive && 'text-destructive data-highlighted:bg-destructive-subtle data-highlighted:text-destructive [&_svg]:text-destructive',
        className,
      )}
      {...props}
    />
  );
}

/** Group of menu entries of which exactly one is chosen; `value` names it. */
export const DropdownMenuRadioGroup = MenuPrimitive.RadioGroup;

/**
 * An entry of a {@link DropdownMenuRadioGroup}; the chosen one carries a
 * check and medium weight, so the choice does not rest on the icon alone.
 *
 * @param props - Radix radio item props.
 * @returns The menu item.
 */
export function DropdownMenuRadioItem({ className, children, ...props }: ComponentProps<typeof MenuPrimitive.RadioItem>) {
  return (
    <MenuPrimitive.RadioItem className={cn(itemClasses, 'pe-8 data-[state=checked]:font-medium', className)} {...props}>
      {children}
      <MenuPrimitive.ItemIndicator className="absolute end-2 flex items-center">
        <Check aria-hidden="true" />
      </MenuPrimitive.ItemIndicator>
    </MenuPrimitive.RadioItem>
  );
}

/**
 * Non-interactive heading inside a menu.
 *
 * @param props - Radix label props.
 * @returns The label.
 */
export function DropdownMenuLabel({ className, ...props }: ComponentProps<typeof MenuPrimitive.Label>) {
  return <MenuPrimitive.Label className={cn('px-2 py-1.5 text-xs font-medium text-muted-foreground', className)} {...props} />;
}

/**
 * Divider between menu groups.
 *
 * @param props - Radix separator props.
 * @returns The separator.
 */
export function DropdownMenuSeparator({ className, ...props }: ComponentProps<typeof MenuPrimitive.Separator>) {
  return <MenuPrimitive.Separator className={cn('-mx-1 my-1 h-px bg-border', className)} {...props} />;
}

/** Popover root. */
export const Popover = PopoverPrimitive.Root;
/** Element that opens the popover. */
export const PopoverTrigger = PopoverPrimitive.Trigger;
/** Positions the popover relative to another element than the trigger. */
export const PopoverAnchor = PopoverPrimitive.Anchor;

/**
 * Floating panel for rich content such as the model picker.
 *
 * @param props - Radix popover content props; `sideOffset` defaults to 8.
 * @returns The portalled popover.
 */
export function PopoverContent({ className, sideOffset = 8, ...props }: ComponentProps<typeof PopoverPrimitive.Content>) {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        sideOffset={sideOffset}
        className={cn(floatingSurface, 'origin-(--radix-popover-content-transform-origin) outline-none', className)}
        {...props}
      />
    </PopoverPrimitive.Portal>
  );
}

/**
 * Provides shared delay settings for all tooltips; render once near the root.
 */
export const TooltipProvider = TooltipPrimitive.Provider;

/**
 * Short label shown on hover and keyboard focus. Not a replacement for
 * `aria-label`: icon buttons need both.
 *
 * @param props - Content, trigger child and placement.
 * @returns The tooltip wrapping its trigger.
 */
export function Tooltip({
  content,
  children,
  side = 'top',
}: {
  content: ReactNode;
  children: ReactNode;
  side?: ComponentProps<typeof TooltipPrimitive.Content>['side'];
}) {
  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          side={side}
          sideOffset={6}
          className="z-50 max-w-64 origin-(--radix-tooltip-content-transform-origin) rounded-md bg-tooltip px-2 py-1 text-xs text-tooltip-foreground shadow-elevation-2 data-[state=closed]:animate-pop-out data-[state=delayed-open]:animate-pop-in data-[state=instant-open]:animate-fade-in"
        >
          {content}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}
