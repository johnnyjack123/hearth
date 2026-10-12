import { fileURLToPath } from 'node:url';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    port: 5173,
    strictPort: true,
    // The API runs separately in dev; the proxy keeps cookies same-origin.
    // HARNESS_API_URL lets a second instance (e.g. end-to-end tests) use another server.
    proxy: { '/api': { target: process.env.HARNESS_API_URL ?? 'http://127.0.0.1:8080' } },
  },
  build: {
    target: 'es2022',
    sourcemap: true,
    // Mermaid and Shiki are large but lazy-loaded by Streamdown.
    chunkSizeWarningLimit: 2000,
  },
});
