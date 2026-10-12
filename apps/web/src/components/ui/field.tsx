import { Label as LabelPrimitive } from 'radix-ui';
import { type ComponentProps, type ReactNode, useId } from 'react';
import { CircleAlert } from 'lucide-react';
import { cn } from '@/lib/utils.ts';

/**
 * Form label.
 *
 * @param props - Radix label props.
 * @returns The label element.
 */
export function Label({ className, ...props }: ComponentProps<typeof LabelPrimitive.Root>) {
  return (
    <LabelPrimitive.Root
      data-slot="label"
      className={cn('flex select-none items-center gap-2 text-sm font-medium leading-none text-foreground', className)}
      {...props}
    />
  );
}

/**
 * Props passed to the control rendered inside a {@link Field}.
 */
export interface FieldControlProps {
  /** Id to put on the control so the label points at it. */
  id: string;
  /** Set when the field has an error. */
  'aria-invalid': true | undefined;
  /** Links the control to its hint and error text. */
  'aria-describedby': string | undefined;
}

/**
 * Props of {@link Field}.
 */
export interface FieldProps {
  /** Visible label. */
  label: ReactNode;
  /** Shown next to the label, e.g. "optional". */
  labelSuffix?: ReactNode;
  /** Help text below the control. */
  hint?: ReactNode;
  /** Error text; also marks the control invalid. */
  error?: ReactNode;
  /** Extra classes for the wrapper. */
  className?: string;
  /** Renders the control with the accessibility props it needs. */
  children: (control: FieldControlProps) => ReactNode;
}

/**
 * Label, control, hint and error wired together accessibly.
 *
 * @param props - See {@link FieldProps}.
 * @returns The field group.
 */
export function Field({ label, labelSuffix, hint, error, className, children }: FieldProps) {
  const id = useId();
  const hintId = hint ? `${id}-hint` : undefined;
  const errorId = error ? `${id}-error` : undefined;
  const describedBy = [errorId, hintId].filter(Boolean).join(' ') || undefined;

  return (
    <div data-slot="field" className={cn('flex flex-col gap-2', className)}>
      <Label htmlFor={id}>
        {label}
        {labelSuffix ? <span className="font-normal text-muted-foreground">{labelSuffix}</span> : null}
      </Label>
      {children({ id, 'aria-invalid': error ? true : undefined, 'aria-describedby': describedBy })}
      {error ? (
        <p id={errorId} className="flex items-center gap-1.5 text-xs text-destructive" role="alert">
          <CircleAlert className="size-3.5 shrink-0" aria-hidden="true" />
          {error}
        </p>
      ) : null}
      {hint ? (
        <p id={hintId} className="text-xs text-muted-foreground">
          {hint}
        </p>
      ) : null}
    </div>
  );
}
