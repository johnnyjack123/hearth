import { motion } from 'motion/react';
import type { ReactNode } from 'react';
import { fade } from '@/lib/motion.ts';

/**
 * Scrollable page frame with a title for settings-like pages.
 *
 * @param props - Title, optional description and actions, and content.
 * @returns The page.
 */
export function Page({
  title,
  description,
  actions,
  children,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <motion.div
        variants={fade}
        initial="initial"
        animate="animate"
        className="mx-auto flex w-full max-w-(--content-width) flex-col gap-8 px-4 py-8 md:py-12"
      >
        <header className="flex flex-wrap items-end justify-between gap-4">
          <div className="flex flex-col gap-1">
            <h1 className="text-2xl font-semibold tracking-tight text-foreground">{title}</h1>
            {description ? <p className="text-sm text-muted-foreground">{description}</p> : null}
          </div>
          {actions}
        </header>
        {children}
      </motion.div>
    </div>
  );
}

/**
 * A titled group of related settings.
 *
 * @param props - Title, optional description and content.
 * @returns The section.
 */
export function Section({ title, description, children }: { title: ReactNode; description?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <h2 className="text-base font-semibold text-foreground">{title}</h2>
        {description ? <p className="text-sm text-muted-foreground">{description}</p> : null}
      </div>
      {children}
    </section>
  );
}

/**
 * One setting row: label and description on the left, control on the right.
 *
 * @param props - Label, optional description and the control.
 * @returns The row.
 */
export function SettingRow({ label, description, control }: { label: ReactNode; description?: ReactNode; control: ReactNode }) {
  return (
    <div className="flex flex-col gap-3 p-4 sm:flex-row sm:items-center sm:justify-between sm:gap-6">
      <div className="flex min-w-0 flex-col gap-1">
        <span className="text-sm font-medium text-foreground">{label}</span>
        {description ? <span className="text-sm text-muted-foreground">{description}</span> : null}
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  );
}
