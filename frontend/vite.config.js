import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// When running `npm run dev` outside Docker, the backend is at localhost:8080.
// The Docker frontend container uses nginx to proxy instead (see nginx.conf).
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:8080',
        changeOrigin: true,
      },
      '/hls': {
        target: 'http://localhost:8088',
        changeOrigin: true,
      },
    },
  },
});
