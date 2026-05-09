import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// When running `npm run dev` outside Docker, the backend is at localhost:8080.
// The Docker frontend container uses nginx to proxy instead (see nginx.conf).

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': {
        target: 'http://backend:8080',
        changeOrigin: true,
      },
      '/uploads': {
        target: 'http://backend:8080',
        changeOrigin: true,
      },
      '/router-api': {
        target: 'http://localhost:8070',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/router-api/, '/api'),
      },
      '/hls': {
        target: 'http://localhost:8088',
        changeOrigin: true,
      },
    },
  },
  define: {
    'process.env.REACT_APP_CLOUDFRONT_DOMAIN': JSON.stringify(process.env.REACT_APP_CLOUDFRONT_DOMAIN),
  },
});
