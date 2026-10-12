import { type ClassValue, clsx } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';

// Teach tailwind-merge our custom token classes so conflicting ones are deduplicated.
const twMerge = extendTailwindMerge({
  extend: {
    classGroups: {
      shadow: [{ shadow: ['elevation-0', 'elevation-1', 'elevation-2', 'elevation-3'] }],
      animate: [
        {
          animate: ['pop-in', 'pop-out', 'fade-in', 'fade-out', 'drawer-in', 'drawer-out', 'dot-pulse', 'skeleton', 'spin'],
        },
      ],
    },
  },
});

/**
 * Joins class names and resolves Tailwind conflicts (later classes win).
 *
 * @param inputs - Class values as accepted by `clsx`.
 * @returns The merged class string.
 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
