import { useMemo } from 'react';
import { encode } from 'uqr';
import { cn } from '@/lib/utils.ts';

/**
 * A QR code drawn as SVG. Its modules stay dark on a light ground in every
 * theme (`scan-ink` on `scan-surface`): many scanners do not read inverted
 * codes.
 *
 * @param props - What the code holds, its accessible label and classes for its size.
 * @returns The code.
 */
export function QrCode({ value, label, className }: { value: string; label: string; className?: string }) {
  const { size, path } = useMemo(() => {
    const { data, size } = encode(value, { ecc: 'M', border: 2 });
    const cells: string[] = [];
    data.forEach((row, y) => {
      row.forEach((dark, x) => {
        if (dark) {
          cells.push(`M${x} ${y}h1v1h-1z`);
        }
      });
    });
    return { size, path: cells.join('') };
  }, [value]);
  return (
    <svg
      role="img"
      aria-label={label}
      viewBox={`0 0 ${size} ${size}`}
      shapeRendering="crispEdges"
      className={cn('rounded-md bg-scan-surface fill-scan-ink', className)}
    >
      <path d={path} />
    </svg>
  );
}
