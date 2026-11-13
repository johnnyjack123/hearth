import { Outlet } from '@tanstack/react-router';
import { motion } from 'motion/react';
import { Logo } from '@/components/brand/logo.tsx';
import { riseIn } from '@/lib/motion.ts';

/**
 * Layout for sign-in and registration: a calm brand panel on wide screens and
 * the form card.
 *
 * @returns The auth layout with the active form in its outlet.
 */
export function AuthLayout() {
  return (
    <div className="pad-safe-area grid min-h-dvh bg-background lg:grid-cols-2">
      <div className="relative hidden overflow-hidden border-e border-border bg-sidebar lg:flex lg:flex-col lg:justify-between lg:p-10">
        <Logo withWordmark />
        <div aria-hidden="true" className="pointer-events-none absolute inset-0">
          <div className="absolute -start-1/4 top-1/4 size-3/5 rounded-full bg-primary/10 blur-3xl" />
          <div className="absolute -end-1/4 bottom-0 size-1/2 rounded-full bg-primary/5 blur-3xl" />
        </div>
      </div>
      <div className="flex items-center justify-center p-6">
        <motion.div variants={riseIn} initial="initial" animate="animate" className="flex w-full max-w-sm flex-col gap-8">
          <div className="lg:hidden">
            <Logo withWordmark />
          </div>
          <Outlet />
        </motion.div>
      </div>
    </div>
  );
}
