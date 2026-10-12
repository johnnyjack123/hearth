import { Dialog as DialogPrimitive } from 'radix-ui';
import type { ReactNode } from 'react';
import { cn } from '@/lib/utils.ts';

/**
 * Props of {@link Sheet}.
 */
export interface SheetProps {
  /** Whether the sheet is open. */
  open: boolean;
  /** Called when the sheet wants to open or close. */
  onOpenChange: (open: boolean) => void;
  /** Accessible name, not shown visually. */
  title: string;
  /** Extra classes for the panel. */
  className?: string;
  /** Panel content. */
  children: ReactNode;
}

/**
 * Panel that slides in from the start edge, e.g. the sidebar on small
 * screens. Traps focus like a dialog.
 *
 * @param props - See {@link SheetProps}.
 * @returns The sheet.
 */
export function Sheet({ open, onOpenChange, title, className, children }: SheetProps) {
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-40 bg-overlay data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out" />
        <DialogPrimitive.Content
          aria-describedby={undefined}
          className={cn(
            'fixed inset-y-0 start-0 z-50 flex shadow-elevation-3 outline-none data-[state=open]:animate-drawer-in data-[state=closed]:animate-drawer-out',
            className,
          )}
        >
          <DialogPrimitive.Title className="sr-only">{title}</DialogPrimitive.Title>
          {children}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
