import { defineConfig } from 'tsup';

export default defineConfig({
  entry: ['src/index.ts'],
  format: ['esm'],
  platform: 'node',
  target: 'node22',
  outDir: 'dist',
  clean: true,
  sourcemap: true,
  // Workspace packages ship TypeScript sources, so they are bundled; npm
  // dependencies stay external and are installed in the production image.
  noExternal: [/^@harness\//],
});
