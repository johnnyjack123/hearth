import { fileURLToPath } from 'node:url';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig, type Plugin } from 'vite';

/**
 * Writes `asset-list.json`: every file of the build under `assets/`. The
 * service worker (`public/sw.js`) drops kept files that are not listed, so
 * its cache does not grow with every update.
 */
function assetList(): Plugin {
  return {
    name: 'hearth-asset-list',
    apply: 'build',
    generateBundle(_options, bundle) {
      const files = Object.keys(bundle).filter((name) => name.startsWith('assets/') && !name.endsWith('.map'));
      this.emitFile({ type: 'asset', fileName: 'asset-list.json', source: JSON.stringify(files.sort()) });
    },
  };
}

export default defineConfig({
  plugins: [react(), tailwindcss(), assetList()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    port: 5173,
    strictPort: true,
    // The API runs separately in dev; the proxy keeps cookies same-origin.
    // HEARTH_API_URL lets a second instance (e.g. end-to-end tests) use another server.
    proxy: { '/api': { target: process.env.HEARTH_API_URL ?? 'http://127.0.0.1:8080' } },
  },
  build: {
    target: 'es2022',
    sourcemap: true,
    // Mermaid and Shiki are large but lazy-loaded by Streamdown.
    chunkSizeWarningLimit: 2000,
  },
});
