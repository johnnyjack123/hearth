import type { Transition, Variants } from 'motion/react';

// Mirrors the motion tokens in styles/tokens.css for the Motion library, which
// needs numbers instead of CSS variables. Keep both files in sync.

/**
 * Durations in seconds, matching `--duration-*`.
 */
export const durations = {
  instant: 0.08,
  fast: 0.15,
  normal: 0.22,
  slow: 0.36,
  /** Delay between items of a staggered entrance. */
  stagger: 0.04,
} as const;

/**
 * Speeds in pixels per second. CSS cannot divide a distance by a speed, so
 * these live here only.
 */
export const speeds = {
  /** Text scrolling into view, slow enough to read along. */
  marquee: 50,
} as const;

/**
 * Cubic-bezier curves matching `--easing-*`.
 */
export const easings = {
  standard: [0.2, 0, 0, 1],
  enter: [0, 0, 0, 1],
  exit: [0.3, 0, 1, 1],
} as const satisfies Record<string, [number, number, number, number]>;

/**
 * Spring presets for layout changes and direct manipulation.
 */
export const springs = {
  /** Layout shifts in lists, e.g. a conversation moving to the top. */
  layout: { type: 'spring', stiffness: 520, damping: 42, mass: 0.9 },
  /** Panels such as the mobile sidebar drawer. */
  panel: { type: 'spring', stiffness: 380, damping: 38 },
} as const satisfies Record<string, Transition>;

/**
 * Standard transitions; exits are faster than entrances.
 */
export const transitions = {
  enter: { duration: durations.normal, ease: easings.enter },
  exit: { duration: durations.fast, ease: easings.exit },
  state: { duration: durations.fast, ease: easings.standard },
} as const satisfies Record<string, Transition>;

/**
 * Fade-and-rise used for chat messages and list items appearing.
 */
export const riseIn: Variants = {
  initial: { opacity: 0, y: 6 },
  animate: { opacity: 1, y: 0, transition: transitions.enter },
  exit: { opacity: 0, transition: transitions.exit },
};

/**
 * Plain cross-fade, e.g. for swapping page content.
 */
export const fade: Variants = {
  initial: { opacity: 0 },
  animate: { opacity: 1, transition: transitions.enter },
  exit: { opacity: 0, transition: transitions.exit },
};
