import { X } from 'lucide-react';
import { AlertDialog as AlertDialogPrimitive, Dialog as DialogPrimitive } from 'radix-ui';
import { type ComponentProps, type ReactNode, useState } from 'react';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';
import { Button } from './button.tsx';

/** Dialog root; controls open state. */
export const Dialog = DialogPrimitive.Root;
/** Element that opens the dialog. */
export const DialogTrigger = DialogPrimitive.Trigger;
/** Element that closes the dialog. */
export const DialogClose = DialogPrimitive.Close;

const overlayClasses =
  'fixed inset-0 z-50 grid place-items-center overflow-y-auto bg-overlay p-4 data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out';
const panelClasses =
  'relative flex w-full max-w-md flex-col gap-5 rounded-xl bg-card p-6 text-card-foreground shadow-elevation-3 focus:outline-none data-[state=open]:animate-pop-in data-[state=closed]:animate-pop-out';

/**
 * Modal dialog panel with overlay and close button. The content sits inside
 * the overlay so long dialogs scroll on small screens.
 *
 * @param props - Radix dialog content props.
 * @returns The portalled dialog.
 */
export function DialogContent({ className, children, ...props }: ComponentProps<typeof DialogPrimitive.Content>) {
  const t = useT();
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className={overlayClasses}>
        <DialogPrimitive.Content data-slot="dialog-content" className={cn(panelClasses, className)} {...props}>
          {children}
          <DialogPrimitive.Close asChild>
            <Button variant="ghost" size="icon-sm" className="absolute end-3 top-3 text-muted-foreground" aria-label={t('common.close')}>
              <X />
            </Button>
          </DialogPrimitive.Close>
        </DialogPrimitive.Content>
      </DialogPrimitive.Overlay>
    </DialogPrimitive.Portal>
  );
}

/**
 * Title and description block at the top of a dialog.
 *
 * @param props - Div props.
 * @returns The header element.
 */
export function DialogHeader({ className, ...props }: ComponentProps<'div'>) {
  return <div className={cn('flex flex-col gap-1.5 pe-8', className)} {...props} />;
}

/**
 * Dialog title, announced to screen readers.
 *
 * @param props - Radix title props.
 * @returns The title element.
 */
export function DialogTitle({ className, ...props }: ComponentProps<typeof DialogPrimitive.Title>) {
  return <DialogPrimitive.Title className={cn('text-lg font-semibold tracking-tight', className)} {...props} />;
}

/**
 * Secondary text below the dialog title.
 *
 * @param props - Radix description props.
 * @returns The description element.
 */
export function DialogDescription({ className, ...props }: ComponentProps<typeof DialogPrimitive.Description>) {
  return <DialogPrimitive.Description className={cn('text-sm text-muted-foreground', className)} {...props} />;
}

/**
 * Action row at the bottom of a dialog; stacks on narrow screens with the
 * primary action on top.
 *
 * @param props - Div props.
 * @returns The footer element.
 */
export function DialogFooter({ className, ...props }: ComponentProps<'div'>) {
  return <div className={cn('flex flex-col-reverse gap-2 sm:flex-row sm:justify-end', className)} {...props} />;
}

/**
 * Props of {@link ConfirmDialog}.
 */
export interface ConfirmDialogProps {
  /** Whether the dialog is open. */
  open: boolean;
  /** Called when the dialog wants to open or close. */
  onOpenChange: (open: boolean) => void;
  /** Question in the title. */
  title: ReactNode;
  /** Consequences of confirming. */
  description: ReactNode;
  /** Label of the confirm button. */
  confirmLabel: ReactNode;
  /** Styles the confirm button as destructive. */
  destructive?: boolean;
  /** Runs on confirm; the dialog shows a spinner until it settles and closes on success. */
  onConfirm: () => Promise<void> | void;
}

/**
 * Confirmation dialog for irreversible actions. Focus starts on "cancel" so
 * an accidental Enter does not confirm.
 *
 * @param props - See {@link ConfirmDialogProps}.
 * @returns The alert dialog.
 */
export function ConfirmDialog({ open, onOpenChange, title, description, confirmLabel, destructive, onConfirm }: ConfirmDialogProps) {
  const t = useT();
  const [busy, setBusy] = useState(false);

  async function confirm() {
    setBusy(true);
    try {
      await onConfirm();
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  }

  return (
    <AlertDialogPrimitive.Root open={open} onOpenChange={(next) => !busy && onOpenChange(next)}>
      <AlertDialogPrimitive.Portal>
        <AlertDialogPrimitive.Overlay className={overlayClasses}>
          <AlertDialogPrimitive.Content className={cn(panelClasses, 'max-w-sm')}>
            <div className="flex flex-col gap-1.5">
              <AlertDialogPrimitive.Title className="text-lg font-semibold tracking-tight">{title}</AlertDialogPrimitive.Title>
              <AlertDialogPrimitive.Description className="text-sm text-muted-foreground">{description}</AlertDialogPrimitive.Description>
            </div>
            <DialogFooter>
              <AlertDialogPrimitive.Cancel asChild>
                <Button variant="ghost" disabled={busy}>
                  {t('common.cancel')}
                </Button>
              </AlertDialogPrimitive.Cancel>
              <Button
                variant={destructive ? 'destructive' : 'primary'}
                loading={busy}
                onClick={(event) => {
                  event.preventDefault();
                  void confirm();
                }}
              >
                {confirmLabel}
              </Button>
            </DialogFooter>
          </AlertDialogPrimitive.Content>
        </AlertDialogPrimitive.Overlay>
      </AlertDialogPrimitive.Portal>
    </AlertDialogPrimitive.Root>
  );
}
