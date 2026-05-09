import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const KAIRO_WEB_PORT = process.env.KAIRO_WEB_PORT ?? '7878';

// In dev, Vite serves the SPA on its own port and proxies
// `/api/v1/*` to a running `kairo-web` (which proxies onward to
// the daemon's Unix socket). In production, the SPA is built and
// served directly by `kairo-web` itself, so the proxy block
// only matters for `pnpm dev`.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api/v1': {
        target: `http://127.0.0.1:${KAIRO_WEB_PORT}`,
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: 'dist',
    sourcemap: true,
    target: 'es2022',
  },
});
